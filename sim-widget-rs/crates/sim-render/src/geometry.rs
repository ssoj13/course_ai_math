//! Deterministic CPU geometry and bounded analytic sampling.
use crate::{Result, bounds::Bounds, error::invalid};
use glam::{DVec2, DVec3};
use sim_model::*;
#[derive(Clone, Debug, PartialEq)]
pub struct SegmentGeometry {
    pub a: DVec3,
    pub b: DVec3,
    pub width_css: f64,
    pub head_css: Option<f64>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CpuGeometry {
    pub positions: Vec<DVec3>,
    pub triangles: Vec<[u32; 3]>,
    pub segments: Vec<SegmentGeometry>,
    pub points: Vec<(DVec3, f64)>,
}
impl CpuGeometry {
    pub fn bounds(&self) -> Result<Bounds> {
        Bounds::from_points(
            self.positions
                .iter()
                .copied()
                .chain(self.segments.iter().flat_map(|s| [s.a, s.b]))
                .chain(self.points.iter().map(|p| p.0)),
        )
    }
}
/// Binding resolution is supplied by the immutable frame compiler.
pub fn compile(
    primitive: &ScenePrimitive,
    resolve: impl Fn(&ScenePosition) -> Result<DVec3>,
) -> Result<CpuGeometry> {
    let mut output = CpuGeometry::default();
    let mut segment = |a: DVec3, b: DVec3, width: f64, head: Option<f64>| {
        output.segments.push(SegmentGeometry {
            a,
            b,
            width_css: width,
            head_css: head,
        });
    };
    match primitive {
        ScenePrimitive::Point {
            position,
            diameter_css,
        } => output.points.push((resolve(position)?, diameter_css.get())),
        ScenePrimitive::Segment { a, b, width_css } => {
            segment(resolve(a)?, resolve(b)?, width_css.get(), None)
        }
        ScenePrimitive::Arrow {
            a,
            b,
            width_css,
            head_css,
        } => segment(
            resolve(a)?,
            resolve(b)?,
            width_css.get(),
            Some(head_css.get()),
        ),
        ScenePrimitive::Arc {
            center,
            normal,
            from,
            angle_radians,
            radius,
            width_css,
        } => {
            let center = resolve(center)?;
            let n = DVec3::from_array(normal.map(Finite::get));
            let from = DVec3::from_array(from.map(Finite::get));
            if n.length_squared() < 1e-20 {
                return Err(invalid("arc.normal", "zero direction"));
            }
            let n = n.normalize();
            let u = from - n * from.dot(n);
            if u.length_squared() < 1e-20 {
                return Err(invalid("arc.from", "direction parallel to normal"));
            }
            let u = u.normalize();
            let v = n.cross(u);
            let angle = angle_radians.get();
            if angle.abs() > std::f64::consts::TAU * 16.0 {
                return Err(invalid("arc.angle", "maximum16turns"));
            }
            let steps = ((angle.abs() * radius.get() / 0.01).ceil() as usize).clamp(1, 4096);
            let mut previous = center + u * radius.get();
            for i in 1..=steps {
                let a = angle * i as f64 / steps as f64;
                let next = center + radius.get() * (u * a.cos() + v * a.sin());
                segment(previous, next, width_css.get(), None);
                previous = next;
            }
        }
        ScenePrimitive::Polygon { positions } => {
            output.positions = positions.iter().map(resolve).collect::<Result<_>>()?;
            output.triangles = triangulate(&output.positions)?;
        }
        ScenePrimitive::Mesh {
            positions,
            triangles,
        } => {
            output.positions = positions
                .iter()
                .map(|p| DVec3::from_array(p.map(Finite::get)))
                .collect();
            output.triangles = triangles.clone();
        }
        ScenePrimitive::Trajectory {
            positions,
            width_css,
        } => {
            let mut values = positions.iter().map(resolve);
            if let Some(first) = values.next() {
                let mut previous = first?;
                for next in values {
                    let next = next?;
                    segment(previous, next, width_css.get(), None);
                    previous = next;
                }
            }
        }
        ScenePrimitive::Grid {
            extent,
            step,
            width_css,
        } => {
            let count = (extent.get() / step.get()).floor();
            if !count.is_finite() || count > 2048.0 {
                return Err(invalid("grid.step", "grid line budget exceeded"));
            }
            for i in -(count as i32)..=count as i32 {
                let x = f64::from(i) * step.get();
                segment(
                    DVec3::new(x, -extent.get(), 0.0),
                    DVec3::new(x, extent.get(), 0.0),
                    width_css.get(),
                    None,
                );
                segment(
                    DVec3::new(-extent.get(), x, 0.0),
                    DVec3::new(extent.get(), x, 0.0),
                    width_css.get(),
                    None,
                );
            }
        }
    }
    if output
        .positions
        .iter()
        .chain(output.segments.iter().flat_map(|s| [&s.a, &s.b]))
        .chain(output.points.iter().map(|p| &p.0))
        .any(|p| !p.is_finite())
    {
        return Err(invalid("geometry", "nonfinite result"));
    }
    Ok(output)
}
/// Ear clipping supports concave planar polygons. Collinear collapse keeps positions without fill.
pub fn triangulate(points: &[DVec3]) -> Result<Vec<[u32; 3]>> {
    if points.len() < 3 || points.len() > 65536 {
        return Err(invalid("polygon", "point budget"));
    }
    if points.iter().any(|p| !p.is_finite()) { return Err(invalid("polygon", "nonfinite position")); }
    let normal = (1..points.len() - 1)
        .map(|i| (points[i] - points[0]).cross(points[i + 1] - points[0]))
        .find(|n| n.length_squared() > 1e-20);
    let Some(normal) = normal else { return Ok(Vec::new()); };
    if !normal.is_finite() || !normal.length_squared().is_finite() { return Err(invalid("polygon", "normal overflow")); }
    let normal = normal.normalize();
    if points
        .iter()
        .any(|p| (*p - points[0]).dot(normal).abs() > 1e-7)
    {
        return Err(invalid("polygon", "nonplanar polygon"));
    }
    let axis = normal.abs().max_element();
    let project = |p: DVec3| {
        if axis == normal.x.abs() {
            DVec2::new(p.y, p.z)
        } else if axis == normal.y.abs() {
            DVec2::new(p.z, p.x)
        } else {
            DVec2::new(p.x, p.y)
        }
    };
    let p = points.iter().map(|p| project(*p)).collect::<Vec<_>>();
    let cross = |a: DVec2, b: DVec2| a.x * b.y - a.y * b.x;
    let area = (0..p.len())
        .map(|i| cross(p[i], p[(i + 1) % p.len()]))
        .sum::<f64>();
    if area.abs() < 1e-12 {
        return Err(invalid("polygon", "zero signed area/self intersection"));
    }
    let sign = area.signum();
    let mut remaining = (0..p.len()).collect::<Vec<_>>();
    let mut out = Vec::with_capacity(p.len() - 2);
    while remaining.len() > 3 {
        let mut found = false;
        for i in 0..remaining.len() {
            let a = remaining[(i + remaining.len() - 1) % remaining.len()];
            let b = remaining[i];
            let c = remaining[(i + 1) % remaining.len()];
            if sign * cross(p[b] - p[a], p[c] - p[b]) <= 1e-12 {
                continue;
            }
            if remaining
                .iter()
                .filter(|k| **k != a && **k != b && **k != c)
                .any(|k| {
                    sign * cross(p[b] - p[a], p[*k] - p[a]) >= -1e-12
                        && sign * cross(p[c] - p[b], p[*k] - p[b]) >= -1e-12
                        && sign * cross(p[a] - p[c], p[*k] - p[c]) >= -1e-12
                })
            {
                continue;
            }
            out.push([a as u32, b as u32, c as u32]);
            remaining.remove(i);
            found = true;
            break;
        }
        if !found {
            return Err(invalid("polygon", "self intersecting/degenerate polygon"));
        }
    }
    out.push([
        remaining[0] as u32,
        remaining[1] as u32,
        remaining[2] as u32,
    ]);
    Ok(out)
}
#[derive(Clone, Debug, PartialEq)]
pub struct CurveSamples {
    pub parameters: Vec<f64>,
    pub points: Vec<DVec3>,
    pub certified_error: f64,
}
/// Authored bound on |f''| certifies linear interpolation error M*h²/8.
/// Sampler never claims to evaluate arbitrary lesson expressions.
pub fn sample_curve(
    domain: [f64; 2],
    tolerance: f64,
    max_vertices: usize,
    second_derivative_bound: f64,
    f: impl Fn(f64) -> DVec3,
) -> Result<CurveSamples> {
    if domain.iter().any(|x| !x.is_finite())
        || domain[1] <= domain[0]
        || !tolerance.is_finite()
        || tolerance <= 0.0
        || !second_derivative_bound.is_finite()
        || second_derivative_bound < 0.0
        || max_vertices < 2
    {
        return Err(invalid("sampling", "invalid domain/error/budget"));
    }
    let h = domain[1] - domain[0];
    let required = (h * (second_derivative_bound / (8.0 * tolerance)).sqrt())
        .ceil()
        .max(1.0);
    if !required.is_finite() || required >= max_vertices as f64 {
        return Err(invalid("sampling", "tolerance exceeds vertex budget"));
    }
    let count = required as usize;
    let parameters = (0..=count)
        .map(|i| domain[0] + h * i as f64 / count as f64)
        .collect::<Vec<_>>();
    let points = parameters.iter().map(|t| f(*t)).collect::<Vec<_>>();
    if points.iter().any(|p| !p.is_finite()) {
        return Err(invalid("sampling", "nonfinite function sample"));
    }
    Ok(CurveSamples {
        parameters,
        points,
        certified_error: second_derivative_bound * (h / count as f64).powi(2) / 8.0,
    })
}
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceSamples {
    pub geometry: CpuGeometry,
    pub certified_error: f64,
}
/// Error bounded tensor-grid refinement; row-major vertices and fixed diagonal.
pub fn sample_surface(
    domain: [[f64; 2]; 2],
    tolerance: f64,
    max_vertices: usize,
    second_derivative_bounds: [f64; 2],
    f: impl Fn(f64, f64) -> DVec3,
) -> Result<SurfaceSamples> {
    if domain.iter().flatten().any(|n| !n.is_finite())
        || domain.iter().any(|d| d[1] <= d[0])
        || !tolerance.is_finite()
        || tolerance <= 0.0
        || second_derivative_bounds
            .iter()
            .any(|v| !v.is_finite() || *v < 0.0)
    {
        return Err(invalid("surface sampling", "invalid domain/error"));
    }
    let h = [domain[0][1] - domain[0][0], domain[1][1] - domain[1][0]];
    let nx = (h[0] * (second_derivative_bounds[0] / (4.0 * tolerance)).sqrt())
        .ceil()
        .max(1.0);
    let ny = (h[1] * (second_derivative_bounds[1] / (4.0 * tolerance)).sqrt())
        .ceil()
        .max(1.0);
    if !nx.is_finite() || !ny.is_finite() || (nx + 1.0) * (ny + 1.0) > max_vertices as f64 {
        return Err(invalid(
            "surface sampling",
            "tolerance exceeds vertex budget",
        ));
    }
    let (nx, ny) = (nx as usize, ny as usize);
    let mut geometry = CpuGeometry::default();
    for j in 0..=ny {
        for i in 0..=nx {
            let p = f(
                domain[0][0] + h[0] * i as f64 / nx as f64,
                domain[1][0] + h[1] * j as f64 / ny as f64,
            );
            if !p.is_finite() {
                return Err(invalid("surface sampling", "nonfinite sample"));
            }
            geometry.positions.push(p);
        }
    }
    for j in 0..ny {
        for i in 0..nx {
            let a = (j * (nx + 1) + i) as u32;
            let b = a + 1;
            let c = a + (nx + 1) as u32;
            geometry.triangles.extend([[a, b, c], [b, c + 1, c]]);
        }
    }
    Ok(SurfaceSamples {
        geometry,
        certified_error: (second_derivative_bounds[0] * (h[0] / nx as f64).powi(2)
            + second_derivative_bounds[1] * (h[1] / ny as f64).powi(2))
            / 8.0,
    })
}
