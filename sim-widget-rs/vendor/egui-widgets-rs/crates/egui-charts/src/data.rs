//! Series storage and the level-of-detail pyramid drawing reads from.
//!
//! No egui here, by rule: this module (like [`crate::scale`]) must stay
//! renderer-agnostic so it can move to a standalone core crate that a wgpu
//! backend consumes without a UI toolkit. See `CLAUDE.md`.
//!
//! # Why a pyramid, and why it is exact
//!
//! The target is that nothing dies when a caller hands over far more data than
//! anyone would display: a year of 5-second bars is 6_307_200 of them.
//! Reducing that to ~2000 pixel columns every frame is ~378M reads a second at
//! 60fps.
//!
//! OHLC aggregation is **associative** — `high` is a max, `low` a min, `open`
//! the first and `close` the last — so a level built from the level below is
//! *exactly* what a direct fold of the raw bars would give. That is the whole
//! reason this is a pyramid and not a cache of guesses, and
//! `a_level_equals_the_direct_fold` pins it.
//!
//! With [`FANOUT`] of 8 the levels for 6.3M bars are 788k, 98k, 12k, 1.5k, 193.
//! Drawing picks the level where a pixel column covers 1..8 bars, so the reads
//! per frame are bounded by the pane's width whatever the dataset size, and the
//! extra memory is the geometric series 1/(8-1) = ~14%.
//!
//! # Storage is columnar
//!
//! Separate `Vec` per field rather than a `Vec<Bar>`: a fold touches one field
//! at a time (min over lows, max over highs), and an array-of-structs pulls
//! four times the needed bytes through cache to do it.

/// Bars aggregated into one entry of the next level up.
///
/// 8 keeps the pyramid shallow (six levels covers a year of 5-second bars)
/// while leaving at most 7 extra reads per drawn column.
pub const FANOUT: usize = 8;

/// One bar, as read out of a series. A plain value — the storage is columnar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bar {
    /// Epoch millis of the bar's open.
    pub t: i64,
    pub o: f64,
    pub h: f64,
    pub l: f64,
    pub c: f64,
    pub v: f64,
}

impl Bar {
    pub fn new(t: i64, o: f64, h: f64, l: f64, c: f64, v: f64) -> Self {
        Self { t, o, h, l, c, v }
    }

    /// Fold `other` in after `self`, as the next bar in time.
    ///
    /// The associativity the pyramid rests on. Order matters: `close` is the
    /// *later* bar's, so this is not commutative and must never be applied to
    /// bars out of order.
    pub fn merge(self, other: Bar) -> Bar {
        Bar {
            t: self.t,
            o: self.o,
            h: self.h.max(other.h),
            l: self.l.min(other.l),
            c: other.c,
            v: self.v + other.v,
        }
    }

    pub fn is_up(&self) -> bool {
        self.c >= self.o
    }
}

/// How a series' timestamps are stored.
///
/// A regularly-sampled series needs no time array at all — 6.3M timestamps is
/// 50MB that is entirely derivable. Explicit stamps are for data with gaps.
#[derive(Clone, Debug, PartialEq)]
pub enum Time {
    /// `t0 + i * step`, in millis.
    Regular {
        t0: i64,
        step: i64,
    },
    Explicit(Vec<i64>),
}

impl Time {
    pub fn at(&self, i: usize) -> i64 {
        match self {
            Time::Regular { t0, step } => t0 + (i as i64) * step,
            // Clamped rather than panicking: an index past the end is a
            // drawing bug, and taking the window down with it helps nobody.
            Time::Explicit(v) => v
                .last()
                .copied()
                .map_or(0, |last| v.get(i).copied().unwrap_or(last)),
        }
    }

    fn push(&mut self, t: i64) {
        if let Time::Explicit(v) = self {
            v.push(t);
        }
    }
}

/// Columnar OHLCV, one level of the pyramid.
#[derive(Clone, Debug, Default, PartialEq)]
struct Cols {
    t: Vec<i64>,
    o: Vec<f64>,
    h: Vec<f64>,
    l: Vec<f64>,
    c: Vec<f64>,
    v: Vec<f64>,
}

impl Cols {
    fn len(&self) -> usize {
        self.o.len()
    }

    fn get(&self, i: usize) -> Option<Bar> {
        Some(Bar {
            t: *self.t.get(i)?,
            o: *self.o.get(i)?,
            h: *self.h.get(i)?,
            l: *self.l.get(i)?,
            c: *self.c.get(i)?,
            v: *self.v.get(i)?,
        })
    }

    /// Write `bar` at `i`, extending by one when `i` is the end.
    ///
    /// Overwrite rather than push-only because appending a raw bar *revises*
    /// the tail entry of every level above: the group it belongs to is not
    /// finished yet.
    fn set(&mut self, i: usize, bar: Bar) {
        if i == self.len() {
            self.t.push(bar.t);
            self.o.push(bar.o);
            self.h.push(bar.h);
            self.l.push(bar.l);
            self.c.push(bar.c);
            self.v.push(bar.v);
        } else if i < self.len() {
            self.t[i] = bar.t;
            self.o[i] = bar.o;
            self.h[i] = bar.h;
            self.l[i] = bar.l;
            self.c[i] = bar.c;
            self.v[i] = bar.v;
        }
    }
}

/// A price series with its level-of-detail pyramid.
///
/// The caller owns one of these and appends to it; the widget borrows it for
/// the frame. Building the pyramid per frame would defeat the point of having
/// one, which is why this is a stored object and not a function over a slice.
#[derive(Clone, Debug)]
pub struct Series {
    time: Time,
    /// Level 0 — the raw bars.
    raw: Cols,
    /// `levels[k]` aggregates `FANOUT^(k+1)` raw bars per entry.
    levels: Vec<Cols>,
}

impl Series {
    /// An empty series with regular sampling (`step` in millis).
    pub fn regular(t0: i64, step: i64) -> Self {
        Self {
            time: Time::Regular {
                t0,
                step: step.max(1),
            },
            raw: Cols::default(),
            levels: Vec::new(),
        }
    }

    /// An empty series carrying an explicit timestamp per bar.
    pub fn irregular() -> Self {
        Self {
            time: Time::Explicit(Vec::new()),
            raw: Cols::default(),
            levels: Vec::new(),
        }
    }

    pub fn from_bars(time: Time, bars: &[Bar]) -> Self {
        let mut s = Self {
            time,
            raw: Cols::default(),
            levels: Vec::new(),
        };
        for b in bars {
            s.push(*b);
        }
        s
    }

    pub fn len(&self) -> usize {
        self.raw.len()
    }

    pub fn is_empty(&self) -> bool {
        self.raw.len() == 0
    }

    pub fn time(&self) -> &Time {
        &self.time
    }

    /// Bar `i` at full resolution.
    pub fn bar(&self, i: usize) -> Option<Bar> {
        self.raw.get(i)
    }

    /// How many levels the pyramid has above the raw bars.
    pub fn depth(&self) -> usize {
        self.levels.len()
    }

    /// Append one bar and revise the pyramid.
    ///
    /// O(depth) — the tail entry of each level is recomputed from the level
    /// below, because appending finishes neither the group it lands in nor any
    /// group above it.
    pub fn push(&mut self, bar: Bar) {
        let i = self.raw.len();
        self.time.push(bar.t);
        let bar = Bar {
            t: self.time.at(i),
            ..bar
        };
        self.raw.set(i, bar);

        let mut level = 0usize;
        loop {
            let below_len = self.below_len(level);
            // A level is worth having only once the level below overflows one
            // group. This also terminates the walk: each level is about
            // 1/FANOUT of the one beneath it.
            if below_len <= FANOUT {
                break;
            }
            if self.levels.len() == level {
                // First push that warrants this level: build it whole. Writing
                // only the tail would leave every earlier group empty, because
                // the level below already holds bars pushed before this level
                // existed. That was a real bug, caught by
                // `a_level_equals_the_direct_fold`.
                let built = self.build_level(level);
                self.levels.push(built);
            } else {
                // Steady state: a new bar can only revise the last group.
                let idx = (below_len - 1) / FANOUT;
                let from = idx * FANOUT;
                let to = below_len.min(from + FANOUT);
                match self.fold_below(level, from, to) {
                    Some(folded) => self.levels[level].set(idx, folded),
                    None => break,
                }
            }
            level += 1;
        }
    }

    /// Entries in the level beneath `level`.
    fn below_len(&self, level: usize) -> usize {
        match level {
            0 => self.raw.len(),
            k => self.levels.get(k - 1).map_or(0, Cols::len),
        }
    }

    /// Fold the whole level beneath `level` into groups of [`FANOUT`].
    fn build_level(&self, level: usize) -> Cols {
        let src_len = self.below_len(level);
        let mut out = Cols::default();
        let mut from = 0usize;
        while from < src_len {
            let to = src_len.min(from + FANOUT);
            match self.fold_below(level, from, to) {
                Some(b) => {
                    let at = out.len();
                    out.set(at, b);
                }
                None => break,
            }
            from += FANOUT;
        }
        out
    }

    /// Fold `[from, to)` of the level beneath `level` into one bar.
    fn fold_below(&self, level: usize, from: usize, to: usize) -> Option<Bar> {
        let src = if level == 0 {
            &self.raw
        } else {
            self.levels.get(level - 1)?
        };
        let mut acc = src.get(from)?;
        for i in (from + 1)..to {
            match src.get(i) {
                Some(b) => acc = acc.merge(b),
                None => break,
            }
        }
        Some(acc)
    }

    /// Raw bars covered by one entry of `level` (`None` = raw itself).
    pub fn span_of(level: Option<usize>) -> usize {
        match level {
            None => 1,
            Some(k) => FANOUT.pow(k as u32 + 1),
        }
    }

    /// The level to draw when one pixel column covers `per_col` raw bars.
    ///
    /// Picks the coarsest level whose entries still fit inside a column, so a
    /// column is assembled from at most [`FANOUT`] reads. `None` means draw the
    /// raw bars.
    pub fn level_for(&self, per_col: f64) -> Option<usize> {
        // NaN answers `false` to every comparison, so it is ruled out before the
        // ordering test rather than by it: a NaN zoom must fall back to the raw
        // bars, not to whichever branch the comparison happens to take.
        if per_col.is_nan() || per_col <= FANOUT as f64 {
            return None;
        }
        let mut best = None;
        for k in 0..self.levels.len() {
            if (Self::span_of(Some(k)) as f64) <= per_col {
                best = Some(k);
            } else {
                break;
            }
        }
        best
    }

    /// Aggregate `[from, to)` raw bars into one bar, via the pyramid.
    ///
    /// The drawing entry point: one call per pixel column. Reads at most
    /// [`FANOUT`] entries when `level_for` chose the level, which is what
    /// bounds the per-frame work by pane width rather than by dataset size.
    pub fn column(&self, from: usize, to: usize) -> Option<Bar> {
        let to = to.min(self.raw.len());
        if from >= to {
            return None;
        }
        let mut acc: Option<Bar> = None;
        self.walk(from, to, |piece| {
            acc = Some(match acc {
                Some(a) => a.merge(piece),
                None => piece,
            });
        });
        acc
    }

    /// Decompose `[from, to)` into the fewest pyramid entries that tile it, and
    /// hand each to `f` in time order.
    ///
    /// The greedy segment-tree walk: at each position take the **largest**
    /// level whose entries align with it and still fit in what is left, else
    /// step one raw bar. Spans are powers of [`FANOUT`], so once alignment or
    /// room fails for one level it fails for every larger one — the search per
    /// step is therefore a short scan, not a search.
    ///
    /// This replaced a version that took whole entries from a single chosen
    /// level and folded the two ragged ends from **raw** bars. That was
    /// correct but its cost was bounded by the level's span rather than by
    /// `FANOUT`: a column of 3153 bars over a 512-span level read up to ~1000
    /// raw bars, against the ~8 the design claims. Here every piece of the
    /// decomposition is an aggregate, so a column costs at most `FANOUT` steps
    /// per level, i.e. `O(FANOUT * depth)` — bounded by the *depth* of the
    /// pyramid and so, in effect, by the log of the dataset size.
    fn walk(&self, from: usize, to: usize, mut f: impl FnMut(Bar)) {
        let mut i = from;
        while i < to {
            let mut chosen: Option<(usize, usize)> = None; // (level, span)
            for k in 0..self.levels.len() {
                let span = Self::span_of(Some(k));
                // Both conditions are monotone in `k`, so the first failure
                // ends the scan.
                if span > to - i || !i.is_multiple_of(span) {
                    break;
                }
                if self.levels[k].len() > i / span {
                    chosen = Some((k, span));
                }
            }
            match chosen {
                Some((k, span)) => match self.levels[k].get(i / span) {
                    Some(b) => {
                        f(b);
                        i += span;
                    }
                    None => break,
                },
                None => match self.raw.get(i) {
                    Some(b) => {
                        f(b);
                        i += 1;
                    }
                    None => break,
                },
            }
        }
    }

    /// How many pyramid entries `[from, to)` decomposes into.
    ///
    /// The cost of drawing one column, made observable so the bound can be
    /// asserted rather than asserted *about*.
    pub fn pieces(&self, from: usize, to: usize) -> usize {
        let to = to.min(self.raw.len());
        let mut n = 0;
        self.walk(from, to, |_| n += 1);
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synth(n: usize) -> Vec<Bar> {
        // Deterministic and non-monotonic, so max/min actually have to work.
        (0..n)
            .map(|i| {
                let x = (i as f64 * 0.7).sin() * 10.0 + i as f64 * 0.01;
                Bar::new(
                    i as i64 * 5_000,
                    x,
                    x + 2.0 + (i % 7) as f64,
                    x - 1.5 - (i % 5) as f64,
                    x + 0.5,
                    (i % 13) as f64,
                )
            })
            .collect()
    }

    fn direct_fold(bars: &[Bar]) -> Bar {
        let mut acc = bars[0];
        for b in &bars[1..] {
            acc = acc.merge(*b);
        }
        acc
    }

    fn series(n: usize) -> Series {
        Series::from_bars(Time::Regular { t0: 0, step: 5_000 }, &synth(n))
    }

    /// The claim the whole design rests on: a pyramid entry is not an
    /// approximation, it is exactly the fold of the raw bars beneath it.
    #[test]
    fn a_level_equals_the_direct_fold() {
        let bars = synth(5_000);
        let s = series(5_000);
        assert!(s.depth() >= 3, "expected several levels, got {}", s.depth());

        for k in 0..s.depth() {
            let span = Series::span_of(Some(k));
            let entries = s.levels[k].len();
            assert!(entries > 0);
            // Every WHOLE entry must match; the last one may still be filling.
            for i in 0..entries.saturating_sub(1) {
                let got = s.levels[k].get(i).expect("entry");
                let want = direct_fold(&bars[i * span..(i + 1) * span]);
                assert_eq!(got, want, "level {k} entry {i}");
            }
        }
    }

    /// Appending must leave the same pyramid as building in one go, or a live
    /// chart slowly diverges from a reloaded one.
    #[test]
    fn appending_matches_building_at_once() {
        let bars = synth(1_000);
        let at_once = Series::from_bars(Time::Regular { t0: 0, step: 5_000 }, &bars);

        let mut incremental = Series::regular(0, 5_000);
        for b in &bars {
            incremental.push(*b);
        }
        assert_eq!(incremental.depth(), at_once.depth());
        assert_eq!(incremental.levels, at_once.levels);
        assert_eq!(incremental.raw, at_once.raw);
    }

    /// A drawn column is the fold of exactly the bars it spans — including the
    /// ragged ends that do not line up with an aggregated entry.
    #[test]
    fn a_column_covers_exactly_its_span() {
        let bars = synth(4_000);
        let s = series(4_000);
        // Deliberately misaligned starts, so the ragged-end path is taken.
        for (from, to) in [(0, 64), (3, 517), (1_001, 3_999), (7, 8), (100, 1_700)] {
            let got = s.column(from, to).expect("column");
            let want = direct_fold(&bars[from..to]);
            assert_eq!(got, want, "column {from}..{to}");
        }
    }

    /// The bound that makes the whole thing survivable, asserted directly on
    /// the cost of drawing a column rather than on a proxy for it.
    ///
    /// The bound is derived, not measured: a greedy decomposition of an
    /// arbitrary range over a `FANOUT`-ary tree takes at most `FANOUT - 1`
    /// pieces on the way up and as many on the way down, per level. So
    /// `2 * (FANOUT - 1) * (depth + 1)` — for a 200k series that is 84, and the
    /// walk uses 62. The ragged-end version it replaced used ~1000.
    ///
    /// The misaligned spans below are the point: they are what a real viewport
    /// produces, and exactly what the old version was slow on.
    #[test]
    fn a_column_costs_a_bounded_number_of_reads() {
        let s = series(200_000);
        let bound = 2 * (FANOUT - 1) * (s.depth() + 1);
        for (from, to) in [
            (0, 10_000),
            (1, 10_001),
            (777, 100_777),
            (3, 199_999),
            (0, 200_000),
            (12_345, 54_321),
        ] {
            let n = s.pieces(from, to);
            assert!(
                n <= bound,
                "column {from}..{to} took {n} reads, bound {bound} (depth {})",
                s.depth()
            );
        }
        // And the whole point: it does not grow with the dataset.
        let big = series(1_000_000);
        let n = big.pieces(7, 900_007);
        assert!(
            n <= 2 * (FANOUT - 1) * (big.depth() + 1),
            "1M series took {n} reads at depth {}",
            big.depth()
        );
        // Five times the data must not cost five times the reads.
        assert!(
            n < 4 * s.pieces(3, 199_999),
            "reads grew with the dataset: {n} vs {}",
            s.pieces(3, 199_999)
        );
    }

    /// The memory claim: the pyramid costs about 1/(FANOUT-1) of the base.
    #[test]
    fn the_pyramid_is_a_small_fraction_of_the_base() {
        let s = series(100_000);
        let extra: usize = s.levels.iter().map(|c| c.len()).sum();
        let frac = extra as f64 / s.len() as f64;
        assert!(
            frac < 0.20,
            "pyramid overhead {frac:.3} of base, expected ~1/7"
        );
    }

    /// A short series has no pyramid at all, and must still draw.
    #[test]
    fn a_short_series_needs_no_levels() {
        let s = series(5);
        assert_eq!(s.depth(), 0);
        assert_eq!(s.level_for(100.0), None);
        let got = s.column(0, 5).expect("column");
        assert_eq!(got, direct_fold(&synth(5)));
        // And an empty one produces nothing rather than panicking.
        let e = Series::regular(0, 1_000);
        assert!(e.column(0, 10).is_none());
        assert!(e.is_empty());
    }

    /// Out-of-range asks are clamped, not panics: they come from view maths.
    #[test]
    fn out_of_range_is_clamped() {
        let s = series(100);
        assert_eq!(s.column(90, 10_000), s.column(90, 100));
        assert!(s.column(200, 300).is_none());
        assert!(s.bar(1_000).is_none());
    }

    #[test]
    fn regular_time_needs_no_array() {
        let s = series(1_000);
        assert_eq!(s.time().at(0), 0);
        assert_eq!(s.time().at(200), 200 * 5_000);
        assert!(matches!(s.time(), Time::Regular { .. }));
    }
}
