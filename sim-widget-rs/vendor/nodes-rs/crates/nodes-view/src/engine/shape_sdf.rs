//! CPU SDF for shapes (SSOT — WGSL mirrors these formulas).

use super::shape2d::Shape2d;

const TAU: f32 = std::f32::consts::TAU;

impl Shape2d {
    /// Axis-aligned bounds in graph UV: `[min_x, min_y, max_x, max_y]`.
    pub fn aabb(&self) -> [f32; 4] {
        self.aabb_with_stroke(0.0)
    }

    /// Bounds expanded so stroke (half width) is not clipped by the instance quad.
    pub fn aabb_with_stroke(&self, stroke_width: f32) -> [f32; 4] {
        let pad = stroke_width.max(0.0) * 0.5 + 1.0;
        match self {
            Shape2d::RoundedRect { min, max, radius } => {
                let r = radius.max(0.0);
                [
                    min[0] - r - pad,
                    min[1] - r - pad,
                    max[0] + r + pad,
                    max[1] + r + pad,
                ]
            }
            Shape2d::Circle { center, radius } => {
                let r = radius.max(0.0);
                [
                    center[0] - r - pad,
                    center[1] - r - pad,
                    center[0] + r + pad,
                    center[1] + r + pad,
                ]
            }
            Shape2d::RegularPolygon {
                center,
                radius,
                sides,
                ..
            } => {
                let r = radius.max(0.0);
                let _ = sides;
                [
                    center[0] - r - pad,
                    center[1] - r - pad,
                    center[0] + r + pad,
                    center[1] + r + pad,
                ]
            }
            Shape2d::Segment { a, b, half_width } => {
                let hw = half_width.max(0.5);
                let min_x = a[0].min(b[0]) - hw - pad;
                let min_y = a[1].min(b[1]) - hw - pad;
                let max_x = a[0].max(b[0]) + hw + pad;
                let max_y = a[1].max(b[1]) + hw + pad;
                [min_x, min_y, max_x, max_y]
            }
        }
    }

    /// Signed distance: negative inside, positive outside.
    pub fn signed_distance(&self, p: [f32; 2]) -> f32 {
        match self {
            Shape2d::RoundedRect { min, max, radius } => {
                sd_rounded_box(p, *min, *max, radius.max(0.0))
            }
            Shape2d::Circle { center, radius } => sd_circle(p, *center, radius.max(0.0)),
            Shape2d::RegularPolygon {
                center,
                radius,
                sides,
                rotation,
            } => sd_regular_polygon(p, *center, radius.max(0.0), *sides, *rotation),
            Shape2d::Segment { a, b, half_width } => sd_segment(p, *a, *b) - half_width.max(0.5),
        }
    }

    pub fn contains_point(&self, p: [f32; 2], padding: f32) -> bool {
        self.signed_distance(p) <= padding
    }

    /// Hit only the border band (hollow interior) of a rounded rect.
    pub fn contains_point_border(&self, p: [f32; 2], padding: f32, border: f32) -> bool {
        let border = border.max(2.0);
        match self {
            Shape2d::RoundedRect { min, max, radius } => {
                if !self.contains_point(p, padding) {
                    return false;
                }
                let inner = Shape2d::RoundedRect {
                    min: [min[0] + border, min[1] + border],
                    max: [max[0] - border, max[1] - border],
                    radius: (*radius - border).max(0.0),
                };
                !inner.contains_point(p, -padding)
            }
            _ => self.contains_point(p, padding),
        }
    }
}

fn sd_circle(p: [f32; 2], center: [f32; 2], r: f32) -> f32 {
    let dx = p[0] - center[0];
    let dy = p[1] - center[1];
    dx.hypot(dy) - r
}

fn sd_rounded_box(p: [f32; 2], min: [f32; 2], max: [f32; 2], r: f32) -> f32 {
    let cx = 0.5 * (min[0] + max[0]);
    let cy = 0.5 * (min[1] + max[1]);
    let hx = 0.5 * (max[0] - min[0]);
    let hy = 0.5 * (max[1] - min[1]);
    let qx = (p[0] - cx).abs() - hx + r;
    let qy = (p[1] - cy).abs() - hy + r;
    let ax = qx.max(0.0);
    let ay = qy.max(0.0);
    ax.hypot(ay) + qx.min(0.0).max(qy.min(0.0)) - r
}

fn sd_regular_polygon(p: [f32; 2], center: [f32; 2], r: f32, sides: u8, rotation: f32) -> f32 {
    let n = sides.max(3) as f32;
    let an = TAU / n;
    let mut d = sd_circle(p, center, r);
    let rel = [p[0] - center[0], p[1] - center[1]];
    let ang = rel[1].atan2(rel[0]) - rotation;
    let sector = (ang / an).floor();
    let a0 = rotation + sector * an;
    let a1 = a0 + an;
    let v0 = [a0.cos(), a0.sin()];
    let v1 = [a1.cos(), a1.sin()];
    let d_edge = sd_segment(rel, [0.0, 0.0], [v0[0] * r, v0[1] * r]).min(sd_segment(
        rel,
        [0.0, 0.0],
        [v1[0] * r, v1[1] * r],
    ));
    d = d.min(d_edge);
    d
}

fn sd_segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let pa = [p[0] - a[0], p[1] - a[1]];
    let ba = [b[0] - a[0], b[1] - a[1]];
    let h =
        ((pa[0] * ba[0] + pa[1] * ba[1]) / (ba[0] * ba[0] + ba[1] * ba[1] + 1e-8)).clamp(0.0, 1.0);
    let dx = pa[0] - ba[0] * h;
    let dy = pa[1] - ba[1] * h;
    dx.hypot(dy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_inside_outside() {
        let c = Shape2d::Circle {
            center: [0.0, 0.0],
            radius: 10.0,
        };
        assert!(c.signed_distance([0.0, 0.0]) < 0.0);
        assert!(c.signed_distance([10.0, 0.0]) >= 0.0);
    }

    #[test]
    fn rounded_rect_border_hollow_interior() {
        let outer = Shape2d::RoundedRect {
            min: [0.0, 0.0],
            max: [100.0, 80.0],
            radius: 4.0,
        };
        assert!(outer.contains_point_border([2.0, 40.0], 0.0, 10.0));
        assert!(!outer.contains_point_border([50.0, 40.0], 0.0, 10.0));
    }

    #[test]
    fn rounded_rect_center_inside() {
        let r = Shape2d::RoundedRect {
            min: [-50.0, -30.0],
            max: [50.0, 30.0],
            radius: 4.0,
        };
        assert!(r.contains_point([0.0, 0.0], 0.0));
    }

    #[test]
    fn segment_capsule_inside() {
        let seg = Shape2d::Segment {
            a: [0.0, 0.0],
            b: [100.0, 0.0],
            half_width: 4.0,
        };
        assert!(seg.signed_distance([50.0, 0.0]) < 0.0);
        assert!(seg.signed_distance([50.0, 10.0]) > 0.0);
    }
}
