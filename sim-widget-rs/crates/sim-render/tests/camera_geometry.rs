use glam::{DVec2, DVec3};
use sim_render::{
    bounds::Bounds,
    camera::{Camera, Projection},
    geometry::{sample_curve, sample_surface, triangulate},
};
#[test]
fn real_projection_and_rays_agree_at_two_depths() {
    let camera = Camera::default();
    let m = camera.matrices([800.0, 600.0]).unwrap();
    let p = DVec3::new(1.0, 1.0, 0.0);
    let farther = DVec3::new(1.0, 1.0, -3.0);
    assert!((m.project(p).unwrap().x - m.project(farther).unwrap().x).abs() < 1e-10);
    let screen = m.project(p).unwrap();
    assert!((m.unproject(screen.truncate(), screen.z).unwrap() - p).length() < 1e-10);
    let ray = m.ray(screen.truncate()).unwrap();
    assert!((p - ray.origin).cross(ray.direction).length() < 1e-9);
    let perspective = Camera {
        projection: Projection::Perspective { vertical_fov: 0.8 },
        ..camera
    }
    .matrices([800.0, 600.0])
    .unwrap();
    assert!(perspective.project(p).unwrap().x > perspective.project(farther).unwrap().x);
    let near = perspective.project(DVec3::new(0.0, 0.0, 9.99)).unwrap();
    assert!((near.z - 1.0).abs() < 1e-9);
    assert!(perspective.project(DVec3::new(0.0, 0.0, 9.999)).is_none());
}
#[test]
fn fit_padding_and_invalid_views() {
    let bounds =
        Bounds::from_points([DVec3::new(-3.0, -1.0, -1.0), DVec3::new(4.0, 3.0, 1.0)]).unwrap();
    for projection in [
        Projection::Orthographic { vertical_span: 8.0 },
        Projection::Perspective { vertical_fov: 0.8 },
    ] {
        let cam = Camera {
            projection,
            ..Camera::default()
        }
        .fit(bounds, [800.0, 600.0], 40.0)
        .unwrap();
        let m = cam.matrices([800.0, 600.0]).unwrap();
        for point in bounds.corners() {
            let s = m.project(point).unwrap();
            assert!(s.x >= 39.999 && s.x <= 760.001 && s.y >= 39.999 && s.y <= 560.001);
        }
    }
    assert!(
        Camera {
            eye: [0.0; 3],
            ..Camera::default()
        }
        .matrices([800.0, 600.0])
        .is_err()
    );
    assert!(Camera::default().matrices([0.0, 600.0]).is_err());
    assert!(
        Camera::default()
            .matrices([800.0, 600.0])
            .unwrap()
            .unproject(DVec2::ZERO, f64::NAN)
            .is_err()
    );
}
#[test]
fn adaptive_certificates_and_nonfinite_budget_failures() {
    let sample = sample_curve([0.0, 1.0], 0.001, 100, 2.0, |x| DVec3::new(x, x * x, 0.0)).unwrap();
    assert!(sample.certified_error <= 0.001);
    for i in 0..sample.points.len() - 1 {
        let x = (sample.parameters[i] + sample.parameters[i + 1]) / 2.0;
        let chord = (sample.points[i] + sample.points[i + 1]) / 2.0;
        assert!((chord.y - x * x).abs() <= sample.certified_error + 1e-12);
    }
    assert_eq!(
        sample,
        sample_curve([0.0, 1.0], 0.001, 100, 2.0, |x| DVec3::new(x, x * x, 0.0)).unwrap()
    );
    assert!(sample_curve([0.0, 1.0], 1e-12, 10, 2.0, |x| DVec3::new(x, x * x, 0.0)).is_err());
    assert!(sample_curve([0.0, 1.0], 0.1, 10, 2.0, |_| DVec3::splat(f64::NAN)).is_err());
    let surface = sample_surface([[0.0, 1.0], [0.0, 1.0]], 0.01, 10000, [2.0, 2.0], |x, y| {
        DVec3::new(x, y, x * x + y * y)
    })
    .unwrap();
    assert!(surface.certified_error <= 0.01);
}
#[test]
fn concave_polygon_tessellation_covers_actual_shape() {
    let points = [
        DVec3::new(0.0, 0.0, 0.0),
        DVec3::new(2.0, 0.0, 0.0),
        DVec3::new(2.0, 2.0, 0.0),
        DVec3::new(1.0, 1.0, 0.0),
        DVec3::new(0.0, 2.0, 0.0),
    ];
    let triangles = triangulate(&points).unwrap();
    assert_eq!(triangles.len(), 3);
    let area = triangles
        .iter()
        .map(|t| {
            (points[t[1] as usize] - points[t[0] as usize])
                .cross(points[t[2] as usize] - points[t[0] as usize])
                .length()
                * 0.5
        })
        .sum::<f64>();
    assert!((area - 3.0).abs() < 1e-12);
}
