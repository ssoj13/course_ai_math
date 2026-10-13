//! Keyframe tracks: keys with per-side tangents, one Bezier evaluator, extrapolation.
//!
//! See `docs/KEYS.md`. A [`Track`] keeps the authoritative [`Key`]s plus a derived segment cache
//! (`Seg`) that makes [`Track::eval`] allocation-free; the keys are private so cache and keys
//! cannot drift apart: every edit goes through a `Track` method, and every key entering a track
//! passes the one validation point, `Key::checked`.

use crate::Real;
use std::fmt;

/// Tangent type of one key side. Auto kinds derive their slope from neighbour positions and values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Tan {
    /// Catmull-Rom slope clamped so a segment never leaves its two keys' value range (Nuke Smooth,
    /// Maya Clamped). The default.
    #[default]
    Smooth,
    /// Handle points straight at the neighbour key on this side.
    Linear,
    /// Hold (step): the OUT side holds the key's value until the next key. A Constant side is never
    /// coupled to the other side of its key (see [`Track`]).
    Constant,
    /// Zero slope.
    Flat,
    /// Slope between the two neighbouring keys; may overshoot.
    CatmullRom,
    /// Stored [`Side::slope`] and optional [`Side::weight`].
    User,
}

impl Tan {
    /// Every kind, in the order a UI lists them.
    pub const ALL: [Tan; 6] = [
        Tan::Smooth,
        Tan::Linear,
        Tan::Constant,
        Tan::Flat,
        Tan::CatmullRom,
        Tan::User,
    ];

    /// Display name.
    pub fn label(self) -> &'static str {
        match self {
            Tan::Smooth => "Smooth",
            Tan::Linear => "Linear",
            Tan::Constant => "Constant",
            Tan::Flat => "Flat",
            Tan::CatmullRom => "Catmull-Rom",
            Tan::User => "User",
        }
    }
}

/// One side of a key.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Side<T: Real = f64> {
    /// The tangent type.
    #[cfg_attr(feature = "serde", serde(default))]
    pub kind: Tan,
    /// Value per time unit. Used by [`Tan::User`] only (zero otherwise). Must be finite.
    #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "is_zero"))]
    pub slope: T,
    /// Handle length in **time** units, finite and `>= 0`; `None` = a third of the segment span.
    /// [`Tan::User`] only.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub weight: Option<T>,
}

#[cfg(feature = "serde")]
fn is_zero<T: Real>(v: &T) -> bool {
    *v == T::ZERO
}

impl<T: Real> Default for Side<T> {
    fn default() -> Self {
        Self::new(Tan::Smooth)
    }
}

impl<T: Real> Side<T> {
    /// A side of an auto/hold kind (no stored slope).
    pub fn new(kind: Tan) -> Self {
        Self {
            kind,
            slope: T::ZERO,
            weight: None,
        }
    }

    /// A [`Tan::User`] side.
    pub fn user(slope: T, weight: Option<T>) -> Self {
        Self {
            kind: Tan::User,
            slope,
            weight,
        }
    }

    /// Drops slope/weight that the kind does not use and validates the rest (finite slope, finite
    /// non-negative weight), so equality and serde are stable and eval never sees a bad number.
    fn checked(mut self) -> Result<Self, KeysError> {
        if self.kind != Tan::User {
            self.slope = T::ZERO;
            self.weight = None;
        }
        if !self.slope.is_finite() {
            return Err(KeysError::NotFinite);
        }
        match self.weight {
            Some(w) if !w.is_finite() => Err(KeysError::NotFinite),
            Some(w) if w < T::ZERO => Err(KeysError::Weight),
            _ => Ok(self),
        }
    }

    /// Makes `self` follow `src` (the coupling of an unbroken key): same kind, same slope for `User`,
    /// weights stay independent. A `Constant` side neither propagates nor is overwritten.
    fn follow(&mut self, src: &Self) {
        if src.kind == Tan::Constant || self.kind == Tan::Constant {
            return;
        }
        self.kind = src.kind;
        self.slope = src.slope;
        if src.kind != Tan::User {
            self.weight = None;
        }
    }
}

/// Which side of a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideId {
    /// The side shaping the segment that arrives at the key.
    In,
    /// The side shaping the segment that leaves the key.
    Out,
}

/// A key: time, value and the two tangent sides.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Key<T: Real = f64> {
    t: T,
    v: T,
    /// Shapes the segment arriving at this key.
    #[cfg_attr(feature = "serde", serde(default, rename = "in"))]
    pub inn: Side<T>,
    /// Shapes the segment leaving this key.
    #[cfg_attr(feature = "serde", serde(default))]
    pub out: Side<T>,
    /// Sides independent. When `false` a [`Track`] keeps them coupled (the `in` side follows).
    #[cfg_attr(feature = "serde", serde(default))]
    pub broken: bool,
}

/// A finite time with `-0.0` mapped to `0.0`, so two spellings of zero are one time.
fn clean<T: Real>(t: T) -> Result<T, KeysError> {
    if t.is_finite() {
        Ok(t + T::ZERO)
    } else {
        Err(KeysError::NotFinite)
    }
}

impl<T: Real> Key<T> {
    /// A `Smooth`/`Smooth` unbroken key.
    pub fn new(t: T, v: T) -> Self {
        Self {
            t,
            v,
            inn: Side::default(),
            out: Side::default(),
            broken: false,
        }
    }

    /// This key with both sides set to `tan` (unbroken); `Constant` sets the out side only.
    pub fn with_tan(mut self, tan: Tan) -> Self {
        self.set_tan(tan);
        self
    }

    fn set_tan(&mut self, tan: Tan) {
        self.out = Side::new(tan);
        if tan != Tan::Constant {
            self.inn = Side::new(tan);
        }
        self.broken = false;
    }

    /// Time.
    pub fn t(&self) -> T {
        self.t
    }

    /// Value.
    pub fn v(&self) -> T {
        self.v
    }

    /// The sides agree as far as an unbroken key requires: same kind and slope, or a `Constant` side.
    pub(crate) fn coupled(&self) -> bool {
        let (i, o) = (&self.inn, &self.out);
        i.kind == Tan::Constant
            || o.kind == Tan::Constant
            || (i.kind == o.kind && i.slope == o.slope)
    }

    /// THE validation point: every key entering a track passes here. Finite time (canonical zero)
    /// and value, valid sides, and an unbroken key gets its coupled sides.
    fn checked(mut self) -> Result<Self, KeysError> {
        self.t = clean(self.t)?;
        if !self.v.is_finite() {
            return Err(KeysError::NotFinite);
        }
        self.inn = self.inn.checked()?;
        self.out = self.out.checked()?;
        if !self.broken {
            let out = self.out;
            self.inn.follow(&out);
        }
        Ok(self)
    }
}

/// Extrapolation of a track before its first / after its last key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Extrap {
    /// Hold the end value.
    #[default]
    Constant,
    /// Continue with the end key's outer slope.
    Linear,
    /// Repeat the key range.
    Cycle,
    /// Repeat the key range, each repetition offset by the range's total value change.
    CycleOffset,
    /// Repeat forward then reversed.
    PingPong,
}

impl Extrap {
    /// Every mode, in the order a UI lists them.
    pub const ALL: [Extrap; 5] = [
        Extrap::Constant,
        Extrap::Linear,
        Extrap::Cycle,
        Extrap::CycleOffset,
        Extrap::PingPong,
    ];

    /// Display name.
    pub fn label(self) -> &'static str {
        match self {
            Extrap::Constant => "Constant",
            Extrap::Linear => "Linear",
            Extrap::Cycle => "Cycle",
            Extrap::CycleOffset => "Cycle offset",
            Extrap::PingPong => "Ping-pong",
        }
    }

    fn cyclic(self) -> bool {
        matches!(self, Extrap::Cycle | Extrap::CycleOffset | Extrap::PingPong)
    }
}

/// Errors of track construction and editing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeysError {
    /// A time, value, slope, weight or sample is NaN or infinite.
    NotFinite,
    /// Key times must be strictly increasing: the key at this index is not after its predecessor,
    /// or (move/shift) lands on this key's time.
    Order(usize),
    /// Key index out of range.
    Index,
    /// A scale factor or period must be finite and positive, a tolerance finite and non-negative.
    Scale,
    /// A handle weight must not be negative.
    Weight,
    /// Wrong number of tracks for a [`Channels`] value.
    Count,
}

impl fmt::Display for KeysError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFinite => f.write_str("a time, value, slope or weight is not finite"),
            Self::Order(i) => write!(f, "key times must be strictly increasing (key {i})"),
            Self::Index => f.write_str("key index out of range"),
            Self::Scale => f.write_str("scale, period or tolerance out of range"),
            Self::Weight => f.write_str("handle weight must not be negative"),
            Self::Count => f.write_str("wrong number of tracks for the channel count"),
        }
    }
}

impl std::error::Error for KeysError {}

/// How a segment is evaluated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Weighted Bezier: the time curve is solved for the parameter.
    Curve,
    /// `a == b == 1/3`: `X(u) = u`, no solve.
    Even,
    /// Left value until the right key.
    Hold0,
    /// Left value at `t0` only, right value after it.
    Hold1,
}

/// Derived data of one segment: everything `eval` needs (9 scalars + the mode: 40 B at f32, 80 B at f64).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Seg<T: Real> {
    t0: T,
    t1: T,
    inv_s: T,
    v0: T,
    v1: T,
    /// Value controls (`v0 + m0*w0`, `v1 - m1*w1`).
    y1: T,
    y2: T,
    /// Normalized handle widths `w/s` in `[0,1]`.
    a: T,
    b: T,
    mode: Mode,
}

/// Gauss-Legendre 3-point rule on `[0,1]`: exact up to degree 5, which is the degree of
/// `Y(u) * X'(u)` (a cubic times a quadratic).
const GL_X: [f64; 3] = [0.112_701_665_379_258_3, 0.5, 0.887_298_334_620_741_7];
const GL_W: [f64; 3] = [5.0 / 18.0, 4.0 / 9.0, 5.0 / 18.0];

impl<T: Real> Seg<T> {
    /// A curved segment from end points and the two handles as `(slope, absolute width)` (width clamped here).
    pub(crate) fn curve(t0: T, t1: T, v0: T, v1: T, h0: (T, T), h1: (T, T)) -> Self {
        let ((m0, w0), (m1, w1)) = (h0, h1);
        let s = t1 - t0;
        let (w0, w1) = (w0.clamp(T::ZERO, s), w1.clamp(T::ZERO, s));
        let (a, b) = (w0 / s, w1 / s);
        let third = T::ONE / T::of(3.0);
        let near = |x: T| (x - third).abs() <= T::EPSILON * T::of(4.0);
        Self {
            t0,
            t1,
            inv_s: T::ONE / s,
            v0,
            v1,
            y1: v0 + m0 * w0,
            y2: v1 - m1 * w1,
            a,
            b,
            mode: if near(a) && near(b) {
                Mode::Even
            } else {
                Mode::Curve
            },
        }
    }

    /// Derived values safe to evaluate: a 16x margin keeps every intermediate of the Bezier, slope and
    /// integral formulas finite. Holds only use the end values.
    fn finite(&self) -> bool {
        let big = T::of(16.0);
        let ends = (self.v0 * big).is_finite() && (self.v1 * big).is_finite();
        match self.mode {
            Mode::Hold0 | Mode::Hold1 => ends,
            Mode::Curve | Mode::Even => {
                ends && (self.y1 * big).is_finite()
                    && (self.y2 * big).is_finite()
                    && self.inv_s.is_finite()
                    && self.a.is_finite()
                    && self.b.is_finite()
            }
        }
    }

    /// Curve parameter at normalized time `tau`.
    fn u(&self, tau: T) -> T {
        if self.mode == Mode::Even {
            tau
        } else {
            solve_u(self.a, self.b, tau)
        }
    }

    fn tau(&self, t: T) -> T {
        ((t - self.t0) * self.inv_s).clamp(T::ZERO, T::ONE)
    }

    pub(crate) fn eval(&self, t: T) -> T {
        match self.mode {
            Mode::Hold0 => self.v0,
            Mode::Hold1 => {
                if t <= self.t0 {
                    self.v0
                } else {
                    self.v1
                }
            }
            Mode::Curve | Mode::Even => {
                bez(self.v0, self.y1, self.y2, self.v1, self.u(self.tau(t)))
            }
        }
    }

    /// `dv/dt` at `t` (right-hand at the left end); 0 on a hold and where a zero-width handle makes
    /// the time curve stand still.
    fn slope(&self, t: T) -> T {
        if matches!(self.mode, Mode::Hold0 | Mode::Hold1) {
            return T::ZERO;
        }
        let u = self.u(self.tau(t));
        let (v, three) = (T::ONE - u, T::of(3.0));
        let dy = three
            * ((self.y1 - self.v0) * v * v
                + T::of(2.0) * (self.y2 - self.y1) * u * v
                + (self.v1 - self.y2) * u * u);
        let dx = xprime(self.a, self.b, u);
        if dx > T::ZERO {
            dy * self.inv_s / dx
        } else {
            T::ZERO
        }
    }

    /// Exact integral of the segment from `t0` to `t` (clamped into the segment): `s * int Y X' du`
    /// up to the solved parameter, no per-kind code.
    fn int(&self, t: T) -> T {
        let d = t.clamp(self.t0, self.t1) - self.t0;
        match self.mode {
            Mode::Hold0 => self.v0 * d,
            Mode::Hold1 => self.v1 * d,
            Mode::Curve | Mode::Even => {
                let u = self.u(self.tau(t));
                let sum = (0..3).fold(T::ZERO, |acc, k| {
                    let w = u * T::of(GL_X[k]);
                    acc + T::of(GL_W[k])
                        * bez(self.v0, self.y1, self.y2, self.v1, w)
                        * xprime(self.a, self.b, w)
                });
                (self.t1 - self.t0) * u * sum
            }
        }
    }
}

/// Cubic Bezier value with control values `p0..p3` at `u`.
#[inline]
fn bez<T: Real>(p0: T, p1: T, p2: T, p3: T, u: T) -> T {
    let (v, three) = (T::ONE - u, T::of(3.0));
    v * v * v * p0 + three * v * v * u * p1 + three * v * u * u * p2 + u * u * u * p3
}

/// `X'(u)` of the normalized time Bezier with controls `(0, a, 1-b, 1)`.
#[inline]
fn xprime<T: Real>(a: T, b: T, u: T) -> T {
    let v = T::ONE - u;
    T::of(3.0) * (a * v * v + T::of(2.0) * (T::ONE - a - b) * u * v + b * u * u)
}

/// Solves `X(u) = tau` for the time Bezier with controls `(0, a, 1-b, 1)`. `X` is monotone because
/// `a, b` are in `[0,1]`, so the root is unique. Newton inside a bracket; a step that leaves the
/// bracket or converges too slowly is replaced by bisection (`rtsafe`), so it always converges to the
/// tolerance, also for degenerate widths (`a = 0`, `b = 1`, ...).
fn solve_u<T: Real>(a: T, b: T, tau: T) -> T {
    let x = |u: T| bez(T::ZERO, a, T::ONE - b, T::ONE, u);
    let (mut lo, mut hi, mut u) = (T::ZERO, T::ONE, tau);
    let (mut step, mut prev) = (T::ONE, T::ONE);
    let tol = T::EPSILON * T::of(16.0);
    for _ in 0..100 {
        let f = x(u) - tau;
        if f.abs() <= tol {
            break;
        }
        if f < T::ZERO {
            lo = u;
        } else {
            hi = u;
        }
        let d = xprime(a, b, u);
        let n = u - f / d;
        let slow = (T::of(2.0) * f).abs() > (prev * d).abs();
        prev = step;
        if slow || !(n > lo && n < hi) {
            step = T::of(0.5) * (hi - lo);
            u = lo + step;
        } else {
            step = f / d;
            u = n;
        }
    }
    u
}

/// Slope (value per time) of side `side` of key `i`.
fn slope<T: Real>(k: &[Key<T>], i: usize, side: &Side<T>, out: bool) -> T {
    let sec = |a: usize, b: usize| (k[b].v - k[a].v) / (k[b].t - k[a].t);
    let l = (i > 0).then(|| sec(i - 1, i));
    let r = (i + 1 < k.len()).then(|| sec(i, i + 1));
    match side.kind {
        Tan::Flat | Tan::Constant => T::ZERO,
        Tan::User => side.slope,
        Tan::Linear => (if out { r } else { l }).unwrap_or(T::ZERO),
        Tan::CatmullRom | Tan::Smooth => match (l, r) {
            (Some(l), Some(r)) => {
                let cr = (k[i + 1].v - k[i - 1].v) / (k[i + 1].t - k[i - 1].t);
                if side.kind == Tan::CatmullRom {
                    cr
                } else if l * r <= T::ZERO {
                    T::ZERO
                } else {
                    // Fritsch-Carlson limit: with span/3 handles this keeps the handle inside the range.
                    let lim = T::of(3.0) * l.abs().min(r.abs());
                    if cr > T::ZERO {
                        cr.min(lim)
                    } else {
                        cr.max(-lim)
                    }
                }
            }
            (Some(d), None) | (None, Some(d)) => d,
            (None, None) => T::ZERO,
        },
    }
}

/// Effective handle width of a side on a segment of span `s`.
fn width<T: Real>(side: &Side<T>, s: T) -> T {
    match (side.kind, side.weight) {
        (Tan::User, Some(w)) => w.clamp(T::ZERO, s),
        _ => s / T::of(3.0),
    }
}

/// The value of a track outside its key range, in terms of the in-range evaluator.
enum Ext<T> {
    /// A straight line: value and slope at the query time.
    Line(T, T),
    /// `off + sgn * f(t)` with `t` inside the key range (`sgn` is `-1` for a reversed ping-pong pass).
    Wrap { t: T, off: T, sgn: T },
}

/// Running unwind: maps raw values fed in order to values within half a period of their predecessor.
fn unwinder<T: Real>(period: T) -> impl FnMut(T) -> T {
    let half = period * T::of(0.5);
    let (mut off, mut prev) = (T::ZERO, None::<T>);
    move |raw| {
        if let Some(p) = prev {
            let d = raw - p;
            if d > half {
                off = off - period * ((d + half) / period).floor();
            } else if d < -half {
                off = off + period * ((-d + half) / period).floor();
            }
        }
        prev = Some(raw);
        raw + off
    }
}

fn check_period<T: Real>(period: T) -> Result<(), KeysError> {
    if period.is_finite() && period > T::ZERO {
        Ok(())
    } else {
        Err(KeysError::Scale)
    }
}

/// A scalar animation track.
///
/// Segment `i` joins key `i` and `i + 1`; its handles are key `i`'s `out` side and key `i + 1`'s `inn`
/// side. A `Constant` out side holds the left value until the next key; otherwise a `Constant` in side
/// on the right key gives the left value at the left key only and the right value right after it.
/// Everything else is one cubic Bezier in (time, value); handle widths are clamped to the span so the
/// time curve stays monotone.
///
/// Coupling: an unbroken key keeps its `in` side equal to its `out` side (same kind, same `User`
/// slope); a `Constant` side is exempt in both directions, so a step out never turns the previous
/// segment into a step. Every method that takes a key or side validates it (finite numbers, weights
/// `>= 0`, strictly increasing times, `-0.0` read as `0.0`).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "serde",
    serde(try_from = "TrackRaw<T>", into = "TrackRaw<T>")
)]
pub struct Track<T: Real = f64> {
    pre: Extrap,
    post: Extrap,
    keys: Vec<Key<T>>,
    times: Vec<T>,
    segs: Vec<Seg<T>>,
    pre_slope: T,
    post_slope: T,
}

/// The on-disk shape of a [`Track`].
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) struct TrackRaw<T: Real = f64> {
    #[cfg_attr(feature = "serde", serde(default))]
    pre: Extrap,
    #[cfg_attr(feature = "serde", serde(default))]
    post: Extrap,
    keys: Vec<Key<T>>,
}

impl<T: Real> From<Track<T>> for TrackRaw<T> {
    fn from(t: Track<T>) -> Self {
        Self {
            pre: t.pre,
            post: t.post,
            keys: t.keys,
        }
    }
}

impl<T: Real> TryFrom<TrackRaw<T>> for Track<T> {
    type Error = KeysError;
    fn try_from(r: TrackRaw<T>) -> Result<Self, KeysError> {
        let mut t = Self::from_keys(r.keys)?;
        t.pre = r.pre;
        t.post = r.post;
        Ok(t)
    }
}

impl<T: Real> Default for Track<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Real> Track<T> {
    /// An empty track (evaluates to 0).
    pub fn new() -> Self {
        Self {
            pre: Extrap::Constant,
            post: Extrap::Constant,
            keys: Vec::new(),
            times: Vec::new(),
            segs: Vec::new(),
            pre_slope: T::ZERO,
            post_slope: T::ZERO,
        }
    }

    /// A track from keys; times must be finite and strictly increasing.
    pub fn from_keys(keys: Vec<Key<T>>) -> Result<Self, KeysError> {
        let mut t = Self::new();
        t.set_keys(keys)?;
        Ok(t)
    }

    /// Replaces all keys after validating them; on error the track is unchanged.
    pub(crate) fn set_keys(&mut self, mut keys: Vec<Key<T>>) -> Result<(), KeysError> {
        for i in 0..keys.len() {
            keys[i] = keys[i].checked()?;
            if i > 0 && keys[i].t <= keys[i - 1].t {
                return Err(KeysError::Order(i));
            }
        }
        let old = std::mem::replace(&mut self.keys, keys);
        if !self.rebuild() {
            self.keys = old;
            self.rebuild();
            return Err(KeysError::NotFinite);
        }
        Ok(())
    }

    /// Empties the track and hands out its key buffer for reuse (the fitter refills it in place).
    pub(crate) fn take_keys(&mut self) -> Vec<Key<T>> {
        self.times.clear();
        self.segs.clear();
        let mut keys = std::mem::take(&mut self.keys);
        keys.clear();
        keys
    }

    /// The keys, sorted by time.
    pub fn keys(&self) -> &[Key<T>] {
        &self.keys
    }

    /// Number of keys.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// No keys.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// First and last key time.
    pub fn range(&self) -> Option<(T, T)> {
        Some((*self.times.first()?, *self.times.last()?))
    }

    /// Extrapolation before the first key.
    pub fn pre(&self) -> Extrap {
        self.pre
    }

    /// Extrapolation after the last key.
    pub fn post(&self) -> Extrap {
        self.post
    }

    /// Sets the extrapolation modes.
    pub fn set_extrap(&mut self, pre: Extrap, post: Extrap) {
        self.pre = pre;
        self.post = post;
    }

    /// Index of the key at time `t`, or where it would be inserted (like `slice::binary_search`).
    pub fn find(&self, t: T) -> Result<usize, usize> {
        let t = t + T::ZERO;
        self.times.binary_search_by(|p| p.total_cmp(&t))
    }

    fn seg_at(&self, i: usize) -> Seg<T> {
        let (a, b) = (&self.keys[i], &self.keys[i + 1]);
        let s = b.t - a.t;
        let mut seg = Seg::curve(
            a.t,
            b.t,
            a.v,
            b.v,
            (slope(&self.keys, i, &a.out, true), width(&a.out, s)),
            (slope(&self.keys, i + 1, &b.inn, false), width(&b.inn, s)),
        );
        if a.out.kind == Tan::Constant {
            seg.mode = Mode::Hold0;
        } else if b.inn.kind == Tan::Constant {
            seg.mode = Mode::Hold1;
        }
        seg
    }

    fn set_ends(&mut self) {
        let n = self.keys.len();
        let end = |i: usize, side: &Side<T>, out: bool| slope(&self.keys, i, side, out);
        (self.pre_slope, self.post_slope) = if n == 0 {
            (T::ZERO, T::ZERO)
        } else {
            (
                end(0, &self.keys[0].out, true),
                end(n - 1, &self.keys[n - 1].inn, false),
            )
        };
    }

    /// Recomputes the whole cache, O(n). `false` when a derived value is not finite (handles that
    /// overflow); the cache is then unusable until the keys are fixed.
    fn rebuild(&mut self) -> bool {
        self.times.clear();
        self.times.extend(self.keys.iter().map(|k| k.t));
        self.segs.clear();
        let mut ok = true;
        for i in 0..self.keys.len().saturating_sub(1) {
            let g = self.seg_at(i);
            ok &= g.finite();
            self.segs.push(g);
        }
        self.set_ends();
        ok && self.ends_finite()
    }

    fn ends_finite(&self) -> bool {
        self.pre_slope.is_finite() && self.post_slope.is_finite()
    }

    /// Recomputes the segments an edit of keys `lo ..= hi` can change (`lo-2 ..= hi+1`); the segment
    /// count and the times must already be right. `false` when a derived value is not finite.
    fn recalc(&mut self, lo: usize, hi: usize) -> bool {
        let n = self.keys.len();
        let mut ok = true;
        if n >= 2 {
            for s in lo.saturating_sub(2)..=(hi + 1).min(n - 2) {
                let g = self.seg_at(s);
                ok &= g.finite();
                self.segs[s] = g;
            }
        }
        self.set_ends();
        ok && self.ends_finite()
    }

    /// [`Track::recalc`] as a `Result`; the caller restores its keys and recalculates on error.
    fn refresh_range(&mut self, lo: usize, hi: usize) -> Result<(), KeysError> {
        if self.recalc(lo, hi) {
            Ok(())
        } else {
            Err(KeysError::NotFinite)
        }
    }

    /// Applies `f` to key `i` and refreshes; if the derived handles overflow, the key is restored and
    /// the edit is [`KeysError::NotFinite`].
    fn edit(&mut self, i: usize, f: impl FnOnce(&mut Key<T>)) -> Result<(), KeysError> {
        let old = *self.keys.get(i).ok_or(KeysError::Index)?;
        f(&mut self.keys[i]);
        self.refresh_range(i, i).inspect_err(|_| {
            self.keys[i] = old;
            self.recalc(i, i);
        })
    }

    /// Puts `key` at index `i` in `keys`, `times` and `segs` (a placeholder segment; the caller refreshes).
    fn splice(&mut self, i: usize, key: Key<T>) {
        self.keys.insert(i, key);
        self.times.insert(i, key.t);
        let n = self.keys.len();
        if n >= 2 {
            let at = i.min(n - 2);
            let seg = self.seg_at(at);
            self.segs.insert(at, seg);
        }
    }

    /// Takes key `i` out of `keys`, `times` and `segs` (the caller refreshes).
    fn unsplice(&mut self, i: usize) -> Key<T> {
        let k = self.keys.remove(i);
        self.times.remove(i);
        if !self.segs.is_empty() {
            self.segs.remove(i.min(self.segs.len() - 1));
        }
        k
    }

    /// Inserts a key, or replaces the key at the same time. Returns its index. A key whose derived
    /// handles overflow is [`KeysError::NotFinite`] and changes nothing.
    pub fn insert(&mut self, key: Key<T>) -> Result<usize, KeysError> {
        let key = key.checked()?;
        match self.find(key.t) {
            Ok(i) => self.edit(i, |k| *k = key).map(|()| i),
            Err(i) => {
                self.splice(i, key);
                self.refresh_range(i, i).map(|()| i).inspect_err(|_| {
                    self.unsplice(i);
                    self.recalc(i, i);
                })
            }
        }
    }

    /// Inserts `(t, v)` with both sides set to `tan` (`Constant`: out side only): the host's "new key
    /// type" is this argument. An existing key at `t` is replaced.
    pub fn add(&mut self, t: T, v: T, tan: Tan) -> Result<usize, KeysError> {
        self.insert(Key::new(t, v).with_tan(tan))
    }

    /// Auto-key: sets the value of the key at `t` keeping its tangents, or inserts a new key with `tan`.
    pub fn set_at(&mut self, t: T, v: T, tan: Tan) -> Result<usize, KeysError> {
        match self.find(t) {
            Ok(i) => self.set_value(i, v).map(|()| i),
            Err(_) => self.add(t, v, tan),
        }
    }

    /// Removes key `i`.
    pub fn remove(&mut self, i: usize) -> Result<Key<T>, KeysError> {
        if i >= self.keys.len() {
            return Err(KeysError::Index);
        }
        let k = self.unsplice(i);
        self.refresh_range(i, i).map(|()| k).inspect_err(|_| {
            self.splice(i, k);
            self.recalc(i, i);
        })
    }

    /// Sets the value of key `i`.
    pub fn set_value(&mut self, i: usize, v: T) -> Result<(), KeysError> {
        if !v.is_finite() {
            return Err(KeysError::NotFinite);
        }
        self.edit(i, |k| k.v = v)
    }

    /// Moves key `i` to time `t` (it may pass its neighbours). Returns the new index. A time that
    /// another key already uses is [`KeysError::Order`] and changes nothing.
    pub fn move_key(&mut self, i: usize, t: T) -> Result<usize, KeysError> {
        let t = clean(t)?;
        if i >= self.keys.len() {
            return Err(KeysError::Index);
        }
        if let Ok(j) = self.find(t) {
            return if j == i {
                Ok(i)
            } else {
                Err(KeysError::Order(j))
            };
        }
        // Keys other than `i` that stay before `t`: the new index.
        let at = self.times.partition_point(|&x| x < t) - usize::from(self.times[i] < t);
        let old_t = self.times[i];
        self.keys[i].t = t;
        self.times[i] = t;
        let (lo, hi) = if at >= i {
            self.keys[i..=at].rotate_left(1);
            self.times[i..=at].rotate_left(1);
            (i, at)
        } else {
            self.keys[at..=i].rotate_right(1);
            self.times[at..=i].rotate_right(1);
            (at, i)
        };
        self.refresh_range(lo, hi).map(|()| at).inspect_err(|_| {
            if at >= i {
                self.keys[i..=at].rotate_right(1);
                self.times[i..=at].rotate_right(1);
            } else {
                self.keys[at..=i].rotate_left(1);
                self.times[at..=i].rotate_left(1);
            }
            self.keys[i].t = old_t;
            self.times[i] = old_t;
            self.recalc(lo, hi);
        })
    }

    /// Moves the keys `sel` by `dt` in one step (a dope-sheet drag). The final state is validated as a
    /// whole: selected keys may pass unselected ones (indices then change, hosts track keys by time),
    /// but landing on another key's time is [`KeysError::Order`] and changes nothing.
    pub fn shift(&mut self, sel: &[usize], dt: T) -> Result<(), KeysError> {
        if !dt.is_finite() {
            return Err(KeysError::NotFinite);
        }
        let mut moved = vec![false; self.keys.len()];
        for &i in sel {
            *moved.get_mut(i).ok_or(KeysError::Index)? = true;
        }
        let mut keys = self.keys.clone();
        for k in keys
            .iter_mut()
            .zip(&moved)
            .filter_map(|(k, &m)| m.then_some(k))
        {
            k.t = k.t + dt;
        }
        keys.sort_by(|a, b| a.t.total_cmp(&b.t));
        self.set_keys(keys)
    }

    /// Sets one side of key `i`. On an unbroken key the other side follows (same kind, same `User`
    /// slope; weights stay independent), except for `Constant` sides.
    pub fn set_side(&mut self, i: usize, id: SideId, side: Side<T>) -> Result<(), KeysError> {
        let side = side.checked()?;
        self.edit(i, |k| {
            let (mine, other) = match id {
                SideId::In => (&mut k.inn, &mut k.out),
                SideId::Out => (&mut k.out, &mut k.inn),
            };
            *mine = side;
            if !k.broken {
                other.follow(&side);
            }
        })
    }

    /// Sets both sides of key `i` to `tan` (`Constant`: out side only) and unbreaks it.
    pub fn set_tan(&mut self, i: usize, tan: Tan) -> Result<(), KeysError> {
        self.edit(i, |k| k.set_tan(tan))
    }

    /// Sets a user handle of key `i` (slope and optional time weight), see [`Track::set_side`].
    pub fn set_user(
        &mut self,
        i: usize,
        id: SideId,
        slope: T,
        weight: Option<T>,
    ) -> Result<(), KeysError> {
        self.set_side(i, id, Side::user(slope, weight))
    }

    /// Makes a side a `User` side at its current effective slope (an auto handle becomes editable
    /// without jumping). A side that is already `User` is left alone.
    pub fn freeze(&mut self, i: usize, id: SideId) -> Result<(), KeysError> {
        let k = self.keys.get(i).ok_or(KeysError::Index)?;
        let side = match id {
            SideId::In => &k.inn,
            SideId::Out => &k.out,
        };
        if side.kind == Tan::User {
            return Ok(());
        }
        let m = slope(&self.keys, i, side, id == SideId::Out);
        self.set_side(i, id, Side::user(m, None))
    }

    /// Drags the handle point of a side to the offset `(dt, dv)` from its key: slope `dv/dt` and weight
    /// `|dt|`. A handle dragged to or across the key (wrong sign of `dt`) gets weight 0 and keeps its
    /// slope.
    pub fn set_handle(&mut self, i: usize, id: SideId, (dt, dv): (T, T)) -> Result<(), KeysError> {
        if !(dt.is_finite() && dv.is_finite()) {
            return Err(KeysError::NotFinite);
        }
        let k = self.keys.get(i).ok_or(KeysError::Index)?;
        let (side, w) = match id {
            SideId::In => (&k.inn, -dt),
            SideId::Out => (&k.out, dt),
        };
        let w = w.max(T::ZERO);
        let m = if w > T::ZERO {
            dv / dt
        } else {
            slope(&self.keys, i, side, id == SideId::Out)
        };
        self.set_side(i, id, Side::user(m, Some(w)))
    }

    /// Breaks or unbreaks key `i`; unbreaking makes the in side follow the out side.
    pub fn set_broken(&mut self, i: usize, broken: bool) -> Result<(), KeysError> {
        self.edit(i, |k| {
            k.broken = broken;
            if !broken {
                let out = k.out;
                k.inn.follow(&out);
            }
        })
    }

    /// Sets the two handles of segment `i` (key `i` out, key `i+1` in) to `tan`. A key whose sides then
    /// differ is broken, one whose sides match again is unbroken: calling it back with the neighbours'
    /// kind restores the original coupling. This is how a "linear segment" next to smooth keys is expressed.
    pub fn set_seg(&mut self, i: usize, tan: Tan) -> Result<(), KeysError> {
        if i + 1 >= self.keys.len() {
            return Err(KeysError::Index);
        }
        let s = Side::new(tan);
        let old = [self.keys[i], self.keys[i + 1]];
        self.keys[i].out = s;
        self.keys[i + 1].inn = s;
        for k in &mut self.keys[i..=i + 1] {
            k.broken = !k.coupled();
        }
        self.refresh_range(i, i + 1).inspect_err(|_| {
            self.keys[i..=i + 1].copy_from_slice(&old);
            self.recalc(i, i + 1);
        })
    }

    /// Effective handle of one side of key `i` as an offset `(dt, dv)` from the key, derived for auto
    /// kinds too (what an editor draws). `None` for a bad index and for a side with no segment (the
    /// first key's in side, the last key's out side).
    pub fn handle(&self, i: usize, id: SideId) -> Option<(T, T)> {
        let k = self.keys.get(i)?;
        let (side, out) = match id {
            SideId::In => (&k.inn, false),
            SideId::Out => (&k.out, true),
        };
        let nb = if out {
            i.checked_add(1)
        } else {
            i.checked_sub(1)
        }?;
        let s = (*self.times.get(nb)? - k.t).abs();
        let (m, w) = (slope(&self.keys, i, side, out), width(side, s));
        Some(if out { (w, m * w) } else { (-w, -m * w) })
    }

    /// Multiplies all times (and weights) by `k` and divides slopes, keeping the curve shape: a
    /// re-timing or fps change. The result is validated (finite, still strictly increasing); on error
    /// the track is unchanged.
    pub fn scale_time(&mut self, k: T) -> Result<(), KeysError> {
        check_period(k)?;
        let mut keys = self.keys.clone();
        for key in &mut keys {
            key.t = key.t * k;
            for s in [&mut key.inn, &mut key.out] {
                s.slope = s.slope / k;
                s.weight = s.weight.map(|w| w * k);
            }
        }
        self.set_keys(keys)
    }

    /// Makes consecutive key values differ by at most half `period` (rotations: shortest path). An
    /// invalid period or an overflow is an error and changes nothing.
    pub fn unwind(&mut self, period: T) -> Result<(), KeysError> {
        check_period(period)?;
        let mut f = unwinder(period);
        let mut keys = self.keys.clone();
        for k in &mut keys {
            k.v = f(k.v);
        }
        self.set_keys(keys)
    }

    /// Value at time `t`. A NaN time gives NaN, an infinite time the end key's value.
    pub fn eval(&self, t: T) -> T {
        self.eval_hint(t, &mut 0)
    }

    /// Value at time `t`, using and updating a segment index hint: sequential access is O(1).
    pub fn eval_hint(&self, t: T, hint: &mut usize) -> T {
        if self.keys.is_empty() {
            return T::ZERO;
        }
        if t.partial_cmp(&t).is_none() {
            return t;
        }
        match self.outside(t) {
            None => self.eval_in(t, hint),
            Some(Ext::Line(v, _)) => v,
            Some(Ext::Wrap { t, off, .. }) => off + self.eval_in(t, hint),
        }
    }

    /// `dv/dt` at time `t`: the right-hand slope at a key, 0 on holds and for a non-finite `t`, the
    /// extrapolated slope outside the range (negated on a reversed ping-pong pass).
    pub fn slope_at(&self, t: T) -> T {
        if self.keys.is_empty() || !t.is_finite() {
            return T::ZERO;
        }
        match self.outside(t) {
            None => self.slope_in(t),
            Some(Ext::Line(_, s)) => s,
            Some(Ext::Wrap { t, sgn, .. }) => sgn * self.slope_in(t),
        }
    }

    /// Exact integral of the track from `a` to `b` (negative when `b < a`), extrapolation included:
    /// holds and every Bezier segment, weighted or not, in closed form. NaN for a non-finite bound.
    /// O(number of segments between the bounds and the first key).
    pub fn integrate(&self, a: T, b: T) -> T {
        if !(a.is_finite() && b.is_finite()) {
            return T::NAN;
        }
        self.cum(b) - self.cum(a)
    }

    /// Fills `out[k] = eval(t0 + k*dt)`; one segment walk, no search per sample.
    pub fn eval_many(&self, t0: T, dt: T, out: &mut [T]) {
        let mut hint = 0;
        for (k, o) in out.iter_mut().enumerate() {
            *o = self.eval_hint(t0 + dt * T::of(k as f64), &mut hint);
        }
    }

    /// Index of the segment containing `t` (`t` within the key range, at least two keys).
    fn seg_idx(&self, t: T) -> usize {
        let i = self.times.partition_point(|&x| x <= t).saturating_sub(1);
        i.min(self.segs.len() - 1)
    }

    /// `t` within `[t_first, t_last]`.
    fn eval_in(&self, t: T, hint: &mut usize) -> T {
        let n = self.keys.len();
        if t >= self.times[n - 1] {
            return self.keys[n - 1].v;
        }
        let ok = |s: &Seg<T>| s.t0 <= t && t < s.t1;
        let h = (*hint).min(self.segs.len() - 1);
        let i = if ok(&self.segs[h]) {
            h
        } else if self.segs.get(h + 1).is_some_and(ok) {
            h + 1
        } else {
            self.seg_idx(t)
        };
        *hint = i;
        self.segs[i].eval(t)
    }

    /// Slope inside the key range (0 for a single key).
    fn slope_in(&self, t: T) -> T {
        if self.keys.len() < 2 {
            return T::ZERO;
        }
        self.segs[self.seg_idx(t)].slope(t)
    }

    /// Cycle index and remainder of `t` (outside the range) for a cyclic extrapolation mode; `None`
    /// when the mode is not cyclic, the range is degenerate, or `t` is so far out that they overflow.
    fn cycle(&self, t: T, pre: bool) -> Option<(T, T)> {
        if !(if pre { self.pre } else { self.post }).cyclic() {
            return None;
        }
        let (t0, tn) = (self.times[0], self.times[self.times.len() - 1]);
        let period = tn - t0;
        if !(period > T::ZERO && period.is_finite()) {
            return None;
        }
        let k = ((t - t0) / period).floor();
        let rem = (t - t0 - k * period).clamp(T::ZERO, period);
        (k.is_finite() && rem.is_finite()).then_some((k, rem))
    }

    /// How `t` outside `[t_first, t_last]` is evaluated; `None` when `t` is inside. Infinite times and
    /// cycle overflows give the end value.
    fn outside(&self, t: T) -> Option<Ext<T>> {
        let n = self.keys.len();
        let (t0, tn) = (self.times[0], self.times[n - 1]);
        let pre = if t < t0 {
            true
        } else if t > tn {
            false
        } else {
            return None;
        };
        let (te, ve, se, mode) = if pre {
            (t0, self.keys[0].v, self.pre_slope, self.pre)
        } else {
            (tn, self.keys[n - 1].v, self.post_slope, self.post)
        };
        let end = Ext::Line(ve, T::ZERO);
        if !t.is_finite() {
            return Some(end);
        }
        Some(match (mode, self.cycle(t, pre)) {
            (Extrap::Linear, _) => {
                let v = ve + se * (t - te);
                // So far out that the line overflows: the end value, like a cycle overflow.
                if v.is_finite() { Ext::Line(v, se) } else { end }
            }
            (Extrap::Cycle, Some((_, rem))) => Ext::Wrap {
                t: t0 + rem,
                off: T::ZERO,
                sgn: T::ONE,
            },
            (Extrap::CycleOffset, Some((k, rem))) => {
                let off = k * (self.keys[n - 1].v - self.keys[0].v);
                if (off * T::of(4.0)).is_finite() {
                    Ext::Wrap {
                        t: t0 + rem,
                        off,
                        sgn: T::ONE,
                    }
                } else {
                    end
                }
            }
            (Extrap::PingPong, Some((k, rem))) => {
                let odd = k - T::of(2.0) * (k * T::of(0.5)).floor() != T::ZERO;
                if odd {
                    Ext::Wrap {
                        t: tn - rem,
                        off: T::ZERO,
                        sgn: -T::ONE,
                    }
                } else {
                    Ext::Wrap {
                        t: t0 + rem,
                        off: T::ZERO,
                        sgn: T::ONE,
                    }
                }
            }
            _ => end,
        })
    }

    /// Integral from the first key to `t` inside the key range.
    fn cum_in(&self, t: T) -> T {
        if self.keys.len() < 2 {
            return T::ZERO;
        }
        let i = self.seg_idx(t);
        let before = self.segs[..i].iter().fold(T::ZERO, |s, g| s + g.int(g.t1));
        before + self.segs[i].int(t)
    }

    /// Integral from the first key to any finite `t` (negative before the first key).
    fn cum(&self, t: T) -> T {
        let n = self.keys.len();
        if n == 0 {
            return T::ZERO;
        }
        let (t0, tn) = (self.times[0], self.times[n - 1]);
        if t >= t0 && t <= tn {
            return self.cum_in(t);
        }
        let pre = t < t0;
        let total = self.cum_in(tn);
        let (te, ve, se, mode) = if pre {
            (t0, self.keys[0].v, self.pre_slope, self.pre)
        } else {
            (tn, self.keys[n - 1].v, self.post_slope, self.post)
        };
        let base = if pre { T::ZERO } else { total };
        let d = t - te;
        let line = |s: T| base + ve * d + s * d * d * T::of(0.5);
        let Some((k, rem)) = self.cycle(t, pre) else {
            return line(if mode == Extrap::Linear { se } else { T::ZERO });
        };
        let (span, dv) = (tn - t0, self.keys[n - 1].v - self.keys[0].v);
        let whole = k * total;
        match mode {
            Extrap::Cycle => whole + self.cum_in(t0 + rem),
            // Period `j` is offset by `j*dv`: periods 0..k add `dv*span*k(k-1)/2`, the partial one `k*dv*rem`.
            Extrap::CycleOffset => {
                whole
                    + dv * span * k * (k - T::ONE) * T::of(0.5)
                    + self.cum_in(t0 + rem)
                    + k * dv * rem
            }
            _ => {
                let odd = k - T::of(2.0) * (k * T::of(0.5)).floor() != T::ZERO;
                // A reversed pass over `[x, t_last]` integrates to `total - cum(t_last - rem)`.
                whole
                    + if odd {
                        total - self.cum_in(tn - rem)
                    } else {
                        self.cum_in(t0 + rem)
                    }
            }
        }
    }
}

/// Makes consecutive values differ by at most half `period` by adding multiples of `period`. An
/// invalid period or an overflow is an error and changes nothing.
pub fn unwind_slice<T: Real>(vals: &mut [T], period: T) -> Result<(), KeysError> {
    check_period(period)?;
    let mut f = unwinder(period);
    if !vals.iter().all(|&v| f(v).is_finite()) {
        return Err(KeysError::NotFinite);
    }
    let mut f = unwinder(period);
    for v in vals {
        *v = f(*v);
    }
    Ok(())
}

/// `N` independent tracks: a vector, a colour, Euler angles. Each component has its own keys.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "serde",
    serde(try_from = "Vec<Track<T>>", into = "Vec<Track<T>>")
)]
pub struct Channels<T: Real, const N: usize>([Track<T>; N]);

impl<T: Real, const N: usize> Default for Channels<T, N> {
    fn default() -> Self {
        Self(std::array::from_fn(|_| Track::new()))
    }
}

impl<T: Real, const N: usize> From<Channels<T, N>> for Vec<Track<T>> {
    fn from(c: Channels<T, N>) -> Self {
        c.0.into()
    }
}

impl<T: Real, const N: usize> TryFrom<Vec<Track<T>>> for Channels<T, N> {
    type Error = KeysError;
    fn try_from(v: Vec<Track<T>>) -> Result<Self, KeysError> {
        v.try_into().map(Self).map_err(|_| KeysError::Count)
    }
}

impl<T: Real, const N: usize> Channels<T, N> {
    /// Channels from tracks.
    pub fn new(tracks: [Track<T>; N]) -> Self {
        Self(tracks)
    }

    /// Component `i`.
    pub fn track(&self, i: usize) -> Option<&Track<T>> {
        self.0.get(i)
    }

    /// Mutable component `i`.
    pub fn track_mut(&mut self, i: usize) -> Option<&mut Track<T>> {
        self.0.get_mut(i)
    }

    /// All components at time `t`.
    pub fn eval(&self, t: T) -> [T; N] {
        std::array::from_fn(|i| self.0[i].eval(t))
    }

    /// All components at time `t` with one segment hint per component.
    pub fn eval_hint(&self, t: T, hints: &mut [usize; N]) -> [T; N] {
        std::array::from_fn(|i| self.0[i].eval_hint(t, &mut hints[i]))
    }
}

#[cfg(test)]
mod tests;
