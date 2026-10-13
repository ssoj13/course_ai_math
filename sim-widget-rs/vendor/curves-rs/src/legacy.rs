//! Loading of old scenes: one [`CurveKind`] per key (the interpolation of the segment that starts at
//! the key) plus Bezier/Hermite [`Tangent`] handles, mapped onto per-side [`Tan`]s.
//!
//! Only the reader lives here; evaluation is always [`Track`]. Mapping per segment `i` (left key `i`,
//! kind `K`), see `docs/KEYS.md` section 5:
//!
//! | `K` | out of key `i` / in of key `i+1` | vs the old evaluator |
//! |---|---|---|
//! | Linear | Linear / Linear | identical |
//! | Smooth (smoothstep) | Flat / Flat | identical |
//! | Step | Constant / (free: the key's out side, Smooth after a Step) | identical |
//! | Hermite | User slope `dy/dx`, default width | identical |
//! | Bezier | User slope `dy/dx` + weight `|dx|` | identical, except handles pointing the wrong way (weight 0) |
//! | CatmullRom | CatmullRom / CatmullRom | semantic: real key times, one-sided ends |
//! | Monotone | Smooth / Smooth | semantic: time-weighted slope, same no-overshoot limit |

use crate::{Key, KeysError, Real, Side, Tan, Track};

/// The interpolation of the segment that starts at a key, in the old model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CurveKind {
    /// Straight line.
    Linear,
    /// Smoothstep.
    Smooth,
    /// Hold the left value.
    Step,
    /// Catmull-Rom through the keys.
    CatmullRom,
    /// Cubic Hermite from handle slopes.
    Hermite,
    /// Cubic Bezier from handle offsets.
    Bezier,
    /// Monotone cubic (Fritsch-Carlson).
    Monotone,
}

/// An old tangent handle: offset `(dx, dy)` from its key.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Tangent<T: Real = f64> {
    /// Horizontal offset (time).
    pub dx: T,
    /// Vertical offset (value).
    pub dy: T,
}

/// An old key. Hosts map their own field names onto this (serde `alias`, or a host-side shim).
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LegacyKey<T: Real = f64> {
    /// Time.
    pub t: T,
    /// Value.
    pub v: T,
    /// Interpolation of the segment starting at this key.
    pub interp: CurveKind,
    /// Incoming handle.
    #[cfg_attr(feature = "serde", serde(default))]
    pub tan_in: Tangent<T>,
    /// Outgoing handle.
    #[cfg_attr(feature = "serde", serde(default))]
    pub tan_out: Tangent<T>,
}

/// Slope `dy/dx` of an old handle; 0 for a (near) zero `dx`, as the old evaluator did.
fn hslope<T: Real>(h: Tangent<T>) -> T {
    if h.dx.abs() <= T::EPSILON {
        T::ZERO
    } else {
        h.dy / h.dx
    }
}

/// The two sides a legacy segment `a -> b` of span `s` asks for; `None` = follows the key.
fn sides<T: Real>(a: &LegacyKey<T>, b: &LegacyKey<T>, s: T) -> (Side<T>, Option<Side<T>>) {
    let pair = |k| (Side::new(k), Some(Side::new(k)));
    let bez = |h: Tangent<T>, sign: T| {
        // The old evaluator clamped the control x into the segment; a wrong-way handle collapsed to 0.
        let w = (h.dx * sign).clamp(T::ZERO, s);
        // `dy` is kept whole (the old code clamped only the control x), so the slope uses the clamped width.
        Side::user(
            if w > T::EPSILON {
                h.dy * sign / w
            } else {
                T::ZERO
            },
            Some(w),
        )
    };
    match a.interp {
        CurveKind::Linear => pair(Tan::Linear),
        CurveKind::Smooth => pair(Tan::Flat),
        CurveKind::Step => (Side::new(Tan::Constant), None),
        CurveKind::CatmullRom => pair(Tan::CatmullRom),
        CurveKind::Monotone => pair(Tan::Smooth),
        CurveKind::Hermite => (
            Side::user(hslope(a.tan_out), None),
            Some(Side::user(hslope(b.tan_in), None)),
        ),
        CurveKind::Bezier => (bez(a.tan_out, T::ONE), Some(bez(b.tan_in, -T::ONE))),
    }
}

impl<T: Real> Track<T> {
    /// A track from old keys (strictly increasing times), with constant extrapolation as the old
    /// evaluators had.
    pub fn from_legacy(old: impl IntoIterator<Item = LegacyKey<T>>) -> Result<Self, KeysError> {
        let old: Vec<LegacyKey<T>> = old.into_iter().collect();
        let n = old.len();
        let mut outs: Vec<Side<T>> = vec![Side::default(); n];
        let mut ins: Vec<Option<Side<T>>> = vec![None; n];
        for i in 0..n.saturating_sub(1) {
            let s = old[i + 1].t - old[i].t;
            let (o, inn) = sides(&old[i], &old[i + 1], s);
            outs[i] = o;
            ins[i + 1] = inn;
        }
        if n > 0 {
            outs[n - 1] = ins[n - 1].unwrap_or(outs[n - 1]);
        }
        let keys = old
            .iter()
            .enumerate()
            .map(|(i, o)| {
                let mut k = Key::new(o.t, o.v);
                k.out = outs[i];
                // A hold segment leaves the right key's in side free: it follows the key's out side,
                // except a hold (Constant never couples), which would turn the previous segment into a step.
                k.inn = ins[i].unwrap_or(if k.out.kind == Tan::Constant {
                    Side::default()
                } else {
                    k.out
                });
                k.broken = !k.coupled();
                k
            })
            .collect();
        Self::from_keys(keys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SideId;

    fn lcg(seed: &mut u64) -> f64 {
        *seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (*seed >> 11) as f64 / (1u64 << 53) as f64
    }

    /// The old `eval_segment` semantics for the five exactly-mapped kinds (exact Bezier solve).
    fn oracle(a: &LegacyKey<f64>, b: &LegacyKey<f64>, x: f64) -> f64 {
        let span = b.t - a.t;
        let t = ((x - a.t) / span).clamp(0.0, 1.0);
        let herm = |m0: f64, m1: f64| {
            let (t2, t3) = (t * t, t * t * t);
            (2.0 * t3 - 3.0 * t2 + 1.0) * a.v
                + (t3 - 2.0 * t2 + t) * m0 * span
                + (-2.0 * t3 + 3.0 * t2) * b.v
                + (t3 - t2) * m1 * span
        };
        match a.interp {
            CurveKind::Linear => a.v + (b.v - a.v) * t,
            CurveKind::Smooth => a.v + (b.v - a.v) * t * t * (3.0 - 2.0 * t),
            CurveKind::Step => a.v,
            CurveKind::Hermite => herm(hslope(a.tan_out), hslope(b.tan_in)),
            CurveKind::Bezier => {
                let x1 = (a.t + a.tan_out.dx).clamp(a.t, b.t);
                let x2 = (b.t + b.tan_in.dx).clamp(a.t, b.t);
                let (y1, y2) = (a.v + a.tan_out.dy, b.v + b.tan_in.dy);
                let bz = |p: [f64; 4], u: f64| {
                    let v = 1.0 - u;
                    v * v * v * p[0]
                        + 3.0 * v * v * u * p[1]
                        + 3.0 * v * u * u * p[2]
                        + u * u * u * p[3]
                };
                let (mut lo, mut hi) = (0.0, 1.0);
                for _ in 0..80 {
                    let u = 0.5 * (lo + hi);
                    if bz([a.t, x1, x2, b.t], u) < x {
                        lo = u;
                    } else {
                        hi = u;
                    }
                }
                bz([a.v, y1, y2, b.v], 0.5 * (lo + hi))
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn exact_kinds_match_old_evaluator() {
        const KINDS: [CurveKind; 5] = [
            CurveKind::Linear,
            CurveKind::Smooth,
            CurveKind::Step,
            CurveKind::Hermite,
            CurveKind::Bezier,
        ];
        let mut seed = 99;
        for _ in 0..200 {
            let mut t = 0.0;
            let old: Vec<LegacyKey<f64>> = (0..5)
                .map(|_| {
                    t += 0.5 + lcg(&mut seed) * 2.0;
                    let r = |s: &mut u64| lcg(s) * 2.0 - 1.0;
                    LegacyKey {
                        t,
                        v: r(&mut seed) * 5.0,
                        interp: KINDS[(lcg(&mut seed) * 5.0) as usize % 5],
                        // dx has the right sign for Bezier; for Hermite only the slope matters.
                        tan_in: Tangent {
                            dx: -(0.05 + lcg(&mut seed) * 0.9),
                            dy: r(&mut seed),
                        },
                        tan_out: Tangent {
                            dx: 0.05 + lcg(&mut seed) * 0.9,
                            dy: r(&mut seed),
                        },
                    }
                })
                .collect();
            let tr = Track::from_legacy(old.clone()).unwrap();
            for i in 0..old.len() - 1 {
                for s in 0..=20 {
                    // Stop just short of the right key: the jump of a hold belongs to the next segment.
                    let x = old[i].t + (old[i + 1].t - old[i].t) * (f64::from(s) / 20.0 * 0.999);
                    let (got, want) = (tr.eval(x), oracle(&old[i], &old[i + 1], x));
                    assert!(
                        (got - want).abs() < 1e-6,
                        "seg {i} {:?} x={x}: {got} vs {want}",
                        old[i].interp
                    );
                }
            }
        }
    }

    #[test]
    fn wrong_way_bezier_handle_collapses_to_flat() {
        let k = |t, v, dx_out| LegacyKey {
            t,
            v,
            interp: CurveKind::Bezier,
            tan_in: Tangent { dx: -0.3, dy: 0.0 },
            tan_out: Tangent {
                dx: dx_out,
                dy: 2.0,
            },
        };
        let tr = Track::from_legacy([k(0.0, 0.0, -0.5), k(1.0, 1.0, 0.0)]).unwrap();
        assert_eq!(tr.handle(0, SideId::Out), Some((0.0, 0.0)));
    }

    #[test]
    fn monotone_and_catmull_semantic_kinds() {
        let mk = |kind| {
            [(0.0, 0.0), (1.0, 1.0), (2.0, 1.0), (3.0, 4.0), (4.0, 5.0)].map(|(t, v)| LegacyKey {
                t,
                v,
                interp: kind,
                tan_in: Tangent::default(),
                tan_out: Tangent::default(),
            })
        };
        let m = Track::from_legacy(mk(CurveKind::Monotone)).unwrap();
        for s in 0..=400 {
            let x = f64::from(s) / 100.0;
            let v = m.eval(x);
            assert!((-1e-12..=5.0 + 1e-12).contains(&v));
            assert!(
                m.eval(x + 0.01) >= v - 1e-12,
                "monotone data stays monotone"
            );
        }
        // Interior uniform spacing: Catmull-Rom equals the classic uniform formula.
        let c = Track::from_legacy(mk(CurveKind::CatmullRom)).unwrap();
        let (p0, p1, p2, p3, t) = (1.0, 1.0, 4.0, 5.0, 0.5);
        let want = 0.5
            * (2.0 * p1
                + (p2 - p0) * t
                + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t
                + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t * t * t);
        assert!((c.eval(2.5) - want).abs() < 1e-12);
    }

    #[test]
    fn step_then_smooth_key_is_not_broken_needlessly() {
        let k = |t, v, interp| LegacyKey {
            t,
            v,
            interp,
            tan_in: Tangent::default(),
            tan_out: Tangent::default(),
        };
        let tr = Track::from_legacy([
            k(0.0, 0.0, CurveKind::Step),
            k(1.0, 1.0, CurveKind::Monotone),
            k(2.0, 2.0, CurveKind::Linear),
        ])
        .unwrap();
        assert!(!tr.keys()[1].broken);
        assert_eq!(tr.eval(0.5), 0.0);
        assert_eq!(tr.pre(), crate::Extrap::Constant);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn legacy_key_json_loads() {
        let j = r#"[{"t":0.0,"v":1.0,"interp":"Linear"},{"t":2.0,"v":3.0,"interp":"Step","tan_out":{"dx":0.1,"dy":0.0}}]"#;
        let old: Vec<LegacyKey<f64>> = serde_json::from_str(j).unwrap();
        assert_eq!(Track::from_legacy(old).unwrap().eval(1.0), 2.0);
    }
}
