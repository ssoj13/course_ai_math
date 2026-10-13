//! Gamut boundary descriptor — a direct port of `cmssm.c`.
//!
//! Implements Jan Morovic's **segment-maxima** gamut-boundary description. The Lab
//! space is centered on `a=b=0`, `L*=50` and split into a fixed `SECTORS × SECTORS`
//! grid indexed by spherical angles:
//!
//! - `r`     = C\* (chroma / radius)
//! - `alpha` = Hab (hue angle)  → grid axis `alpha`
//! - `theta` = L\* (lightness)   → grid axis `theta`
//!
//! Each cell stores the sample of greatest radius that fell in it (the *segment
//! maximum*). [`Gbd::compute`] then fills the empty cells by geometric interpolation
//! so the whole hull is described, after which [`Gbd::check_point`] answers in-gamut
//! queries.
//!
//! The C reference keeps `M_PI` (unlike `cmscam02.c`, which snapshots its own literal),
//! so we use [`std::f64::consts::PI`] here to stay bit-for-bit faithful.

use std::f64::consts::PI;

use crate::context::Context;
use crate::error::ErrorCode;
use crate::math::mat3::{MATRIX_DET_TOLERANCE, Vec3};
use crate::types::color::CIELab;

/// Number of divisions in `alpha` and `theta` (`SECTORS` in `cmssm.c`).
const SECTORS: usize = 16;

/// Spherical coordinates (`cmsSpherical`): radius plus the two grid angles in degrees.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct Spherical {
    /// Radius = C\* (chroma).
    r: f64,
    /// Hue angle Hab, degrees `0..360`.
    alpha: f64,
    /// Lightness angle, degrees `0..180`.
    theta: f64,
}

/// Occupancy of a grid cell (`GDBPointType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GdbPointType {
    /// `GP_EMPTY` — no sample seen yet.
    Empty,
    /// `GP_SPECIFIED` — set from an actual added point.
    Specified,
    /// `GP_MODELED` — filled in by interpolation during `compute`.
    Modeled,
}

/// One grid cell (`cmsGDBPoint`): its occupancy plus the retained maximum, which also
/// keeps the exact `alpha`/`theta` of that maximum (not just the sector centre).
#[derive(Debug, Clone, Copy)]
struct GdbPoint {
    /// Cell occupancy.
    type_: GdbPointType,
    /// Retained segment maximum (`r`, `alpha`, `theta`).
    p: Spherical,
}

impl Default for GdbPoint {
    fn default() -> Self {
        // Mirrors `_cmsMallocZero`: Type = GP_EMPTY (0), p = {0, 0, 0}.
        GdbPoint { type_: GdbPointType::Empty, p: Spherical::default() }
    }
}

/// Gamut boundary descriptor (`cmsGDB`).
///
/// Owns its context (for `cmsSignalError`-style diagnostics) and the `16 × 16` grid,
/// indexed `Gamut[theta][alpha]` exactly as in the C.
pub struct Gbd {
    /// Owning context (`ContextID`), used only for error signalling.
    context: Context,
    /// The segment-maxima grid, indexed `[theta][alpha]`.
    gamut: [[GdbPoint; SECTORS]; SECTORS],
}

/// A line in parametric form `P = a + t*u` (`cmsLine`).
#[derive(Debug, Clone, Copy)]
struct Line {
    /// Base point.
    a: Vec3,
    /// Direction.
    u: Vec3,
}

// --------------------------------------------------------------------------------------------

/// `_cmsAtan2` — `atan2` returning a positive angle in degrees (`0..360`).
fn atan2_deg(y: f64, x: f64) -> f64 {
    // Deal with the undefined case exactly as the C does.
    if x == 0.0 && y == 0.0 {
        return 0.0;
    }

    let mut a = (y.atan2(x) * 180.0) / PI;

    while a < 0.0 {
        a += 360.0;
    }

    a
}

/// `ToSpherical` — convert a centred Lab vector (`n = [L, a, b]`) to spherical.
fn to_spherical(v: &Vec3) -> Spherical {
    let l = v.0[0];
    let a = v.0[1];
    let b = v.0[2];

    let r = (l * l + a * a + b * b).sqrt();

    if r == 0.0 {
        return Spherical { r, alpha: 0.0, theta: 0.0 };
    }

    Spherical {
        r,
        alpha: atan2_deg(a, b),
        theta: atan2_deg((a * a + b * b).sqrt(), l),
    }
}

/// `ToCartesian` — convert spherical back to a Lab vector (`n = [L, a, b]`).
fn to_cartesian(sp: &Spherical) -> Vec3 {
    let sin_alpha = ((PI * sp.alpha) / 180.0).sin();
    let cos_alpha = ((PI * sp.alpha) / 180.0).cos();
    let sin_theta = ((PI * sp.theta) / 180.0).sin();
    let cos_theta = ((PI * sp.theta) / 180.0).cos();

    let a = sp.r * sin_theta * sin_alpha;
    let b = sp.r * sin_theta * cos_alpha;
    let l = sp.r * cos_theta;

    Vec3::new(l, a, b)
}

/// `QuantizeToSector` — grid cell of a spherical coord, saturating `360`/`180` to the
/// last sector. Returns `(alpha, theta)` as signed indices (they are range-checked by
/// the caller, matching the C `int` semantics).
fn quantize_to_sector(sp: &Spherical) -> (i32, i32) {
    let mut alpha = ((sp.alpha * SECTORS as f64) / 360.0).floor() as i32;
    let mut theta = ((sp.theta * SECTORS as f64) / 180.0).floor() as i32;

    if alpha >= SECTORS as i32 {
        alpha = SECTORS as i32 - 1;
    }
    if theta >= SECTORS as i32 {
        theta = SECTORS as i32 - 1;
    }

    (alpha, theta)
}

/// `LineOf2Points` — the parametric line through `a` toward `b`.
fn line_of_2_points(a: &Vec3, b: &Vec3) -> Line {
    Line {
        a: *a,
        u: Vec3::new(b.0[0] - a.0[0], b.0[1] - a.0[1], b.0[2] - a.0[2]),
    }
}

/// `GetPointOfLine` — evaluate the parametric line at `t`.
fn get_point_of_line(line: &Line, t: f64) -> Vec3 {
    Vec3::new(
        line.a.0[0] + t * line.u.0[0],
        line.a.0[1] + t * line.u.0[1],
        line.a.0[2] + t * line.u.0[2],
    )
}

/// `ClosestLineToLine` — closest point on segment `line1` to segment `line2`, both
/// parameterised over `0 <= t <= 1`.
///
/// softSurfer segment-to-segment algorithm (algorithm 0106). The near-parallel guard
/// (`D < MATRIX_DET_TOLERANCE`) is preserved verbatim; only the point on `line1` is
/// returned (the C keeps `tc` "for future use").
///
/// `unused_assignments`: the `tn = 0.0` / `tn = td` clamps mirror the reference exactly,
/// but since `tc` (the parameter on `line2`) is intentionally never computed here, those
/// two writes have no later reader. Kept for a faithful 1:1 port of the edge logic.
#[allow(unused_assignments)]
fn closest_line_to_line(line1: &Line, line2: &Line) -> Vec3 {
    let w0 = Vec3::minus(line1.a, line2.a);

    let a = Vec3::dot(line1.u, line1.u);
    let b = Vec3::dot(line1.u, line2.u);
    let c = Vec3::dot(line2.u, line2.u);
    let d = Vec3::dot(line1.u, w0);
    let e = Vec3::dot(line2.u, w0);

    let big_d = a * c - b * b; // Denominator
    let mut sd = big_d; // default sD = D >= 0
    let mut td = big_d;

    let mut sn;
    let mut tn;

    if big_d < MATRIX_DET_TOLERANCE {
        // The lines are almost parallel.
        sn = 0.0; // force using point P0 on segment S1
        sd = 1.0; // to prevent possible division by 0.0 later
        tn = e;
        td = c;
    } else {
        // Get the closest points on the infinite lines.
        sn = b * e - c * d;
        tn = a * e - b * d;

        if sn < 0.0 {
            // sc < 0 => the s=0 edge is visible.
            sn = 0.0;
            tn = e;
            td = c;
        } else if sn > sd {
            // sc > 1 => the s=1 edge is visible.
            sn = sd;
            tn = e + b;
            td = c;
        }
    }

    if tn < 0.0 {
        // tc < 0 => the t=0 edge is visible.
        tn = 0.0;
        // recompute sc for this edge
        if -d < 0.0 {
            sn = 0.0;
        } else if -d > a {
            sn = sd;
        } else {
            sn = -d;
            sd = a;
        }
    } else if tn > td {
        // tc > 1 => the t=1 edge is visible.
        tn = td;
        // recompute sc for this edge
        if (-d + b) < 0.0 {
            sn = 0.0;
        } else if (-d + b) > a {
            sn = sd;
        } else {
            sn = -d + b;
            sd = a;
        }
    }

    // Finally do the division to get sc.
    let sc = if sn.abs() < MATRIX_DET_TOLERANCE { 0.0 } else { sn / sd };
    // tc left unused (kept for future use in the reference).
    let _ = td;

    get_point_of_line(line1, sc)
}

// ------------------------------------------------------------------ Wrapper

/// Relative moves of the `FindNearSectors` spiral walk (`Spiral[]`).
///
/// ```text
/// 24   9  10  11  12
/// 23   8   1   2  13
/// 22   7   *   3  14
/// 21   6   5   4  15
/// 20  19  18  17  16
/// ```
///
/// Stored as `(AdvX, AdvY)` (i.e. `(alpha, theta)` deltas).
const SPIRAL: [(i32, i32); 24] = [
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (-1, -2),
    (0, -2),
    (1, -2),
    (2, -2),
    (2, -1),
    (2, 0),
    (2, 1),
    (2, 2),
    (1, 2),
    (0, 2),
    (-1, 2),
    (-2, 2),
    (-2, 1),
    (-2, 0),
    (-2, -1),
    (-2, -2),
];

/// Number of spiral steps (`NSTEPS`).
const NSTEPS: usize = SPIRAL.len();

impl Gbd {
    /// `cmsGBDAlloc` — a fresh, empty descriptor bound to `context`.
    pub fn new(context: Context) -> Self {
        Gbd {
            context,
            gamut: [[GdbPoint::default(); SECTORS]; SECTORS],
        }
    }

    // `cmsGBDFree` maps to Rust's automatic drop — the grid is owned inline, so there is
    // nothing to release explicitly.

    /// `GetPoint` — locate the grid cell holding `Lab`, returning `(theta, alpha, sp)`.
    ///
    /// The GBD is centred on `a = b = 0`, `L* = 50`; `L` is shifted by `-50` before the
    /// spherical conversion. Returns `None` (after signalling `cmsERROR_RANGE`) if the
    /// spherical value or the quantised quadrant lands out of range.
    fn get_point(&self, lab: &CIELab) -> Option<(usize, usize, Spherical)> {
        // Center L* by subtracting half of its domain, that's 50.
        let v = Vec3::new(lab.l - 50.0, lab.a, lab.b);

        // Convert to spherical coordinates.
        let sp = to_spherical(&v);

        if sp.r < 0.0 || sp.alpha < 0.0 || sp.theta < 0.0 {
            self.context.signal(ErrorCode::Range, "spherical value out of range");
            return None;
        }

        // On which sector it falls?
        let (alpha, theta) = quantize_to_sector(&sp);

        if alpha < 0 || theta < 0 || alpha >= SECTORS as i32 || theta >= SECTORS as i32 {
            self.context.signal(ErrorCode::Range, " quadrant out of range");
            return None;
        }

        Some((theta as usize, alpha as usize, sp))
    }

    /// `cmsGDBAddPoint` — add a Lab sample, keeping the segment maximum for its cell.
    ///
    /// Returns `false` if the point cannot be placed (out-of-range), matching the C
    /// `FALSE`.
    pub fn add_point(&mut self, lab: &CIELab) -> bool {
        let (theta, alpha, sp) = match self.get_point(lab) {
            Some(t) => t,
            None => return false,
        };

        let ptr = &mut self.gamut[theta][alpha];

        // Store when the sector is empty, or when this sample's radius exceeds the
        // retained maximum. The reference spells these as two identical branches
        // (`Type == GP_EMPTY` else `sp.r > p.r`); they are merged here since the stored
        // value is the same.
        if ptr.type_ == GdbPointType::Empty || sp.r > ptr.p.r {
            ptr.type_ = GdbPointType::Specified;
            ptr.p = sp;
        }

        true
    }

    /// `cmsGDBCheckPoint` — is `Lab` inside the described gamut?
    ///
    /// A point is in gamut when its cell has data and the point's radius does not exceed
    /// the cell's stored maximum. An empty cell (no data) reports out of gamut.
    pub fn check_point(&self, lab: &CIELab) -> bool {
        let (theta, alpha, sp) = match self.get_point(lab) {
            Some(t) => t,
            None => return false,
        };

        let ptr = &self.gamut[theta][alpha];

        if ptr.type_ == GdbPointType::Empty {
            return false;
        }

        // In gamut only if radius is within the stored maximum.
        sp.r <= ptr.p.r
    }

    /// `FindNearSectors` — indices of the non-empty neighbours around `(alpha, theta)`.
    ///
    /// The spiral walk wraps modulo `SECTORS` on both axes (hue and lightness are
    /// treated cyclically). Indices are returned as owned `(theta, alpha)` tuples so the
    /// caller can read neighbours and then write the target cell without borrow
    /// conflicts.
    fn find_near_sectors(&self, alpha: usize, theta: usize) -> Vec<(usize, usize)> {
        let mut close = Vec::with_capacity(NSTEPS);

        for &(adv_x, adv_y) in SPIRAL.iter() {
            let mut a = (alpha as i32 + adv_x) % SECTORS as i32;
            let mut t = (theta as i32 + adv_y) % SECTORS as i32;

            // Cycle at the begin (C `%` truncates toward zero, so negatives can appear).
            if a < 0 {
                a += SECTORS as i32;
            }
            if t < 0 {
                t += SECTORS as i32;
            }

            let (au, tu) = (a as usize, t as usize);
            if self.gamut[tu][au].type_ != GdbPointType::Empty {
                close.push((tu, au));
            }
        }

        close
    }

    /// `InterpolateMissingSector` — fill one empty cell by geometry.
    ///
    /// A ray is cast from the sector centre to the Lab origin; for every pair of nearby
    /// specified cells the closest point between that ray and the cell-to-cell edge is
    /// found, and the farthest such point that still falls inside this sector's angular
    /// bounds becomes the modelled maximum. Already-specified cells are left untouched.
    /// Always returns `true` (mirrors the C, which never fails here).
    fn interpolate_missing_sector(&mut self, alpha: usize, theta: usize) -> bool {
        // Is that point already specified?
        if self.gamut[theta][alpha].type_ != GdbPointType::Empty {
            return true;
        }

        // Fill close points (owned indices — no borrow held across the write below).
        let close = self.find_near_sectors(alpha, theta);

        // Find a central point on the sector.
        let sp = Spherical {
            alpha: (alpha as f64 + 0.5) * 360.0 / SECTORS as f64,
            theta: (theta as f64 + 0.5) * 180.0 / SECTORS as f64,
            r: 50.0,
        };

        // Convert to Cartesian.
        let lab = to_cartesian(&sp);

        // Create a ray line from centre to this point.
        let centre = Vec3::new(50.0, 0.0, 0.0);
        let ray = line_of_2_points(&lab, &centre);

        // For all close sectors, keep the farthest in-bounds intersection.
        let mut closel = Spherical { r: 0.0, alpha: 0.0, theta: 0.0 };

        for k in 0..close.len() {
            for m in (k + 1)..close.len() {
                let (tk, ak) = close[k];
                let (tm, am) = close[m];

                // A line from sector to sector.
                let a1 = to_cartesian(&self.gamut[tk][ak].p);
                let a2 = to_cartesian(&self.gamut[tm][am].p);

                let edge = line_of_2_points(&a1, &a2);

                // Find a line (closest point on the ray to this edge).
                let temp = closest_line_to_line(&ray, &edge);

                // Convert to spherical.
                let templ = to_spherical(&temp);

                if templ.r > closel.r
                    && templ.theta >= (theta as f64 * 180.0 / SECTORS as f64)
                    && templ.theta <= ((theta as f64 + 1.0) * 180.0 / SECTORS as f64)
                    && templ.alpha >= (alpha as f64 * 360.0 / SECTORS as f64)
                    && templ.alpha <= ((alpha as f64 + 1.0) * 360.0 / SECTORS as f64)
                {
                    closel = templ;
                }
            }
        }

        self.gamut[theta][alpha].p = closel;
        self.gamut[theta][alpha].type_ = GdbPointType::Modeled;

        true
    }

    /// `cmsGDBCompute` — interpolate every missing cell.
    ///
    /// The algorithm first computes the black slice (`theta = 0`) and the white slice
    /// (`theta = SECTORS-1`), then the mid sectors (`theta = 1..SECTORS`). `_flags`
    /// is accepted for parity but unused, exactly like the C `dwFlags`.
    pub fn compute(&mut self, _flags: u32) -> bool {
        // Interpolate black.
        for alpha in 0..SECTORS {
            if !self.interpolate_missing_sector(alpha, 0) {
                return false;
            }
        }

        // Interpolate white.
        for alpha in 0..SECTORS {
            if !self.interpolate_missing_sector(alpha, SECTORS - 1) {
                return false;
            }
        }

        // Interpolate mid.
        for theta in 1..SECTORS {
            for alpha in 0..SECTORS {
                if !self.interpolate_missing_sector(alpha, theta) {
                    return false;
                }
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lab(l: f64, a: f64, b: f64) -> CIELab {
        CIELab { l, a, b }
    }

    #[test]
    fn atan2_deg_is_positive_degrees() {
        assert_eq!(atan2_deg(0.0, 0.0), 0.0);
        assert!((atan2_deg(1.0, 1.0) - 45.0).abs() < 1e-9);
        // Negative angle wraps into 0..360.
        assert!((atan2_deg(-1.0, 1.0) - 315.0).abs() < 1e-9);
    }

    #[test]
    fn spherical_roundtrips_through_cartesian() {
        let v = Vec3::new(10.0, -20.0, 30.0);
        let sp = to_spherical(&v);
        let back = to_cartesian(&sp);
        for i in 0..3 {
            assert!((v.0[i] - back.0[i]).abs() < 1e-9, "component {i} mismatch");
        }
    }

    #[test]
    fn added_point_is_reported_in_gamut() {
        let mut gbd = Gbd::new(Context::new());
        let p = lab(80.0, 60.0, 60.0);
        assert!(gbd.add_point(&p));
        // Its own cell now holds it, so it must be in gamut (r <= r).
        assert!(gbd.check_point(&p));
    }

    #[test]
    fn larger_radius_replaces_smaller() {
        let mut gbd = Gbd::new(Context::new());
        // Two points in the same hue/lightness sector, different chroma.
        let small = lab(60.0, 5.0, 0.0);
        let big = lab(60.0, 40.0, 0.0);
        assert!(gbd.add_point(&small));
        assert!(gbd.add_point(&big));
        // The larger-radius point defines the boundary; the small one stays inside.
        assert!(gbd.check_point(&small));
        assert!(gbd.check_point(&big));
        // A point just beyond the big radius (same direction) is out.
        assert!(!gbd.check_point(&lab(60.0, 80.0, 0.0)));
    }

    #[test]
    fn compute_fills_and_membership_holds() {
        let mut gbd = Gbd::new(Context::new());

        // A coarse gamut shell: corners of a Lab cube plus mid samples.
        for &l in &[20.0_f64, 80.0] {
            for &a in &[-60.0_f64, 60.0] {
                for &b in &[-60.0_f64, 60.0] {
                    assert!(gbd.add_point(&lab(l, a, b)));
                }
            }
        }
        for &l in &[10.0_f64, 50.0, 90.0] {
            for &a in &[-70.0_f64, 0.0, 70.0] {
                for &b in &[-70.0_f64, 0.0, 70.0] {
                    gbd.add_point(&lab(l, a, b));
                }
            }
        }

        assert!(gbd.compute(0));

        // Interior points: the neutral axis and a modest-chroma colour.
        assert!(gbd.check_point(&lab(50.0, 0.0, 0.0)));
        assert!(gbd.check_point(&lab(50.0, 20.0, 20.0)));

        // Well outside the shell in chroma -> out of gamut.
        assert!(!gbd.check_point(&lab(50.0, 400.0, 400.0)));
        assert!(!gbd.check_point(&lab(50.0, 20.0, 300.0)));
    }
}
