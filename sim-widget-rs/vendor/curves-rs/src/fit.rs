//! Key reduction: Bezier fitting of baked samples with a hard error bound (Schneider,
//! "An Algorithm for Automatically Fitting Digitized Curves", Graphics Gems I, adapted to a
//! function `v(t)`). See `docs/KEYS.md` section 4.

use crate::keys::Seg;
use crate::{Key, KeysError, Real, Side, Tan, Track};

/// Result summary of a fit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FitStats {
    /// Keys in the fitted track.
    pub keys: usize,
    /// Largest `|eval(t_i) - v_i|` over all samples, measured on the finished track.
    pub max_err: f64,
}

/// One accepted piece: samples `[i, j]` joined by a straight segment or a fitted Bezier.
#[derive(Debug, Clone, Copy)]
enum Piece {
    Linear,
    /// Handle widths in time units.
    Curve(f64, f64),
}

/// Reusable fitter; owns its scratch buffers so steady-state fits do not allocate.
#[derive(Debug, Default)]
pub struct Fitter {
    slopes: Vec<f64>,
    u: Vec<f64>,
    stack: Vec<(usize, usize)>,
    pieces: Vec<(usize, usize, Piece)>,
}

impl Fitter {
    /// A fitter with empty scratch.
    pub fn new() -> Self {
        Self::default()
    }

    /// Fits `samples` (`(t, v)`, strictly increasing `t`) into `out`, keeping its extrapolation.
    /// Every sample ends within `tol` of the track.
    pub fn fit<T: Real>(
        &mut self,
        samples: &[(T, T)],
        tol: T,
        out: &mut Track<T>,
    ) -> Result<FitStats, KeysError> {
        self.fit_with(samples.len(), |k| samples[k], tol, out)
    }

    /// Like [`Fitter::fit`] for samples at `t0 + k*dt` (a per-frame bake).
    pub fn fit_uniform<T: Real>(
        &mut self,
        t0: T,
        dt: T,
        values: &[T],
        tol: T,
        out: &mut Track<T>,
    ) -> Result<FitStats, KeysError> {
        // `t0 + k*dt` computed per index so the times are exact multiples, not accumulated sums.
        let at = |k: usize| (t0 + dt * T::of(k as f64), values[k]);
        self.fit_with(values.len(), at, tol, out)
    }

    fn fit_with<T: Real>(
        &mut self,
        n: usize,
        at: impl Fn(usize) -> (T, T),
        tol: T,
        out: &mut Track<T>,
    ) -> Result<FitStats, KeysError> {
        if !(tol.is_finite() && tol >= T::ZERO) {
            return Err(KeysError::Scale);
        }
        for k in 0..n {
            let (t, v) = at(k);
            if !t.is_finite() || !v.is_finite() {
                return Err(KeysError::NotFinite);
            }
            if k > 0 && t <= at(k - 1).0 {
                return Err(KeysError::Order(k));
            }
        }
        if n >= 3 {
            self.slopes.clear();
            self.slopes.extend((0..n).map(|k| {
                let (a, b) = (k.saturating_sub(1), (k + 1).min(n - 1));
                (at(b).1 - at(a).1).to_f64() / (at(b).0 - at(a).0).to_f64()
            }));
        }
        let (pre, post) = (out.pre(), out.post());
        // The bound is checked on the finished track, not on the pieces: if a rounding difference
        // pushes a sample over `tol`, refit with a tighter inner tolerance. Zero always succeeds
        // (every sample becomes a key, and a key evaluates to its own value exactly).
        let mut max_err = 0.0;
        for shrink in [1.0, 0.5, 0.0] {
            let mut keys = out.take_keys();
            if n < 3 {
                keys.extend((0..n).map(|k| Key::new(at(k).0, at(k).1).with_tan(Tan::Linear)));
            } else {
                self.split(n, &at, tol * T::of(shrink));
                self.keys(n, &at, &mut keys);
            }
            out.set_keys(keys)?;
            let mut hint = 0;
            max_err = (0..n)
                .map(|k| {
                    let (t, v) = at(k);
                    (out.eval_hint(t, &mut hint) - v).abs().to_f64()
                })
                .fold(0.0, f64::max);
            if max_err <= tol.to_f64() {
                break;
            }
        }
        out.set_extrap(pre, post);
        Ok(FitStats {
            keys: out.len(),
            max_err,
        })
    }

    /// Recursive subdivision (explicit stack): fills `pieces`, sorted by start sample.
    fn split<T: Real>(&mut self, n: usize, at: &impl Fn(usize) -> (T, T), tol: T) {
        self.pieces.clear();
        self.stack.clear();
        self.stack.push((0, n - 1));
        while let Some((i, j)) = self.stack.pop() {
            if j == i + 1 {
                self.pieces.push((i, j, Piece::Linear));
                continue;
            }
            let (lin_err, _) = line_err(i, j, at);
            if lin_err <= tol.to_f64() {
                self.pieces.push((i, j, Piece::Linear));
                continue;
            }
            let (err, worst, w0, w1) = self.fit_curve(i, j, at, tol.to_f64());
            if err <= tol.to_f64() {
                self.pieces.push((i, j, Piece::Curve(w0, w1)));
            } else {
                self.stack.push((worst, j));
                self.stack.push((i, worst));
            }
        }
        self.pieces.sort_unstable_by_key(|p| p.0);
    }

    /// Least-squares handle widths for samples `[i, j]` with the end slopes fixed, refined by Newton
    /// reparameterization. Returns `(max error, worst interior sample, w0, w1)` of the best attempt;
    /// the error is measured with the real evaluator.
    fn fit_curve<T: Real>(
        &mut self,
        i: usize,
        j: usize,
        at: &impl Fn(usize) -> (T, T),
        tol: f64,
    ) -> (f64, usize, f64, f64) {
        let ((ti, vi), (tj, vj)) = (at(i), at(j));
        let (tspan, vspan) = ((tj - ti).to_f64(), (vj - vi).to_f64());
        // Normalize both axes to the unit square so the least squares weighs them equally.
        let r = (i..=j)
            .map(|k| (at(k).1 - vi).to_f64().abs())
            .fold(0.0, f64::max)
            .max(1e-300);
        let pt = |k: usize| {
            let (t, v) = at(k);
            ((t - ti).to_f64() / tspan, (v - vi).to_f64() / r)
        };
        let (m0, m1) = (self.slopes[i] * tspan / r, self.slopes[j] * tspan / r);
        let p3 = (1.0, vspan / r);
        self.u.clear();
        self.u.extend((i..=j).map(|k| pt(k).0));
        let mut best = (f64::INFINITY, i + 1, tspan / 3.0, tspan / 3.0);
        for _ in 0..5 {
            let (a1, a2) = solve_alpha(&self.u, i, &pt, m0, m1, p3);
            let (w0, w1) = (a1 * tspan, a2 * tspan);
            let seg = Seg::curve(
                ti,
                tj,
                vi,
                vj,
                (T::of(self.slopes[i]), T::of(w0)),
                (T::of(self.slopes[j]), T::of(w1)),
            );
            let (mut err, mut worst) = (0.0f64, i + 1);
            for k in i + 1..j {
                let (t, v) = at(k);
                let e = (seg.eval(t) - v).to_f64().abs();
                if e > err {
                    (err, worst) = (e, k);
                }
            }
            if err < best.0 {
                best = (err, worst, w0, w1);
            }
            if err <= tol {
                break;
            }
            reparam(
                &mut self.u,
                i,
                &pt,
                [(0.0, 0.0), (a1, a1 * m0), (1.0 - a2, p3.1 - a2 * m1), p3],
            );
        }
        best
    }

    /// Builds the keys from the accepted pieces into `keys`. Fitted pieces always write their measured
    /// handle width, so the finished track evaluates exactly what the fit measured.
    fn keys<T: Real>(&self, n: usize, at: &impl Fn(usize) -> (T, T), keys: &mut Vec<Key<T>>) {
        let mut push = |idx: usize, inn: Option<Side<T>>, out: Option<Side<T>>| {
            let (t, v) = at(idx);
            let mut k = Key::new(t, v);
            k.inn = inn.or(out).unwrap_or_default();
            k.out = out.or(inn).unwrap_or_default();
            k.broken = !k.coupled();
            keys.push(k);
        };
        let mut prev_in: Option<Side<T>> = None;
        for &(i, j, piece) in &self.pieces {
            let (out, inn) = match piece {
                Piece::Linear => (Side::new(Tan::Linear), Side::new(Tan::Linear)),
                Piece::Curve(w0, w1) => (
                    Side::user(T::of(self.slopes[i]), Some(T::of(w0))),
                    Side::user(T::of(self.slopes[j]), Some(T::of(w1))),
                ),
            };
            push(i, prev_in, Some(out));
            prev_in = Some(inn);
        }
        push(n - 1, prev_in, None);
    }
}

/// Max vertical distance of samples `[i, j]` from the chord, and its sample.
fn line_err<T: Real>(i: usize, j: usize, at: &impl Fn(usize) -> (T, T)) -> (f64, usize) {
    let ((ti, vi), (tj, vj)) = (at(i), at(j));
    let mut worst = (0.0f64, i + 1);
    for k in i + 1..j {
        let (t, v) = at(k);
        let line = vi + (vj - vi) * ((t - ti) / (tj - ti));
        let e = (line - v).to_f64().abs();
        if e > worst.0 {
            worst = (e, k);
        }
    }
    worst
}

/// Cubic Bernstein basis at `u`.
fn basis(u: f64) -> [f64; 4] {
    let v = 1.0 - u;
    [v * v * v, 3.0 * v * v * u, 3.0 * v * u * u, u * u * u]
}

/// Schneider's 2x2 least squares for the two handle widths (normalized time) with fixed
/// directions `(1, m0)` and `(-1, -m1)`. Falls back to a third of the span when ill-conditioned.
fn solve_alpha(
    u: &[f64],
    i: usize,
    pt: &impl Fn(usize) -> (f64, f64),
    m0: f64,
    m1: f64,
    p3: (f64, f64),
) -> (f64, f64) {
    let (t1, t2) = ((1.0, m0), (-1.0, -m1));
    let dot = |a: (f64, f64), b: (f64, f64)| a.0 * b.0 + a.1 * b.1;
    let (mut c11, mut c12, mut c22, mut x1, mut x2) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (n, &uu) in u.iter().enumerate() {
        let b = basis(uu);
        let (a1, a2) = ((t1.0 * b[1], t1.1 * b[1]), (t2.0 * b[2], t2.1 * b[2]));
        let p = pt(i + n);
        // Sample minus the part of the curve the end points already explain.
        let s = (p.0 - p3.0 * (b[2] + b[3]), p.1 - p3.1 * (b[2] + b[3]));
        c11 += dot(a1, a1);
        c12 += dot(a1, a2);
        c22 += dot(a2, a2);
        x1 += dot(a1, s);
        x2 += dot(a2, s);
    }
    let det = c11 * c22 - c12 * c12;
    let third = (1.0 / 3.0, 1.0 / 3.0);
    if det.abs() < 1e-12 * (c11 * c22).abs().max(1e-300) {
        return third;
    }
    let (a1, a2) = ((x1 * c22 - x2 * c12) / det, (c11 * x2 - c12 * x1) / det);
    if a1.is_finite() && a2.is_finite() {
        (a1.clamp(0.0, 1.0), a2.clamp(0.0, 1.0))
    } else {
        third
    }
}

/// One Newton-Raphson pass moving each sample's parameter toward its closest curve point.
fn reparam(u: &mut [f64], i: usize, pt: &impl Fn(usize) -> (f64, f64), p: [(f64, f64); 4]) {
    for (n, uu) in u.iter_mut().enumerate() {
        let b = basis(*uu);
        let q = (
            (0..4).map(|k| p[k].0 * b[k]).sum::<f64>(),
            (0..4).map(|k| p[k].1 * b[k]).sum::<f64>(),
        );
        let v = 1.0 - *uu;
        let (w1, w2) = ([v * v, 2.0 * v * *uu, *uu * *uu], [v, *uu]);
        let d = |k: usize| (p[k + 1].0 - p[k].0, p[k + 1].1 - p[k].1);
        let q1 = (
            3.0 * (0..3).map(|k| d(k).0 * w1[k]).sum::<f64>(),
            3.0 * (0..3).map(|k| d(k).1 * w1[k]).sum::<f64>(),
        );
        let dd = |k: usize| {
            let (a, b) = (d(k), d(k + 1));
            (b.0 - a.0, b.1 - a.1)
        };
        let q2 = (
            6.0 * (0..2).map(|k| dd(k).0 * w2[k]).sum::<f64>(),
            6.0 * (0..2).map(|k| dd(k).1 * w2[k]).sum::<f64>(),
        );
        let target = pt(i + n);
        let diff = (q.0 - target.0, q.1 - target.1);
        let num = diff.0 * q1.0 + diff.1 * q1.1;
        let den = q1.0 * q1.0 + q1.1 * q1.1 + diff.0 * q2.0 + diff.1 * q2.1;
        if den.abs() > 1e-12 {
            *uu = (*uu - num / den).clamp(0.0, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lcg(seed: &mut u64) -> f64 {
        *seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (*seed >> 11) as f64 / (1u64 << 53) as f64
    }

    #[test]
    fn sine_within_tolerance_and_reduced() {
        let vals: Vec<f64> = (0..300).map(|k| (k as f64 * 0.05).sin() * 10.0).collect();
        let mut tr = Track::new();
        let st = Fitter::new()
            .fit_uniform(0.0, 1.0, &vals, 0.05, &mut tr)
            .unwrap();
        assert!(st.max_err <= 0.05 + 1e-9, "err {}", st.max_err);
        assert!(st.keys < 60, "keys {}", st.keys);
    }

    #[test]
    fn noisy_walk_error_bound_holds() {
        let mut seed = 7;
        let mut v = 0.0;
        let vals: Vec<f64> = (0..500)
            .map(|_| {
                v += lcg(&mut seed) - 0.5;
                v
            })
            .collect();
        for tol in [0.02, 0.1, 0.5] {
            let mut tr = Track::new();
            let st = Fitter::new()
                .fit_uniform(0.0, 1.0, &vals, tol, &mut tr)
                .unwrap();
            assert!(st.max_err <= tol + 1e-9, "tol {tol} err {}", st.max_err);
        }
    }

    #[test]
    fn irregular_times_f32_and_degenerate_inputs() {
        let s: Vec<(f32, f32)> = [0.0f32, 0.5, 1.7, 2.0, 5.0, 5.5, 9.0]
            .iter()
            .map(|&t| (t, (t * 0.7).sin() * 3.0))
            .collect();
        let mut tr = Track::new();
        let st = Fitter::new().fit(&s, 0.01f32, &mut tr).unwrap();
        assert!(st.max_err <= 0.011);
        let mut f = Fitter::new();
        let mut tr = Track::<f64>::new();
        assert_eq!(f.fit::<f64>(&[], 0.1, &mut tr).unwrap().keys, 0);
        assert_eq!(f.fit(&[(1.0, 2.0)], 0.1, &mut tr).unwrap().keys, 1);
        assert_eq!(
            f.fit(&[(1.0, 2.0), (0.5, 1.0)], 0.1, &mut tr),
            Err(KeysError::Order(1))
        );
        assert_eq!(f.fit(&[(0.0, 0.0)], -1.0, &mut tr), Err(KeysError::Scale));
        assert_eq!(
            f.fit(&[(0.0, 0.0)], f64::NAN, &mut tr),
            Err(KeysError::Scale)
        );
    }

    #[test]
    fn linear_and_constant_data_collapse_to_two_keys() {
        let lin: Vec<f64> = (0..100).map(|k| k as f64 * 2.0).collect();
        let mut tr = Track::new();
        assert_eq!(
            Fitter::new()
                .fit_uniform(0.0, 1.0, &lin, 1e-6, &mut tr)
                .unwrap()
                .keys,
            2
        );
        let flat = vec![3.0; 50];
        assert_eq!(
            Fitter::new()
                .fit_uniform(0.0, 1.0, &flat, 1e-6, &mut tr)
                .unwrap()
                .keys,
            2
        );
    }

    #[test]
    fn keeps_extrapolation() {
        let mut tr = Track::new();
        tr.set_extrap(crate::Extrap::Cycle, crate::Extrap::PingPong);
        Fitter::new()
            .fit_uniform(0.0, 1.0, &[0.0, 1.0, 0.0, 1.0], 0.01, &mut tr)
            .unwrap();
        assert_eq!(
            (tr.pre(), tr.post()),
            (crate::Extrap::Cycle, crate::Extrap::PingPong)
        );
    }

    /// The bound re-measured on the returned track (not trusting the reported `max_err`), with every
    /// fitted handle carrying its measured weight.
    #[test]
    fn finished_track_meets_the_bound_and_keeps_measured_weights() {
        let mut seed = 11;
        let mut v = 0.0;
        let noisy: Vec<f64> = (0..400)
            .map(|k| {
                v += lcg(&mut seed) - 0.5;
                v + (k as f64 * 0.07).sin() * 3.0
            })
            .collect();
        let sine: Vec<f64> = (0..300).map(|k| (k as f64 * 0.05).sin() * 10.0).collect();
        for vals in [&noisy, &sine] {
            for tol in [1e-6, 1e-3, 0.05, 0.5] {
                let mut tr = Track::new();
                let st = Fitter::new()
                    .fit_uniform(0.0, 1.0, vals, tol, &mut tr)
                    .unwrap();
                for (k, &x) in vals.iter().enumerate() {
                    let e = (tr.eval(k as f64) - x).abs();
                    assert!(e <= tol, "tol {tol}: sample {k} off by {e}");
                }
                assert!(st.max_err <= tol);
                for k in tr.keys() {
                    for s in [k.inn, k.out] {
                        assert!(s.kind != Tan::User || s.weight.is_some());
                    }
                }
            }
        }
    }

    #[test]
    fn refit_reuses_the_track_and_gives_the_same_result() {
        let vals: Vec<f64> = (0..200).map(|k| (k as f64 * 0.1).sin()).collect();
        let (mut f, mut a, mut b) = (Fitter::new(), Track::new(), Track::new());
        f.fit_uniform(0.0, 1.0, &vals, 0.01, &mut a).unwrap();
        f.fit_uniform(0.0, 1.0, &[3.0; 10], 0.01, &mut b).unwrap();
        f.fit_uniform(0.0, 1.0, &vals, 0.01, &mut b).unwrap();
        assert_eq!(a, b);
    }
}
