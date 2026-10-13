//! Red-gate: HueCurve CPU must evaluate fitted `curve_data`, not control-point lerp.
//!
//! Pins the C10 Hue wire (2026-08-03): `processor/apply.rs` reads the same B-spline uniforms
//! compile built for the GPU, honours user slopes, and respects `HsyTransformStyle::None`.

use vfx_ocio::{
    GradingHueCurveTransform, GroupTransform, HsyTransformStyle, Processor, Transform,
    TransformDirection,
};

fn identity_hue_transform() -> GradingHueCurveTransform {
    GradingHueCurveTransform::default()
}

/// Non-collinear HUE_SAT — lerp and quadratic B-spline agree only at control points.
fn mid_segment_hue_sat() -> GradingHueCurveTransform {
    let mut t = identity_hue_transform();
    t.hue_sat = vec![
        [0.0, 1.0],
        [1.0 / 6.0, 1.6],
        [2.0 / 6.0, 0.7],
        [0.5, 1.3],
        [4.0 / 6.0, 0.9],
        [5.0 / 6.0, 1.1],
    ];
    t
}

#[test]
fn cpu_hue_sat_moves_mid_segment_pixel() {
    let proc = Processor::from_transform(
        &Transform::GradingHueCurve(mid_segment_hue_sat()),
        TransformDirection::Forward,
    )
    .unwrap();

    // Red-ish pixel → Log HSY hue near 0; probe sits between control points 0 and 1/6.
    let mut pixels = [[0.85_f32, 0.15, 0.12]];
    let before = pixels[0];
    proc.apply_rgb(&mut pixels);
    let diff = (pixels[0][0] - before[0]).abs()
        + (pixels[0][1] - before[1]).abs()
        + (pixels[0][2] - before[2]).abs();
    assert!(
        diff > 1e-3,
        "non-identity HUE_SAT must move a mid-segment red pixel, got diff={diff}"
    );
}

#[test]
fn user_slopes_change_cpu_pixels() {
    let mut flat = mid_segment_hue_sat();
    flat.hue_sat_slopes = vec![];

    let mut steep = mid_segment_hue_sat();
    // One slope per control point — steep at the red sector.
    steep.hue_sat_slopes = vec![8.0, -8.0, 0.0, 0.0, 0.0, 0.0];

    let proc_flat = Processor::from_transform(
        &Transform::GradingHueCurve(flat),
        TransformDirection::Forward,
    )
    .unwrap();
    let proc_steep = Processor::from_transform(
        &Transform::GradingHueCurve(steep),
        TransformDirection::Forward,
    )
    .unwrap();

    let input = [[0.85_f32, 0.15, 0.12]];
    let mut a = input;
    let mut b = input;
    proc_flat.apply_rgb(&mut a);
    proc_steep.apply_rgb(&mut b);

    let diff = (a[0][0] - b[0][0]).abs() + (a[0][1] - b[0][1]).abs() + (a[0][2] - b[0][2]).abs();
    assert!(
        diff > 1e-4,
        "explicit HUE_SAT slopes must reach the CPU path, got diff={diff}"
    );
}

#[test]
fn slopes_survive_processor_group_round_trip() {
    let mut src = mid_segment_hue_sat();
    src.hue_sat_slopes = vec![2.0, -1.5, 0.5, 0.0, 0.25, -0.25];

    let proc = Processor::from_transform(
        &Transform::GradingHueCurve(src.clone()),
        TransformDirection::Forward,
    )
    .unwrap();
    let group: GroupTransform = proc.create_group_transform();
    assert_eq!(group.transforms.len(), 1);
    let Transform::GradingHueCurve(back) = &group.transforms[0] else {
        panic!("expected GradingHueCurve child");
    };
    assert_eq!(back.hue_sat_slopes.len(), src.hue_sat_slopes.len());
    for (a, b) in back.hue_sat_slopes.iter().zip(src.hue_sat_slopes.iter()) {
        assert!((a - b).abs() < 1e-5, "slope round-trip {a} vs {b}");
    }

    // Pixel path after recompile must match the original processor.
    let proc2 = Processor::from_transform(
        &Transform::GradingHueCurve(back.clone()),
        TransformDirection::Forward,
    )
    .unwrap();
    let input = [[0.85_f32, 0.15, 0.12]];
    let mut a = input;
    let mut b = input;
    proc.apply_rgb(&mut a);
    proc2.apply_rgb(&mut b);
    for i in 0..3 {
        assert!(
            (a[0][i] - b[0][i]).abs() < 1e-5,
            "recompiled slopes must preserve pixels ch{i}"
        );
    }
}

#[test]
fn hsy_none_differs_from_default_on_sat_sat() {
    let mut base = identity_hue_transform();
    base.sat_sat = vec![[0.0, 0.0], [0.5, 0.85], [1.0, 1.0]];

    let mut none = base.clone();
    none.rgb_to_hsy = HsyTransformStyle::None;
    let mut default = base;
    default.rgb_to_hsy = HsyTransformStyle::Default;

    let proc_none = Processor::from_transform(
        &Transform::GradingHueCurve(none),
        TransformDirection::Forward,
    )
    .unwrap();
    let proc_def = Processor::from_transform(
        &Transform::GradingHueCurve(default),
        TransformDirection::Forward,
    )
    .unwrap();

    // For Default this is RGB; for None it is treated as already-HSY.
    let input = [[0.2_f32, 0.55, 0.35]];
    let mut a = input;
    let mut b = input;
    proc_none.apply_rgb(&mut a);
    proc_def.apply_rgb(&mut b);
    let diff = (a[0][0] - b[0][0]).abs() + (a[0][1] - b[0][1]).abs() + (a[0][2] - b[0][2]).abs();
    assert!(
        diff > 1e-3,
        "HsyTransformStyle::None must not silently equal Default, got diff={diff}"
    );
}

#[test]
fn draw_curve_only_evaluates_hue_sat_when_otherwise_identity() {
    let mut t = identity_hue_transform();
    t.draw_curve_only = true;
    // Identity set is local_bypass for the normal path; drawCurveOnly must still graph HUE_SAT.
    // Make HUE_SAT non-flat so the graph is non-trivial mid-segment.
    t.hue_sat = vec![
        [0.0, 1.0],
        [1.0 / 6.0, 1.5],
        [2.0 / 6.0, 0.8],
        [0.5, 1.2],
        [4.0 / 6.0, 0.9],
        [5.0 / 6.0, 1.1],
    ];

    let proc =
        Processor::from_transform(&Transform::GradingHueCurve(t), TransformDirection::Forward)
            .unwrap();

    let x = 1.0 / 12.0; // mid-segment between 0 and 1/6
    let mut pixels = [[x, x, x]];
    proc.apply_rgb(&mut pixels);
    // Identity HUE_SAT would leave x unchanged at 1.0 gain domain — non-identity moves it.
    assert!(
        (pixels[0][0] - x).abs() > 1e-3,
        "drawCurveOnly must evaluate HUE_SAT mid-segment, got {}",
        pixels[0][0]
    );
    assert!((pixels[0][0] - pixels[0][1]).abs() < 1e-6);
    assert!((pixels[0][1] - pixels[0][2]).abs() < 1e-6);
}
