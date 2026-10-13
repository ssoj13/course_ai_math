//! Tests for the OCIO Config YAML parser, role/display/view lookups, and
//! processor construction.
//!
//! Extracted from `config.rs` during the STRUCT.2 split (TODO6.md).
//! Code is left at its original indentation (no dedent) because YAML raw
//! strings inside the test bodies are whitespace-sensitive — stripping
//! 4-space indents from `r#"..."#` literals corrupts the test fixtures.

use super::*;
use crate::builtin::TestSchema;

const MINIMAL_CONFIG: &str = r#"
ocio_profile_version: 2

roles:
  reference: Linear
  scene_linear: Linear
  default: sRGB

colorspaces:
  - name: Linear
    family: Scene
    encoding: scene-linear
    description: Linear reference

  - name: sRGB
    family: Display
    encoding: sdr-video
    description: sRGB display

displays:
  sRGB:
    - name: Raw
      colorspace: Linear
    - name: sRGB
      colorspace: sRGB
"#;

#[test]
fn parse_minimal_config() {
    let config = Config::from_yaml_str(MINIMAL_CONFIG, PathBuf::from(".")).unwrap();

    assert_eq!(config.version(), ConfigVersion::V2);
    assert_eq!(config.colorspaces().len(), 2);
    assert!(config.colorspace("Linear").is_some());
    assert!(config.colorspace("sRGB").is_some());
}

#[test]
fn roles_lookup() {
    let config = Config::from_yaml_str(MINIMAL_CONFIG, PathBuf::from(".")).unwrap();

    // Role should resolve to color space
    let cs = config.colorspace("scene_linear").unwrap();
    assert_eq!(cs.name(), "Linear");
}

#[test]
fn displays_parsed() {
    let config = Config::from_yaml_str(MINIMAL_CONFIG, PathBuf::from(".")).unwrap();

    let displays = config.displays();
    assert!(displays.display("sRGB").is_some());

    let display = displays.display("sRGB").unwrap();
    assert_eq!(display.views().len(), 2);
}

#[test]
fn create_processor() {
    let config = Config::from_yaml_str(MINIMAL_CONFIG, PathBuf::from(".")).unwrap();

    // Should not fail even without transforms defined
    let result = config.processor("Linear", "sRGB");
    assert!(result.is_ok());
}

#[test]
fn colorspace_not_found() {
    let config = Config::from_yaml_str(MINIMAL_CONFIG, PathBuf::from(".")).unwrap();

    let result = config.processor("NonExistent", "sRGB");
    assert!(matches!(result, Err(OcioError::ColorSpaceNotFound { .. })));
}

#[test]
fn parse_matrix_transform() {
    let yaml = r#"
ocio_profile_version: 2.1
roles:
  default: raw
colorspaces:
  - name: raw
    family: raw
  - name: WithMatrix
    from_scene_reference: !<MatrixTransform> {matrix: [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]}
displays:
  sRGB:
    - name: Raw
      colorspace: raw
"#;
    let config = Config::from_yaml_str(yaml, PathBuf::from(".")).unwrap();
    let cs = config.colorspace("WithMatrix").unwrap();
    // Assert the PARSED VALUES equal the authored YAML (4x4 identity matrix,
    // zero offset), not merely that some transform was produced. The old
    // `is_some()` check passed even if the matrix entries were dropped.
    match cs.from_reference() {
        Some(Transform::Matrix(m)) => {
            assert_eq!(
                m.matrix,
                [
                    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0
                ]
            );
            assert_eq!(m.offset, [0.0, 0.0, 0.0, 0.0]);
        }
        other => panic!("expected MatrixTransform, got {other:?}"),
    }
}

#[test]
fn parse_exponent_with_linear_scalar_gamma_offset() {
    // OCIO scalar shorthand: `gamma: 2.4` / `offset: 0.055` must expand to
    // [g, g, g, 1.0] / [o, o, o, 0.0] (OCIOYaml.cpp:1049-1056), NOT collapse
    // to the identity default. Regression for BUG4 — `yaml_f64_list` used to
    // drop scalar YAML values, turning the sRGB OETF into a silent no-op.
    let yaml = r#"
ocio_profile_version: 2.1
roles:
  default: raw
colorspaces:
  - name: raw
    family: raw
  - name: srgb_curve
    from_scene_reference: !<ExponentWithLinearTransform> {gamma: 2.4, offset: 0.055, direction: inverse}
displays:
  sRGB:
    - name: Raw
      colorspace: raw
"#;
    let config = Config::from_yaml_str(yaml, PathBuf::from(".")).unwrap();
    let cs = config.colorspace("srgb_curve").unwrap();
    match cs.from_reference() {
        Some(Transform::ExponentWithLinear(ewl)) => {
            assert_eq!(ewl.gamma, [2.4, 2.4, 2.4, 1.0]);
            assert_eq!(ewl.offset, [0.055, 0.055, 0.055, 0.0]);
        }
        other => panic!("expected ExponentWithLinear, got {other:?}"),
    }
}

#[test]
fn parse_exponent_scalar_value() {
    // Same scalar-shorthand class for `ExponentTransform` (OCIOYaml.cpp:936-942):
    // `value: 2.2` -> [2.2, 2.2, 2.2, 1.0].
    let yaml = r#"
ocio_profile_version: 2.1
roles:
  default: raw
colorspaces:
  - name: raw
    family: raw
  - name: gamma22
    from_scene_reference: !<ExponentTransform> {value: 2.2}
displays:
  sRGB:
    - name: Raw
      colorspace: raw
"#;
    let config = Config::from_yaml_str(yaml, PathBuf::from(".")).unwrap();
    let cs = config.colorspace("gamma22").unwrap();
    match cs.from_reference() {
        Some(Transform::Exponent(exp)) => {
            assert_eq!(exp.value, [2.2, 2.2, 2.2, 1.0]);
        }
        other => panic!("expected Exponent, got {other:?}"),
    }
}

#[test]
fn aces20_acescg_to_srgb_encoded_applies_oetf() {
    // S0 end-to-end regression (BUG4): ACEScg -> "sRGB Encoded Rec.709 (sRGB)"
    // must apply the AP1->AP0->Rec.709 matrix chain + sRGB OETF, not pass
    // through as identity. The matrix legs preserve neutral grey, so the OETF
    // is the only visible op for a grey input: 0.18 linear -> ~0.461 encoded.
    let config = crate::builtin::aces_2_0_studio_all_views();
    let proc = config
        .processor("ACEScg", "sRGB Encoded Rec.709 (sRGB)")
        .expect("processor build for ACEScg -> sRGB Encoded Rec.709");
    let mut px = [[0.18f32, 0.18, 0.18]];
    proc.apply_rgb(&mut px);
    assert!(
        px[0][0] > 0.45 && px[0][0] < 0.47,
        "expected ~0.461 (sRGB OETF of 0.18), got {:?} — OETF leg is a no-op",
        px[0]
    );
}

#[test]
fn parse_fixed_function_transform() {
    let yaml = r#"
ocio_profile_version: 2.1
roles:
  default: raw
colorspaces:
  - name: raw
    family: raw
  - name: ACES_with_RedMod
    from_scene_reference: !<FixedFunctionTransform> {style: ACES_RedMod10}
displays:
  sRGB:
    - name: Raw
      colorspace: raw
"#;
    let config = Config::from_yaml_str(yaml, PathBuf::from(".")).unwrap();
    let cs = config.colorspace("ACES_with_RedMod").unwrap();
    // The authored style `ACES_RedMod10` must parse to the matching enum
    // variant. The old `is_some()` check passed for any style (or a fallback).
    match cs.from_reference() {
        Some(Transform::FixedFunction(ff)) => {
            assert_eq!(ff.style, FixedFunctionStyle::AcesRedMod10);
        }
        other => panic!("expected FixedFunctionTransform, got {other:?}"),
    }
}

#[test]
fn parse_exposure_contrast_transform() {
    let yaml = r#"
ocio_profile_version: 2.1
roles:
  default: raw
colorspaces:
  - name: raw
    family: raw
  - name: Graded
    from_scene_reference: !<ExposureContrastTransform> {exposure: 1.0, contrast: 1.2, gamma: 0.9, pivot: 0.18, style: linear}
displays:
  sRGB:
    - name: Raw
      colorspace: raw
"#;
    let config = Config::from_yaml_str(yaml, PathBuf::from(".")).unwrap();
    let cs = config.colorspace("Graded").unwrap();
    // Assert every authored scalar/style is read back exactly, not just that a
    // transform exists (the old `is_some()` would pass on dropped fields).
    match cs.from_reference() {
        Some(Transform::ExposureContrast(ec)) => {
            assert_eq!(ec.exposure, 1.0);
            assert_eq!(ec.contrast, 1.2);
            assert_eq!(ec.gamma, 0.9);
            assert_eq!(ec.pivot, 0.18);
            assert_eq!(ec.style, ExposureContrastStyle::Linear);
        }
        other => panic!("expected ExposureContrastTransform, got {other:?}"),
    }
}

#[test]
fn parse_look_transform() {
    let yaml = r#"
ocio_profile_version: 2.1
roles:
  default: raw
colorspaces:
  - name: raw
    family: raw
  - name: WithLook
    from_scene_reference: !<LookTransform> {src: raw, dst: raw, looks: +FilmGrade}
displays:
  sRGB:
    - name: Raw
      colorspace: raw
"#;
    let config = Config::from_yaml_str(yaml, PathBuf::from(".")).unwrap();
    let cs = config.colorspace("WithLook").unwrap();
    // Assert the parsed src/dst/looks equal the authored YAML, not just that
    // some transform was produced.
    match cs.from_reference() {
        Some(Transform::Look(lt)) => {
            assert_eq!(lt.src, "raw");
            assert_eq!(lt.dst, "raw");
            assert_eq!(lt.looks, "+FilmGrade");
        }
        other => panic!("expected LookTransform, got {other:?}"),
    }
}

#[test]
fn parse_grading_primary_transform() {
    let yaml = r#"
ocio_profile_version: 2.1
roles:
  default: raw
colorspaces:
  - name: raw
    family: raw
  - name: Graded
    from_scene_reference: !<GradingPrimaryTransform>
      style: log
      brightness: {rgb: [0.1, 0.0, -0.1], master: 0.0}
      gamma: {rgb: [1.0, 1.05, 1.0], master: 1.0}
      contrast: {rgb: [1.1, 1.0, 0.9], master: 1.0}
      saturation: 1.2
      pivot: {contrast: 0.5}
displays:
  sRGB:
    - name: Raw
      colorspace: raw
"#;
    let config = Config::from_yaml_str(yaml, PathBuf::from(".")).unwrap();
    let cs = config.colorspace("Graded").unwrap();
    let transform = cs.from_reference().unwrap();
    if let Transform::GradingPrimary(gp) = transform {
        assert_eq!(gp.style, GradingPrimaryStyle::Log);
        assert!((gp.value.brightness.red - 0.1).abs() < 0.001);
        assert!((gp.value.saturation - 1.2).abs() < 0.001);
        assert!((gp.value.contrast.red - 1.1).abs() < 0.001);
    } else {
        panic!("Expected GradingPrimaryTransform");
    }
}

#[test]
fn parse_grading_rgb_curve_transform() {
    let yaml = r#"
ocio_profile_version: 2.1
roles:
  default: raw
colorspaces:
  - name: raw
    family: raw
  - name: Curved
    from_scene_reference: !<GradingRGBCurveTransform>
      red: {control_points: [0.0, 0.0, 0.5, 0.6, 1.0, 1.0]}
      green: {control_points: [0.0, 0.0, 1.0, 1.0]}
      blue: {control_points: [0.0, 0.1, 1.0, 0.9]}
displays:
  sRGB:
    - name: Raw
      colorspace: raw
"#;
    let config = Config::from_yaml_str(yaml, PathBuf::from(".")).unwrap();
    let cs = config.colorspace("Curved").unwrap();
    let transform = cs.from_reference().unwrap();
    if let Transform::GradingRgbCurve(gc) = transform {
        assert_eq!(gc.red.len(), 3);
        assert_eq!(gc.green.len(), 2);
        assert!((gc.red[1][1] - 0.6).abs() < 0.001);
    } else {
        panic!("Expected GradingRgbCurveTransform");
    }
}

#[test]
fn parse_grading_tone_transform() {
    let yaml = r#"
ocio_profile_version: 2.1
roles:
  default: raw
colorspaces:
  - name: raw
    family: raw
  - name: Toned
    from_scene_reference: !<GradingToneTransform>
      shadows: {rgb: [1.1, 1.0, 0.9], master: 1.0, start: 0.5, pivot: 0.0}
      midtones: {rgb: [1.0, 1.0, 1.0], master: 1.05, center: 0.4, width: 0.6}
      highlights: {rgb: [0.95, 1.0, 1.1], master: 1.0, start: 0.3, pivot: 1.0}
displays:
  sRGB:
    - name: Raw
      colorspace: raw
"#;
    let config = Config::from_yaml_str(yaml, PathBuf::from(".")).unwrap();
    let cs = config.colorspace("Toned").unwrap();
    let transform = cs.from_reference().unwrap();
    if let Transform::GradingTone(gt) = transform {
        assert!((gt.shadows.red - 1.1).abs() < 0.001);
        assert!((gt.midtones.master - 1.05).abs() < 0.001);
    } else {
        panic!("Expected GradingToneTransform");
    }
}

#[test]
fn parse_shared_views() {
    let yaml = r#"
ocio_profile_version: 2.3
roles:
  default: raw
colorspaces:
  - name: raw
    family: raw
  - name: sRGB
    family: display
shared_views:
  - name: Raw
    colorspace: raw
  - name: sRGB Display
    view_transform: ACES_1.0_SDR
    colorspace: sRGB
    looks: +FilmGrade
    description: Standard sRGB output
displays:
  Monitor:
    - name: Raw
      colorspace: raw
"#;
    let config = Config::from_yaml_str(yaml, PathBuf::from(".")).unwrap();
    assert_eq!(config.num_shared_views(), 2);
    let views = config.shared_views();
    assert_eq!(views[0].name(), "Raw");
    assert_eq!(views[1].name(), "sRGB Display");
    assert_eq!(views[1].view_transform(), Some("ACES_1.0_SDR"));
    assert_eq!(views[1].looks(), Some("+FilmGrade"));
}

#[test]
fn parse_named_transforms() {
    let yaml = r#"
ocio_profile_version: 2.1
roles:
  default: raw
colorspaces:
  - name: raw
    family: raw
named_transforms:
  - name: Utility - Exposure +1
    family: Utility
    description: Add one stop of exposure
    transform: !<ExposureContrastTransform> {exposure: 1.0, style: linear}
  - name: Utility - Rec709 to XYZ
    family: Utility
    transform: !<MatrixTransform> {matrix: [0.4124, 0.3576, 0.1805, 0, 0.2126, 0.7152, 0.0722, 0, 0.0193, 0.1192, 0.9505, 0, 0, 0, 0, 1]}
    inverse_transform: !<MatrixTransform> {matrix: [3.2406, -1.5372, -0.4986, 0, -0.9689, 1.8758, 0.0415, 0, 0.0557, -0.2040, 1.0570, 0, 0, 0, 0, 1]}
displays:
  sRGB:
    - name: Raw
      colorspace: raw
"#;
    let config = Config::from_yaml_str(yaml, PathBuf::from(".")).unwrap();
    assert_eq!(config.num_named_transforms(), 2);

    let nt1 = config.named_transform("Utility - Exposure +1").unwrap();
    assert_eq!(nt1.family, Some("Utility".to_string()));
    assert!(nt1.forward.is_some());
    assert!(nt1.inverse.is_none());

    let nt2 = config.named_transform("Utility - Rec709 to XYZ").unwrap();
    assert!(nt2.forward.is_some());
    assert!(nt2.inverse.is_some());
}

#[test]
fn parse_viewing_rules() {
    let yaml = r#"
ocio_profile_version: 2.1

environment: {}
roles:
  default: scene_linear
file_rules:
  - !<Rule> {name: Default, colorspace: default}

viewing_rules:
  - !<Rule> {name: log-encoded, encodings: [log]}
  - !<Rule> {name: scene-linear, encodings: [scene-linear]}
  - !<Rule> {name: aces-primaries, colorspaces: [ACEScg, ACES2065-1]}

colorspaces:
  - !<ColorSpace>
    name: scene_linear
    encoding: scene-linear
  - !<ColorSpace>
    name: ACEScg
    encoding: scene-linear
  - !<ColorSpace>
    name: log_cs
    encoding: log
  - !<ColorSpace>
    name: raw
    isdata: true

displays:
  sRGB:
    - name: View
      colorspace: scene_linear
"#;
    let config = Config::from_yaml_str(yaml, PathBuf::from(".")).unwrap();

    assert_eq!(config.num_viewing_rules(), 3);

    // Check log rule
    let log_rule = config.viewing_rule("log-encoded").unwrap();
    assert_eq!(log_rule.encodings, vec!["log"]);
    assert!(log_rule.colorspaces.is_empty());

    // Check scene-linear rule
    let linear_rule = config.viewing_rule("scene-linear").unwrap();
    assert_eq!(linear_rule.encodings, vec!["scene-linear"]);

    // Check colorspace rule
    let aces_rule = config.viewing_rule("aces-primaries").unwrap();
    assert_eq!(aces_rule.colorspaces, vec!["ACEScg", "ACES2065-1"]);
    assert!(aces_rule.encodings.is_empty());
}

#[test]
fn file_rule_validation() {
    // Valid config with Default rule using role reference
    let yaml = r#"
ocio_profile_version: 2
roles:
  default: sRGB
  reference: scene_linear
file_rules:
  - !<Rule> {name: exr, pattern: "*.exr", colorspace: scene_linear}
  - !<Rule> {name: Default, colorspace: default}
colorspaces:
  - !<ColorSpace>
    name: scene_linear
  - !<ColorSpace>
    name: sRGB
displays:
  sRGB:
    - name: View
      colorspace: sRGB
"#;
    let config = Config::from_yaml_str(yaml, PathBuf::from(".")).unwrap();
    let errors = config.validate();
    assert!(
        errors.is_empty(),
        "Valid config should have no errors: {:?}",
        errors
    );

    // Invalid: Default rule references non-existent colorspace/role
    let yaml_bad_cs = r#"
ocio_profile_version: 2
roles:
  default: sRGB
  reference: sRGB
file_rules:
  - !<Rule> {name: Default, colorspace: nonexistent}
colorspaces:
  - !<ColorSpace>
    name: sRGB
displays:
  sRGB:
    - name: View
      colorspace: sRGB
"#;
    let config_bad = Config::from_yaml_str(yaml_bad_cs, PathBuf::from(".")).unwrap();
    let errors = config_bad.validate();
    assert!(
        errors.iter().any(|e| e.contains("nonexistent")),
        "Should detect invalid colorspace in Default rule: {:?}",
        errors
    );
}

#[test]
fn test_expand_colorspace_transform() {
    use crate::colorspace::{ColorSpace, Encoding};
    use crate::transform::{ColorSpaceTransform, MatrixTransform, Transform, TransformDirection};

    let mut config = Config::new();

    // Create colorspaces with transforms
    // sRGB -> linear (to_reference is sRGB EOTF)
    let srgb_to_linear = Transform::Matrix(MatrixTransform {
        name: String::new(),
        matrix: [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ],
        offset: [0.1, 0.1, 0.1, 0.0], // offset to verify transform is applied
        direction: TransformDirection::Forward,
    });

    let linear_to_rec709 = Transform::Matrix(MatrixTransform {
        name: String::new(),
        matrix: [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ],
        offset: [0.2, 0.2, 0.2, 0.0], // different offset
        direction: TransformDirection::Forward,
    });

    let srgb = ColorSpace::builder("sRGB")
        .encoding(Encoding::Sdr)
        .to_reference(srgb_to_linear)
        .build();

    let rec709 = ColorSpace::builder("Rec709")
        .encoding(Encoding::Sdr)
        .from_reference(linear_to_rec709)
        .build();

    let linear = ColorSpace::builder("Linear")
        .encoding(Encoding::SceneLinear)
        .build();

    config.add_colorspace(srgb);
    config.add_colorspace(rec709);
    config.add_colorspace(linear);

    // Test ColorSpaceTransform expansion
    let cst = Transform::ColorSpace(ColorSpaceTransform {
        src: "sRGB".into(),
        dst: "Rec709".into(),
        data_bypass: true,
        direction: TransformDirection::Forward,
    });

    let expanded = config.expand_transform(&cst).unwrap();

    // Should be a group with two transforms (to_reference + from_reference)
    if let Transform::Group(gt) = &expanded {
        assert_eq!(
            gt.transforms.len(),
            2,
            "Should have 2 transforms: to_ref + from_ref"
        );
    } else {
        panic!("Expected GroupTransform, got {:?}", expanded);
    }

    // Test processor_from_transform
    let proc = config
        .processor_from_transform(&cst, TransformDirection::Forward)
        .unwrap();
    assert!(!proc.ops().is_empty(), "Processor should have operations");

    // Test inverse direction
    let cst_inv = Transform::ColorSpace(ColorSpaceTransform {
        src: "sRGB".into(),
        dst: "Rec709".into(),
        data_bypass: true,
        direction: TransformDirection::Inverse,
    });

    let expanded_inv = config.expand_transform(&cst_inv).unwrap();
    if let Transform::Group(gt) = &expanded_inv {
        // Inverse: dst.to_ref + src.from_ref (or inverse of to_ref)
        assert!(!gt.transforms.is_empty(), "Inverse should have transforms");
    }
}

#[test]
fn test_processor_from_configs() {
    // Test cross-config processor using built-in ACES
    let aces = crate::builtin::aces_2_0_studio_all_views();

    // Create a second config with aces_interchange role
    let mut studio = Config::new();
    studio.set_role(crate::role::names::ACES_INTERCHANGE, "ACES2065-1");

    // Add ACES2065-1 as interchange
    let aces2065 = ColorSpace::builder("ACES2065-1")
        .encoding(Encoding::SceneLinear)
        .build();
    studio.add_colorspace(aces2065);

    // Add a custom Log colorspace
    let log_cs = ColorSpace::builder("StudioLog")
        .encoding(Encoding::Log)
        .from_reference(Transform::Log(LogTransform {
            name: String::new(),
            base: 10.0,
            direction: TransformDirection::Forward,
        }))
        .build();
    studio.add_colorspace(log_cs);

    // Identify interchange - should find aces_interchange
    let (src_int, dst_int) = aces.identify_interchange_space(&studio).unwrap();
    assert_eq!(src_int, "ACES2065-1");
    assert_eq!(dst_int, "ACES2065-1");

    // Create cross-config processor
    let proc = aces.processor_from_configs("ACEScg", &studio, "StudioLog");
    assert!(
        proc.is_ok(),
        "Cross-config processor should work: {:?}",
        proc.err()
    );
}

#[test]
fn test_processor_from_configs_with_explicit_interchange() {
    let aces = crate::builtin::aces_2_0_studio_all_views();

    // Create second config without roles but with matching colorspace
    let mut studio = Config::new();
    let aces2065 = ColorSpace::builder("ACES2065-1")
        .encoding(Encoding::SceneLinear)
        .build();
    let linear = ColorSpace::builder("Linear")
        .encoding(Encoding::SceneLinear)
        .from_reference(Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix: [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            offset: [0.0; 4],
            direction: TransformDirection::Forward,
        }))
        .build();
    studio.add_colorspace(aces2065);
    studio.add_colorspace(linear);

    // Explicit interchange
    let proc = aces.processor_from_configs_with_interchange(
        "ACEScg",
        "ACES2065-1",
        &studio,
        "Linear",
        "ACES2065-1",
    );
    assert!(proc.is_ok(), "Explicit interchange should work");
}

#[test]
fn test_identify_builtin_colorspace() {
    let aces = crate::builtin::aces_2_0_studio_all_views();

    let result = aces.identify_builtin_colorspace(&TestSchema::shared().working_space);
    assert_eq!(result, Some("ACEScg"));

    let result = aces.identify_builtin_colorspace(&TestSchema::shared().display_encoded_srgb);
    assert_eq!(result, Some("sRGB"));
}

#[test]
fn test_processor_combine() {
    use crate::Processor;

    let aces = crate::builtin::aces_2_0_studio_all_views();

    let p1 = aces
        .processor(
            &TestSchema::shared().working_space,
            &TestSchema::shared().interchange,
        )
        .unwrap();
    let p2 = aces
        .processor(
            &TestSchema::shared().interchange,
            &TestSchema::shared().display_encoded_srgb,
        )
        .unwrap();

    let combined = Processor::combine(&p1, &p2).unwrap();
    assert!(!combined.ops().is_empty());

    let direct = aces
        .processor(
            &TestSchema::shared().working_space,
            &TestSchema::shared().display_encoded_srgb,
        )
        .unwrap();

    let mut data1 = [[0.18_f32, 0.18, 0.18]];
    let mut data2 = [[0.18_f32, 0.18, 0.18]];

    combined.apply_rgb(&mut data1);
    direct.apply_rgb(&mut data2);

    for i in 0..3 {
        assert!(
            (data1[0][i] - data2[0][i]).abs() < 0.001,
            "Combined processor should match direct: {:?} vs {:?}",
            data1,
            data2
        );
    }
}

#[test]
fn file_rule_path_search_resolves_colorspace_from_path() {
    // Ported from OpenColorIO's `FileRules, rules_test`
    // (tests/cpu/FileRules_tests.cpp). `ColorSpaceNamePathSearch` was
    // entirely unrecognized before this fix — the rule vanished from the
    // parsed config outright (loading required a `colorspace:` field this
    // rule never has), so a config depending on it silently fell straight
    // through to the next rule.
    let yaml = r#"
ocio_profile_version: 2
roles:
  default: raw
file_rules:
  - !<Rule> {name: ColorSpaceNamePathSearch}
  - !<Rule> {name: dpx file, pattern: "*", extension: dpx, colorspace: raw}
  - !<Rule> {name: Default, colorspace: default}
colorspaces:
  - !<ColorSpace>
    name: raw
  - !<ColorSpace>
    name: cs1
  - !<ColorSpace>
    name: other_cs1
displays:
  sRGB:
    - name: View
      colorspace: raw
"#;
    let config = Config::from_yaml_str(yaml, PathBuf::from(".")).unwrap();
    assert_eq!(config.file_rules().len(), 3);
    assert!(matches!(
        config.file_rules()[0].kind,
        FileRuleKind::PathSearch
    ));

    // Right-most colorspace name in the path wins.
    assert_eq!(
        config.colorspace_from_filepath("show/cs2/img_cs1.exr"),
        Some("cs1")
    );
    // Two names ending at the same position: the LONGEST one wins.
    assert_eq!(
        config.colorspace_from_filepath("show/cs1/img_other_cs1.exr"),
        Some("other_cs1")
    );
    assert_eq!(
        config.colorspace_from_filepath("show/other_cs1/img_cs1.exr"),
        Some("cs1")
    );
    // No colorspace name anywhere in the path: PathSearch doesn't match,
    // falls through to the next rule (the ".dpx" pattern rule).
    assert_eq!(
        config.colorspace_from_filepath("/mnt/user/unknown.dpx"),
        Some("raw")
    );
    // Matches neither: falls through to Default.
    assert_eq!(
        config.colorspace_from_filepath("/mnt/user/unknown.jpg"),
        Some("default")
    );
}

#[test]
fn file_rule_basic_combines_pattern_and_extension_into_one_regex() {
    // Ported from OpenColorIO's `FileRules, rules_filepattern`
    // (tests/cpu/FileRules_tests.cpp). Basic file rules match ONE combined
    // regex `^(pattern)(\.extension)$` against the whole path, not a glob
    // match on the full path plus an independent, separately-cased
    // extension comparison — see config.rs `file_rule_basic_matches`.
    fn config_with_rule(pattern: &str, extension: &str) -> Config {
        let yaml = format!(
            r#"
ocio_profile_version: 2
roles:
  default: sRGB
file_rules:
  - !<Rule> {{name: r0, pattern: "{pattern}", extension: "{extension}", colorspace: cs1}}
  - !<Rule> {{name: Default, colorspace: default}}
colorspaces:
  - !<ColorSpace>
    name: sRGB
  - !<ColorSpace>
    name: cs1
displays:
  sRGB:
    - name: View
      colorspace: sRGB
"#
        );
        Config::from_yaml_str(&yaml, PathBuf::from(".")).unwrap()
    }

    fn rule_name(config: &Config, path: &str) -> String {
        config
            .colorspace_from_filepath_with_rule(path)
            .unwrap()
            .1
            .name
            .clone()
    }

    // pattern="*", extension="[eE][xX][r]" — ref: ".exr"/.eXr" match (case
    // folded on e/x by the user's own brackets), ".EXR" does NOT (final
    // 'r' is a literal, lower-case only) and files with no extension at
    // all never match (extension is never optional).
    let c = config_with_rule("*", "[eE][xX][r]");
    assert_eq!(rule_name(&c, "/An/Arbitrary/Path/MyFile.exr"), "r0");
    assert_eq!(rule_name(&c, "/An/Arbitrary/Path/MyFile.eXr"), "r0");
    assert_eq!(rule_name(&c, "/An/Arbitrary/Path/MyFile.EXR"), "Default");
    assert_eq!(rule_name(&c, "/An/Arbitrary/Path/MyFileexr"), "Default");

    // Plain "jpg" extension IS case-insensitive by default (ref always
    // passes ignoreCase=true for the extension half) — unlike the pattern
    // half, which stays case-sensitive.
    let c = config_with_rule("*", "jpg");
    assert_eq!(rule_name(&c, "test.jpg"), "r0");
    assert_eq!(rule_name(&c, "test.Jpg"), "r0");
    assert_eq!(rule_name(&c, "test.jpG"), "r0");
    assert_eq!(rule_name(&c, "test.Jpeg"), "Default");

    // Pattern half is case-sensitive: "*gamma*" must not match "GaMma".
    let c = config_with_rule("*gamma*", "exr");
    assert_eq!(rule_name(&c, "/An/gamma/Arbitrary/Path/MyFile.exr"), "r0");
    assert_eq!(
        rule_name(&c, "/An/GaMma/Arbitrary/Path/MyFile.exr"),
        "Default"
    );

    // A `?` glob wildcard is a single character, "*ga?ma*" must not match
    // a doubled-up "gatttttttma".
    let c = config_with_rule("*ga?ma*", "exr");
    assert_eq!(rule_name(&c, "/An/gamma/Arbitrary/Path/MyFile.exr"), "r0");
    assert_eq!(
        rule_name(&c, "/An/gatttttttmaArbitrary/Path/MyFile.exr"),
        "Default"
    );

    // Bracket character classes in the pattern, including negation.
    let c = config_with_rule("*g[a-d]mma*", "exr");
    assert_eq!(rule_name(&c, "/An/gamma/Arbitrary/Path/MyFile.exr"), "r0");
    assert_eq!(rule_name(&c, "/An/gbmma/Arbitrary/Path/MyFile.exr"), "r0");
    assert_eq!(
        rule_name(&c, "/An/gemma/Arbitrary/Path/MyFile.exr"),
        "Default"
    );

    // Empty extension still requires the path to have SOME extension: the
    // ref converts an empty extension field to `(\..*)`, not "no check".
    let c = config_with_rule("*", "");
    assert_eq!(rule_name(&c, "plate.anything"), "r0");
    assert_eq!(rule_name(&c, "plate_no_ext"), "Default");
}

#[test]
fn look_transform_uses_explicit_inverse_transform_as_is_not_inverted_again() {
    // P0-b: `RunLookTokens`'s four-way switch (`LookTransform.cpp:243-269`).
    // An explicit `inverse_transform` on a Look already IS the "run
    // backward" transform, so it must be used AS-IS when a look chain runs
    // backward — not wrapped in another `Inverse`, which used to silently
    // run it forward instead. A look with only a forward transform (no
    // `inverse_transform`) is the other case: THAT one legitimately gets
    // inverted. Uses three looks with deliberately mismatched forward/
    // inverse transforms (a real inverse pair would make the two code
    // paths numerically indistinguishable) to pin all four branches.
    use crate::look::Look;
    use crate::transform::{LookTransform, MatrixTransform, Transform, TransformDirection};

    fn scale(factor: f64) -> Transform {
        Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix: [
                factor, 0.0, 0.0, 0.0, 0.0, factor, 0.0, 0.0, 0.0, 0.0, factor, 0.0, 0.0, 0.0, 0.0,
                1.0,
            ],
            offset: [0.0; 4],
            direction: TransformDirection::Forward,
        })
    }

    let mut config = Config::new();
    config.add_colorspace(
        ColorSpace::builder("scene_linear")
            .encoding(Encoding::SceneLinear)
            .build(),
    );
    config.set_role("reference", "scene_linear");
    config.set_role("default", "scene_linear");

    // Deliberately NOT a real inverse pair (x2 / x0.5) - x3 is distinct
    // from both x2 and 1/x2, so "used as-is" vs "inverted" give three
    // different, unambiguous answers.
    config.add_look(
        Look::new("look_both")
            .process_space("scene_linear")
            .transform(scale(2.0))
            .inverse_transform(scale(3.0)),
    );
    config.add_look(
        Look::new("look_fwd_only")
            .process_space("scene_linear")
            .transform(scale(2.0)),
    );
    config.add_look(
        Look::new("look_inv_only")
            .process_space("scene_linear")
            .inverse_transform(scale(3.0)),
    );

    let run = |looks: &str, direction: TransformDirection| -> f32 {
        let lt = Transform::Look(LookTransform {
            src: "scene_linear".into(),
            dst: "scene_linear".into(),
            looks: looks.into(),
            skip_color_space_conversion: true,
            direction,
        });
        let proc = config
            .processor_from_transform(&lt, TransformDirection::Forward)
            .unwrap_or_else(|e| panic!("processor for look '{looks}' ({direction:?}): {e}"));
        let mut px = [[1.0_f32, 1.0, 1.0]];
        proc.apply_rgb(&mut px);
        px[0][0]
    };

    // forward=true, look has a forward transform -> used AS-IS (x2).
    assert!(
        (run("look_both", TransformDirection::Forward) - 2.0).abs() < 1e-5,
        "forward direction must use the forward transform as-is"
    );

    // forward=false (LookTransform Inverse flips the per-look token),
    // look has inverse_transform -> used AS-IS (x3), NOT inverted to 1/3.
    assert!(
        (run("look_both", TransformDirection::Inverse) - 3.0).abs() < 1e-5,
        "inverse direction with an explicit inverse_transform must use it \
         as-is, not wrap it in another Inverse (the P0-b bug: this used to \
         produce 1/3, running the backward transform forward)"
    );

    // forward=false, only a forward transform exists -> INVERTED (1/2).
    assert!(
        (run("look_fwd_only", TransformDirection::Inverse) - 0.5).abs() < 1e-5,
        "inverse direction with no inverse_transform must invert the \
         forward transform"
    );

    // forward=true, only inverse_transform exists -> INVERTED (1/3).
    assert!(
        (run("look_inv_only", TransformDirection::Forward) - (1.0 / 3.0)).abs() < 1e-5,
        "forward direction with no forward transform must invert \
         inverse_transform, not silently drop the look (the other half of \
         P0-b: this used to produce 1.0, i.e. no-op)"
    );
}

#[test]
fn named_transform_lookup_resolves_aliases_and_ignores_case() {
    // `NamedTransform` carries the identical `aliases` field `ColorSpace`
    // has, but `Config::named_transform` used to compare only the primary
    // name, case-sensitively — a named transform reachable only by alias
    // (or a differently-cased name) was invisible to lookup, `processor()`'s
    // NamedTransform fallback, and `canonical_name`.
    use crate::config::NamedTransform;
    use crate::transform::{MatrixTransform, Transform, TransformDirection};

    let mut config = Config::new();
    let mut nt = NamedTransform::new("Camera Log to Linear");
    nt.aliases.push("cam_log_to_lin".into());
    nt.forward = Some(Transform::Matrix(MatrixTransform {
        name: String::new(),
        matrix: MatrixTransform::IDENTITY,
        offset: [0.1, 0.1, 0.1, 0.0],
        direction: TransformDirection::Forward,
    }));
    config.add_named_transform(nt);

    // Alias lookup.
    assert!(config.named_transform("cam_log_to_lin").is_some());
    // Case-insensitive primary-name lookup.
    assert!(config.named_transform("camera log to linear").is_some());
    // canonical_name resolves the alias to the primary name.
    assert_eq!(
        config.canonical_name("cam_log_to_lin"),
        "Camera Log to Linear"
    );
    // Unknown name still resolves to nothing.
    assert!(config.named_transform("not_a_real_name").is_none());
}

#[test]
fn viewing_rule_falls_through_to_encodings_when_colorspaces_dont_match() {
    // Ported from OCIO's `Config::Impl::getFilteredViews` (`Config.cpp:846-908`):
    // a rule's `colorspaces` list not matching does NOT exclude the view outright
    // — the reference falls through to ALSO check `encodings` on the same rule
    // (`if (!added && !viewEncoding.empty())`). A rule naming both lists is not
    // "colorspaces XOR encodings"; the view is included if EITHER matches.
    use crate::colorspace::{ColorSpace, Encoding};
    use crate::config::types::ViewingRule;
    use crate::display::View;

    let mut config = Config::new();
    config.add_colorspace(
        ColorSpace::builder("ACEScg")
            .encoding(Encoding::SceneLinear)
            .build(),
    );
    config.add_viewing_rule(ViewingRule {
        name: "mixed".into(),
        // Neither name matches "ACEScg" - forces the colorspaces check to miss.
        colorspaces: vec!["some_other_cs".into()],
        // But the encoding DOES match ACEScg's scene-linear encoding.
        encodings: vec!["scene-linear".into()],
        custom_keys: Vec::new(),
    });

    let view = View::new("View", "ACEScg").with_rule("mixed");
    assert!(
        config.is_display_view_applicable(&view, "ACEScg"),
        "colorspaces list missing a match must fall through to the encodings \
         check on the same rule, not exclude the view outright"
    );

    // Sanity: a colorspace with neither a listed name nor a matching encoding
    // is correctly excluded.
    config.add_colorspace(ColorSpace::builder("raw").is_data(true).build());
    assert!(!config.is_display_view_applicable(&view, "raw"));
}

#[test]
fn file_rule_regex_requires_full_path_match() {
    // OCIO's regex FileRule matches with std::regex_match — the WHOLE
    // path must match, not a substring search (FileRules.cpp:492-493).
    // A bare "exr" regex must NOT match "textures/exr_notes.txt" just
    // because it contains "exr" somewhere in the middle.
    let yaml = r#"
ocio_profile_version: 2
roles:
  default: sRGB
file_rules:
  - !<Rule> {name: log, regex: ".*_log\\.(exr|dpx)", colorspace: sRGB}
  - !<Rule> {name: bare, regex: "exr", colorspace: sRGB}
  - !<Rule> {name: Default, colorspace: default}
colorspaces:
  - !<ColorSpace>
    name: sRGB
displays:
  sRGB:
    - name: View
      colorspace: sRGB
"#;
    let config = Config::from_yaml_str(yaml, PathBuf::from(".")).unwrap();

    // Full-path match still works.
    assert_eq!(
        config.colorspace_from_filepath("shot001_log.exr"),
        Some("sRGB")
    );

    // A substring-only match must fall through to Default, not hit the
    // "bare" or "log" rules.
    let (_, rule) = config
        .colorspace_from_filepath_with_rule("textures/exr_notes.txt")
        .unwrap();
    assert_eq!(rule.name, "Default");

    let (_, rule) = config
        .colorspace_from_filepath_with_rule("plate_lin.exr")
        .unwrap();
    assert_eq!(rule.name, "Default");
}

#[test]
fn test_processor_inverse() {
    let aces = crate::builtin::aces_2_0_studio_all_views();

    let forward = aces
        .processor(
            &TestSchema::shared().working_space,
            &TestSchema::shared().display_encoded_srgb,
        )
        .unwrap();
    let inverse = forward.inverse().unwrap();

    let original = [0.18_f32, 0.18, 0.18];
    let mut data = [original];

    forward.apply_rgb(&mut data);
    inverse.apply_rgb(&mut data);

    for i in 0..3 {
        assert!(
            (data[0][i] - original[i]).abs() < 0.01,
            "Round-trip should preserve values: {:?} vs {:?}",
            data[0],
            original
        );
    }
}

/// B23: a config's name and description are two fields, as in OCIO (`Config::getName` /
/// `getDescription`, loaded from the `name:` and `description:` keys, both written back by
/// `serialize`). Expected values from PyOpenColorIO 2.5.1 on the real config this crate embeds:
/// ```text
/// c = PyOpenColorIO.Config.CreateFromFile(
///     'crates/ocio/vfx-ocio/src/builtin/data/studio-config-all-views-v4.0.0_aces-v2.0_ocio-v2.5.ocio')
/// repr(c.getName()); repr(c.getDescription())
/// repr(c.getColorSpace('ACEScg').getDescription())
/// ```
#[test]
fn name_and_description_match_pyocio_on_the_studio_config() {
    const NAME: &str = "studio-config-all-views-v4.0.0_aces-v2.0_ocio-v2.5";
    const DESCRIPTION: &str = "Academy Color Encoding System - Studio Config (All Views) [COLORSPACES v4.0.0] [ACES v2.0] [OCIO v2.5]\n------------------------------------------------------------------------------------------------------\n\nThis \"OpenColorIO\" config is geared toward studios requiring a config that includes a wide variety of camera colorspaces, displays and looks.";
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/builtin/data/studio-config-all-views-v4.0.0_aces-v2.0_ocio-v2.5.ocio"
    );
    let config = Config::from_file(path).expect("studio config");
    assert_eq!(config.name(), NAME);
    assert_eq!(config.description(), DESCRIPTION);
    assert_eq!(
        config.colorspace("ACEScg").unwrap().description(),
        "Convert ACEScg to ACES2065-1"
    );

    // Serialized and read back, both survive (a multi-line description is a literal block).
    let yaml = config.serialize().unwrap();
    let again = Config::from_yaml_str(&yaml, std::path::PathBuf::from("."))
        .expect("re-parse serialized config");
    assert_eq!(again.name(), NAME);
    assert_eq!(again.description(), DESCRIPTION);

    // Setting one does not touch the other.
    let mut c = again;
    c.set_description("one\ntwo");
    assert_eq!(c.name(), NAME);
    assert_eq!(c.description(), "one\ntwo");
    c.set_name("renamed");
    assert_eq!(c.description(), "one\ntwo");
}

/// The builder's `description` reaches `Config::description`, not `name`.
#[test]
fn builder_description_is_the_description() {
    let config = crate::ConfigBuilder::new("TestConfig")
        .description("Test config")
        .add_colorspace(crate::ColorSpace::builder("Linear").build())
        .set_role("reference", "Linear")
        .build()
        .unwrap();
    assert_eq!(config.name(), "TestConfig");
    assert_eq!(config.description(), "Test config");
}

/// B24: loading `ExponentWithLinearTransform {style: clamp | pass_thru}` fails as in OCIO.
/// PyOpenColorIO 2.5.1 `Config.CreateFromStream` on this YAML with `style: clamp` ->
/// "Loading the OCIO profile failed. Clamp negative extrapolation is not valid for MonCurve
/// exponent style." (`pass_thru`: "Pass thru ..."); `mirror` and `linear` load.
#[test]
fn exponent_with_linear_style_is_validated_on_load() {
    let yaml = |style: &str| {
        format!(
            "ocio_profile_version: 2\nroles:\n  default: raw\ncolorspaces:\n  - !<ColorSpace>\n    name: raw\n  - !<ColorSpace>\n    name: srgbish\n    from_scene_reference: !<ExponentWithLinearTransform> {{gamma: [2.4, 2.4, 2.4, 1], offset: [0.055, 0.055, 0.055, 0], style: {style}}}\n"
        )
    };
    for (style, msg) in [
        (
            "clamp",
            "Clamp negative extrapolation is not valid for MonCurve exponent style.",
        ),
        (
            "pass_thru",
            "Pass thru negative extrapolation is not valid for MonCurve exponent style.",
        ),
    ] {
        let err = Config::from_yaml_str(&yaml(style), std::path::PathBuf::from("."))
            .err()
            .unwrap_or_else(|| panic!("style {style} was accepted"));
        assert!(err.to_string().contains(msg), "{style}: {err}");
    }
    for style in ["mirror", "linear"] {
        Config::from_yaml_str(&yaml(style), std::path::PathBuf::from("."))
            .unwrap_or_else(|e| panic!("style {style}: {e}"));
    }
}

/// A config transform with an unknown tag, an untagged mapping, or an enum string OCIO does not
/// know fails the load, with OCIO's message; the spellings OCIO accepts (case-insensitive) load.
/// PyOpenColorIO 2.5.1 `Config.CreateFromStream` on each fragment. The reader used to drop the
/// first two (empty slot) and map every unknown string to a default (an unknown FixedFunction
/// style became ACES_RedMod03, which also swallowed OCIO's own `ACES2_OutputTransform`).
#[test]
fn unknown_transform_tags_and_styles_fail_the_load_as_in_ocio() {
    let yaml = |t: &str| {
        format!(
            "ocio_profile_version: 2\nroles:\n  default: raw\ncolorspaces:\n  - !<ColorSpace>\n    name: raw\n  - !<ColorSpace>\n    name: s\n    from_scene_reference: {t}\n"
        )
    };
    let load = |t: &str| Config::from_yaml_str(&yaml(t), std::path::PathBuf::from("."));
    #[rustfmt::skip]
    let rejected = [
        ("!<ExponentTransform> {value: [2,2,2,1], style: bogus}", "Unknown exponent style: 'bogus'."),
        ("!<ExponentWithLinearTransform> {gamma: [2.4,2.4,2.4,1], offset: [0.05,0.05,0.05,0], style: bogus}", "Unknown exponent style: 'bogus'."),
        ("!<ExponentTransform> {value: [2,2,2,1], style: passThru}", "Unknown exponent style: 'passThru'."),
        ("!<FooTransform> {}", "Unsupported transform type !<FooTransform>"),
        ("{children: []}", "Unsupported transform type"),
        ("!<CDLTransform> {style: bogus}", "Wrong CDL style: 'bogus'."),
        ("!<CDLTransform> {style: no_clamp}", "Wrong CDL style: 'no_clamp'."),
        ("!<RangeTransform> {min_in_value: 0, min_out_value: 0, style: bogus}", "Wrong Range style 'bogus'."),
        ("!<ExposureContrastTransform> {style: bogus}", "Unknown exposure contrast style: 'bogus'."),
        ("!<LogTransform> {base: 2, direction: bogus}", "Unrecognized transform direction: 'bogus'."),
        ("!<FixedFunctionTransform> {style: bogus}", "Unknown Fixed FunctionOp style: 'bogus'."),
        ("!<FixedFunctionTransform> {style: ACES_RED_MOD_03}", "Unknown Fixed FunctionOp style: 'ACES_RED_MOD_03'."),
        ("!<GradingPrimaryTransform> {style: bogus}", "Unknown grading style: 'bogus'."),
        // OCIO's YAML has no inline LUTs; these were invented here (and the 3D one ignored its
        // data and built an identity).
        ("!<Lut1DTransform> {length: 3, values: [0, 0.2, 1]}", "Unsupported transform type !<Lut1DTransform>"),
        ("!<Lut3DTransform> {gridSize: 2}", "Unsupported transform type !<Lut3DTransform>"),
    ];
    for (t, msg) in rejected {
        let err = load(t).err().unwrap_or_else(|| panic!("{t} loaded"));
        assert!(err.to_string().contains(msg), "{t}: {err}");
    }
    for t in [
        "!<ExponentTransform> {value: [2,2,2,1], style: Mirror}",
        "!<ExponentTransform> {value: [2,2,2,1], style: PASS_THRU}",
        "!<LogTransform> {base: 2, direction: INVERSE}",
        "!<FixedFunctionTransform> {style: ACES_RedMod03}",
        "!<FixedFunctionTransform> {style: ACES_REDMOD03}",
        "!<CDLTransform> {style: NoClamp}",
        "!<MatrixTransform> {bogus_key: 1}",
    ] {
        load(t).unwrap_or_else(|e| panic!("{t}: {e}"));
    }
    // OCIO's own spelling of the ACES 2 output transform is that style, not a fallback.
    let c = load("!<FixedFunctionTransform> {style: ACES2_OutputTransform, params: [100, 0.3127, 0.329, 0.64, 0.33, 0.3, 0.6, 0.15, 0.06]}")
        .unwrap();
    match c.colorspace("s").unwrap().from_reference() {
        Some(Transform::FixedFunction(ff)) => {
            assert_eq!(ff.style, FixedFunctionStyle::AcesOutputTransform20)
        }
        other => panic!("{other:?}"),
    }
}

/// One `AllocationType` for colour spaces and `AllocationTransform` (OCIO has one `Allocation`),
/// parsed as OCIO `AllocationFromString`: `uniform` / `lg2` in any case; anything else (the
/// invented `log2` / `log` included) loads as unknown and fails at processor creation with OCIO's
/// `BuildAllocationOp` message.
/// PyOpenColorIO 2.5.1 on the same config.
#[test]
fn allocation_names_parse_as_ocio() {
    let yaml = |a: &str| {
        format!(
            "ocio_profile_version: 2\nroles: {{default: raw}}\ncolorspaces:\n  - !<ColorSpace>\n    name: raw\n  - !<ColorSpace>\n    name: x\n    allocation: {a}\n    allocationvars: [-8, 5]\n    from_scene_reference: !<AllocationTransform> {{allocation: {a}, vars: [-8, 5]}}\n"
        )
    };
    for (name, want) in [
        ("lg2", AllocationType::Log2),
        ("LG2", AllocationType::Log2),
        ("uniform", AllocationType::Uniform),
        ("log2", AllocationType::Unknown),
        ("log", AllocationType::Unknown),
        ("bogus", AllocationType::Unknown),
    ] {
        let config = Config::from_yaml_str(&yaml(name), PathBuf::from(".")).unwrap();
        let cs = config.colorspace("x").unwrap();
        assert_eq!(cs.allocation().alloc_type, want, "{name}");
        let proc = config.processor("raw", "x");
        if want == AllocationType::Unknown {
            let err = proc.err().map(|e| e.to_string()).unwrap_or_default();
            assert!(
                err.contains("Unsupported Allocation Type."),
                "{name}: {err}"
            );
        } else {
            proc.unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }
}

/// Every scene-referred colour space of the studio config converts to ACES2065-1 as in OCIO.
/// The YAML loader read LogCamera/LogAffine parameters under invented camelCase keys
/// (`logSideSlope`, ...) where OCIO and every OCIO config write `log_side_slope`, so each camera
/// log space loaded with default parameters (ARRI LogC3 gave [2.79, 3.02, 4.03] for OCIO's
/// [0.365, 0.404, 1.366]). PyOpenColorIO 2.5.1 `getProcessor(cs, "ACES2065-1")` on [0.4, 0.5, 0.6].
#[test]
#[rustfmt::skip]
fn studio_config_scene_spaces_match_pyocio() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/builtin/data/studio-config-all-views-v4.0.0_aces-v2.0_ocio-v2.5.ocio");
    let config = Config::from_file(path).unwrap();
    let rows: [(&str, [f32; 3]); 47] = [
        ("ACES2065-1", [0.4, 0.5, 0.6]),
        ("ACEScc", [0.4621921, 0.6141737, 1.735304]),
        ("ACEScct", [0.4621907, 0.6141708, 1.735298]),
        ("ACEScg", [0.4468417, 0.505074, 0.6007027]),
        ("ADX10", [0.2373969, 0.3041282, 0.5021853]),
        ("ADX16", [25248.04, 58863.95, 64708.48]),
        ("Apple Log", [0.1627776, 0.2183239, 0.4522759]),
        ("ARRI LogC3 (EI800)", [0.3651524, 0.403577, 1.365625]),
        ("Linear ARRI Wide Gamut 3", [0.4403453, 0.4811699, 0.605845]),
        ("ARRI LogC4", [1.550513, 2.168611, 6.488118]),
        ("Linear ARRI Wide Gamut 4", [0.4353662, 0.4990958, 0.6001855]),
        ("BMDFilm WideGamut Gen5", [0.5580525, 0.5330252, 2.410112]),
        ("Linear BMD WideGamut Gen5", [0.4463222, 0.4844569, 0.6135703]),
        ("DaVinci Intermediate WideGamut", [0.5888653, 0.6803817, 2.622966]),
        ("Linear DaVinci WideGamut", [0.4335765, 0.4846411, 0.6310772]),
        ("CanonLog2 CinemaGamut D55", [0.3268798, 0.4022492, 1.479545]),
        ("Linear CinemaGamut D55", [0.432485, 0.4885725, 0.6237199]),
        ("CanonLog3 CinemaGamut D55", [0.4021299, 0.5178787, 1.269609]),
        ("D-Log D-Gamut", [0.3335721, 0.3904359, 1.220203]),
        ("Linear D-Gamut", [0.4403059, 0.4855939, 0.6111977]),
        ("V-Log V-Gamut", [0.2765566, 0.3742562, 1.017181]),
        ("Linear V-Gamut", [0.4383851, 0.4972311, 0.6019528]),
        ("Log3G10 REDWideGamutRGB", [0.778295, 0.8382041, 3.826413]),
        ("Linear REDWideGamutRGB", [0.4346024, 0.4865755, 0.6462111]),
        ("S-Log3 S-Gamut3", [0.2895564, 0.3867686, 1.042933]),
        ("S-Log3 S-Gamut3.Cine", [0.3091993, 0.3639451, 1.079366]),
        ("S-Log3 Venice S-Gamut3", [0.2880357, 0.3813128, 1.045349]),
        ("S-Log3 Venice S-Gamut3.Cine", [0.3103816, 0.3575463, 1.081343]),
        ("Linear S-Gamut3", [0.4350665, 0.4941266, 0.6015462]),
        ("Linear S-Gamut3.Cine", [0.4450071, 0.4919759, 0.6086247]),
        ("Linear Venice S-Gamut3", [0.4324262, 0.4936126, 0.602496]),
        ("Linear Venice S-Gamut3.Cine", [0.4430915, 0.4912668, 0.6094357]),
        ("sRGB Encoded Rec.709 (sRGB)", [0.1968928, 0.2168657, 0.3036388]),
        ("Gamma 1.8 Encoded Rec.709", [0.2652006, 0.2894468, 0.3826626]),
        ("Gamma 2.2 Encoded Rec.709", [0.1995727, 0.2204566, 0.309698]),
        ("Gamma 2.4 Encoded Rec.709", [0.1733771, 0.1924811, 0.2786713]),
        ("Camera Rec.709", [0.2403608, 0.2620946, 0.350607]),
        ("sRGB Encoded P3-D65", [0.1922782, 0.2191517, 0.3140329]),
        ("Gamma 2.2 Encoded AdobeRGB", [0.1857099, 0.2179995, 0.3132516]),
        ("sRGB Encoded AP1", [0.174717, 0.2203861, 0.3191599]),
        ("Gamma 2.2 Encoded AP1", [0.176522, 0.2241204, 0.325671]),
        ("CIE XYZ-D65 - Scene-referred", [0.4191186, 0.5420843, 0.5510467]),
        ("Linear AdobeRGB", [0.457023, 0.4975399, 0.5883771]),
        ("Linear P3-D65", [0.4675876, 0.5032436, 0.5956807]),
        ("Linear Rec.2020", [0.4484128, 0.5048941, 0.597268]),
        ("Linear Rec.709 (sRGB)", [0.4737746, 0.5007008, 0.5853371]),
        ("Raw", [0.4, 0.5, 0.6]),
    ];
    let mut bad = Vec::new();
    for (name, want) in rows {
        let p = match config.processor(name, "ACES2065-1") {
            Ok(p) => p,
            Err(e) => {
                bad.push(format!("{name}: {e}"));
                continue;
            }
        };
        let mut px = [[0.4f32, 0.5, 0.6]];
        p.apply_rgb(&mut px);
        if (0..3).any(|c| (px[0][c] - want[c]).abs() > 1e-4 * want[c].abs().max(1.0)) {
            bad.push(format!("{name}: got {:?}, PyOCIO {want:?}", px[0]));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("
"));
}

/// The YAML keys OCIO writes (`OCIOYaml.cpp` load/save): `data_bypass`, `looks_bypass`,
/// `lintolog_bypass`, `s_contrast`. The reader used invented spellings (`dataBypass`,
/// `looksBypass`, `bypass_lin_to_log`, `scontrast`), so these settings in a real config were
/// silently dropped.
#[test]
fn transform_keys_are_ocios() {
    let yaml = "ocio_profile_version: 2\nroles: {default: a}\ndisplays:\n  d:\n    - !<View> {name: v, colorspace: a}\ncolorspaces:\n  - !<ColorSpace>\n    name: a\n  - !<ColorSpace>\n    name: b\n    from_scene_reference: !<GroupTransform>\n      children:\n        - !<ColorSpaceTransform> {src: a, dst: a, data_bypass: false}\n        - !<DisplayViewTransform> {src: a, display: d, view: v, looks_bypass: true, data_bypass: false}\n        - !<GradingRGBCurveTransform> {style: linear, lintolog_bypass: true}\n        - !<GradingToneTransform> {style: linear, s_contrast: 1.3}\n";
    let config = Config::from_yaml_str(yaml, PathBuf::from(".")).unwrap();
    let t = format!(
        "{:?}",
        config.colorspace("b").unwrap().from_reference().unwrap()
    );
    for want in [
        "data_bypass: false, direction: Forward }), DisplayView",
        "looks_bypass: true, data_bypass: false",
        "bypass_lin_to_log: true",
        "scontrast: 1.3",
    ] {
        assert!(t.contains(want), "missing {want:?} in {t}");
    }
}

/// `serialize` writes every transform (it wrote none: no colour-space, look, view-transform or
/// named-transform transforms, no view_transforms / display_colorspaces / named_transforms /
/// shared_views / viewing_rules / virtual_display sections), so the studio config read back from
/// its own serialization gives the same pixels for every colour space, display/view and look.
/// (PyOpenColorIO 2.5.1 loads the same text and matches its own processors on the original.)
#[test]
fn serialize_round_trips_the_studio_config() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/builtin/data/studio-config-all-views-v4.0.0_aces-v2.0_ocio-v2.5.ocio"
    );
    let a = Config::from_file(path).unwrap();
    let yaml = a.serialize().unwrap();
    let b =
        Config::from_yaml_str(&yaml, PathBuf::from(".")).unwrap_or_else(|e| panic!("{e}\n{yaml}"));
    assert_eq!(a.colorspaces().len(), b.colorspaces().len());
    assert_eq!(a.view_transforms().len(), b.view_transforms().len());
    assert_eq!(a.named_transforms().len(), b.named_transforms().len());
    assert_eq!(a.shared_views().len(), b.shared_views().len());
    const PX: [[f32; 3]; 3] = [[0.4, 0.5, 0.6], [0.02, 0.18, 0.9], [2.0, 0.01, 0.3]];
    let same = |what: &str, pa: OcioResult<crate::Processor>, pb: OcioResult<crate::Processor>| {
        // What the original cannot build (B31: half-domain Lut1D inverse) must fail the same way.
        let (pa, pb) = match (pa, pb) {
            (Ok(pa), Ok(pb)) => (pa, pb),
            (Err(ea), Err(eb)) => {
                assert_eq!(ea.to_string(), eb.to_string(), "{what}");
                return false;
            }
            (a, b) => panic!("{what}: original {:?} vs reloaded {:?}", a.err(), b.err()),
        };
        let (mut x, mut y) = (PX, PX);
        pa.apply_rgb(&mut x);
        pb.apply_rgb(&mut y);
        for (p, q) in x.iter().zip(&y) {
            for c in 0..3 {
                assert!(
                    (p[c] - q[c]).abs() <= 1e-6 * p[c].abs().max(1.0)
                        || (p[c].is_nan() && q[c].is_nan()),
                    "{what}: {x:?} vs {y:?}"
                );
            }
        }
        true
    };
    let mut checked = 0;
    for cs in a.colorspaces() {
        let name = cs.name();
        checked += same(
            name,
            a.processor(name, "ACES2065-1"),
            b.processor(name, "ACES2065-1"),
        ) as usize;
        checked += same(
            name,
            a.processor("ACES2065-1", name),
            b.processor("ACES2065-1", name),
        ) as usize;
    }
    for display in a.displays().displays() {
        for view in a.get_views(display.name()) {
            let what = format!("{} / {}", display.name(), view.name());
            checked += same(
                &what,
                a.display_processor("ACEScg", display.name(), view.name()),
                b.display_processor("ACEScg", display.name(), view.name()),
            ) as usize;
        }
    }
    for look in a.looks().all() {
        checked += same(
            look.name(),
            a.processor_with_looks("ACEScg", "ACEScct", look.name()),
            b.processor_with_looks("ACEScg", "ACEScct", look.name()),
        ) as usize;
    }
    assert!(checked > 100, "{checked}");
}

/// Every transform type `serialize` writes reads back to the same transform, with non-default
/// parameters and inverse directions (compared through `Debug`).
#[test]
fn serialize_round_trips_every_transform_type() {
    let yaml = r#"ocio_profile_version: 2
roles: {default: a}
file_rules:
  - !<Rule> {name: Default, colorspace: a}
viewing_rules:
  - !<Rule> {name: r, colorspaces: [a], custom: {k: "v, w"}}
colorspaces:
  - !<ColorSpace>
    name: a
  - !<ColorSpace>
    name: "b, with: odd name"
    interchange:
      amf_transform_ids: "x\ny\n"
    from_scene_reference: !<GroupTransform>
      direction: inverse
      children:
        - !<MatrixTransform> {matrix: [1.1, 0.1, 0, 0, 0, 0.9, 0.1, 0, 0.2, 0, 1, 0, 0, 0, 0, 1], offset: [0.01, 0, -0.02, 0], direction: inverse}
        - !<ExponentTransform> {value: [2.2, 2.3, 2.4, 1], style: mirror}
        - !<ExponentWithLinearTransform> {gamma: [2.4, 2.4, 2.4, 1], offset: [0.055, 0.055, 0.055, 0], style: mirror, direction: inverse}
        - !<LogTransform> {base: 10, direction: inverse}
        - !<LogAffineTransform> {base: 10, log_side_slope: [0.3, 0.31, 0.32], log_side_offset: 0.6, lin_side_slope: 1.1, lin_side_offset: 0.01}
        - !<LogCameraTransform> {base: 10, log_side_slope: 0.25, log_side_offset: 0.39, lin_side_slope: 5.6, lin_side_offset: 0.05, lin_side_break: 0.011, linear_slope: [5.3, 5.3, 5.3], direction: inverse}
        - !<CDLTransform> {slope: [1.1, 1, 0.9], offset: [0.01, 0, -0.01], power: [1.2, 1, 0.8], saturation: 0.9, style: noclamp}
        - !<RangeTransform> {min_in_value: -0.1, max_in_value: 1.1, min_out_value: 0, max_out_value: 1, style: noClamp}
        - !<RangeTransform> {min_in_value: 0.1}
        - !<FixedFunctionTransform> {style: RGB_TO_HSV, direction: inverse}
        - !<FixedFunctionTransform> {style: ACES_GamutComp13, params: [1.147, 1.264, 1.312, 0.815, 0.803, 0.88, 1.2]}
        - !<ExposureContrastTransform> {style: video, exposure: 0.5, contrast: 1.2, gamma: 1.1, pivot: 0.2, log_exposure_step: 0.1, log_midway_gray: 0.4}
        - !<ColorSpaceTransform> {src: a, dst: a, data_bypass: false}
        - !<BuiltinTransform> {style: ACEScct_to_ACES2065-1, direction: inverse}
        - !<AllocationTransform> {allocation: lg2, vars: [-8, 5, 0.00390625]}
        - !<FileTransform> {src: lut.cube, cccid: grade, cdl_style: asc, interpolation: tetrahedral, direction: inverse}
"#;
    let a = Config::from_yaml_str(yaml, PathBuf::from("/some/dir")).unwrap();
    let out = a.serialize().unwrap();
    let b = Config::from_yaml_str(&out, PathBuf::from("/some/dir"))
        .unwrap_or_else(|e| panic!("{e}\n{out}"));
    let t = |c: &Config| {
        format!(
            "{:?}",
            c.colorspace("b, with: odd name")
                .unwrap()
                .from_reference()
                .unwrap()
        )
    };
    assert_eq!(t(&a), t(&b), "\n{out}");
    assert!(
        t(&a).contains("direction: Inverse }"),
        "group direction lost: {}",
        t(&a)
    );
    let cs = b.colorspace("b, with: odd name").unwrap();
    // OCIO sanitises the trailing newline (`SanitizeNewlines`).
    assert_eq!(cs.interchange_attribute("amf_transform_ids"), Some("x\ny"));
    assert_eq!(
        format!("{:?}", a.viewing_rules()),
        format!("{:?}", b.viewing_rules())
    );
    assert!(out.contains("custom: {k: \"v, w\"}"), "{out}");
}

/// A transform OCIO's YAML cannot hold is an error, not a silently dropped slot.
#[test]
fn serialize_refuses_what_ocio_yaml_cannot_hold() {
    let mut config = Config::create_raw();
    let cs = crate::ColorSpace::builder("g")
        .from_reference(Transform::Lut1D(
            crate::transform::Lut1DTransform::identity(2),
        ))
        .build();
    config.add_colorspace(cs);
    let err = config.serialize().unwrap_err().to_string();
    assert!(err.contains("inline LUT"), "{err}");
}

/// Name lookup as OCIO's `Config::getColorSpace`: a colour space name or alias first, then a
/// role, roles matched case-insensitively (`LookupRole` lowercases); resolving returns the
/// colour space's own name, also for a role that points at an alias.
#[test]
fn colorspace_lookup_order_matches_ocio() {
    let yaml = r#"ocio_profile_version: 2
roles:
  scene_linear: lin_alias
  shadowed: B
colorspaces:
  - !<ColorSpace>
    name: A
    aliases: [lin_alias]
  - !<ColorSpace>
    name: B
  - !<ColorSpace>
    name: shadowed
"#;
    let config = Config::from_yaml_str(yaml, PathBuf::from(".")).unwrap();
    assert_eq!(
        config.colorspace("SCENE_LINEAR").map(|cs| cs.name()),
        Some("A")
    );
    assert_eq!(config.resolve_colorspace_name("scene_linear"), Some("A"));
    assert_eq!(config.canonical_name("Scene_Linear"), "A");
    // A colour space named like a role wins over the role.
    assert_eq!(
        config.colorspace("shadowed").map(|cs| cs.name()),
        Some("shadowed")
    );
    assert_eq!(config.resolve_colorspace_name("nope"), None);
}
