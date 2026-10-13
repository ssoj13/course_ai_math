//! The generator: a [`Pattern`] rasterised into one seamless, antialiased tile. No egui here, so
//! the same tiles can feed any renderer.
//!
//! **Seamless by construction.** Every primitive is periodic along x and y with a period that is a
//! whole number of pixels (its period in points times the pixels per point, rounded), and the tile
//! is the least common multiple of those periods on each axis. Repeating the tile therefore repeats
//! the pattern exactly. That is why a line family only comes in the four directions a pixel grid
//! repeats: horizontal, vertical and the two diagonals.
//!
//! **Periods are measured along the axes**, the way a hatch reads on screen (a diagonal every 6
//! points along a bar); a line's `width` is its thickness across the line.

/// The direction of a line family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dir {
    /// `-`: lines along x.
    Horizontal,
    /// `|`: lines along y.
    Vertical,
    /// `/`: rising to the right on screen (y points down: `x + y` is constant along a line).
    Rising,
    /// `\`: falling to the right on screen (`x - y` is constant along a line).
    Falling,
}

/// One periodic primitive of a [`Pattern`]; lengths in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Prim {
    /// Parallel lines `period` apart along the axes, `width` thick.
    Lines { dir: Dir, period: f32, width: f32 },
    /// A grid of discs `period` apart, `radius` in size; `stagger` shifts every other row by half a
    /// period.
    Dots {
        period: f32,
        radius: f32,
        stagger: bool,
    },
    /// Squares of `size`, alternating.
    Checker { size: f32 },
}

/// How many primitives a [`Pattern`] unites.
pub const MAX_PRIMS: usize = 3;

/// A pattern: the union of up to [`MAX_PRIMS`] primitives. Plain data (`Copy`), so a widget model
/// can carry one per item.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pattern {
    prims: [Option<Prim>; MAX_PRIMS],
}

impl Pattern {
    /// A pattern of one primitive.
    pub const fn new(prim: Prim) -> Self {
        Self {
            prims: [Some(prim), None, None],
        }
    }

    /// The pattern with `prim` added; `None` when it already holds [`MAX_PRIMS`].
    pub fn with(mut self, prim: Prim) -> Option<Self> {
        let free = self.prims.iter_mut().find(|slot| slot.is_none())?;
        *free = Some(prim);
        Some(self)
    }

    /// Its primitives.
    pub fn prims(&self) -> impl Iterator<Item = &Prim> {
        self.prims.iter().flatten()
    }

    /// `/` lines every 6 points, 1 point thick: the NLE "offline / file source" look.
    pub const fn hatch() -> Self {
        Self::new(Prim::Lines {
            dir: Dir::Rising,
            period: 6.0,
            width: 1.0,
        })
    }

    /// `\` lines every 6 points, 1 point thick.
    pub const fn back_hatch() -> Self {
        Self::new(Prim::Lines {
            dir: Dir::Falling,
            period: 6.0,
            width: 1.0,
        })
    }

    /// `/` and `\` lines every 8 points.
    pub const fn cross_hatch() -> Self {
        Self {
            prims: [thin(Dir::Rising, 8.0), thin(Dir::Falling, 8.0), None],
        }
    }

    /// Horizontal and vertical lines every 8 points.
    pub const fn grid() -> Self {
        Self {
            prims: [thin(Dir::Horizontal, 8.0), thin(Dir::Vertical, 8.0), None],
        }
    }

    /// Horizontal stripes 2 points thick every 6 points.
    pub const fn stripes() -> Self {
        Self::new(Prim::Lines {
            dir: Dir::Horizontal,
            period: 6.0,
            width: 2.0,
        })
    }

    /// Staggered dots of radius 1 every 6 points.
    pub const fn dots() -> Self {
        Self::new(Prim::Dots {
            period: 6.0,
            radius: 1.0,
            stagger: true,
        })
    }

    /// Alternating squares of 4 points.
    pub const fn checker() -> Self {
        Self::new(Prim::Checker { size: 4.0 })
    }
}

/// A 1-point line family `period` points apart (the presets' building block).
const fn thin(dir: Dir, period: f32) -> Option<Prim> {
    Some(Prim::Lines {
        dir,
        period,
        width: 1.0,
    })
}

/// A rasterised pattern: `width x height` coverages, rows top first, 0 (empty) to 255 (covered).
#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    pub width: usize,
    pub height: usize,
    pub coverage: Vec<u8>,
}

/// Why a pattern has no tile.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TileError {
    /// It has no primitive.
    Empty,
    /// Its periods do not repeat within [`MAX_TILE`] pixels on an axis (periods with a large least
    /// common multiple).
    TooLarge { width: usize, height: usize },
    /// A length is not finite and positive, or `pixels_per_point` is not.
    BadLength,
}

/// The largest tile side, in pixels.
pub const MAX_TILE: usize = 1024;

/// Subsamples per pixel side. 16, not 4: a regular subsample grid is aligned with the diagonals'
/// own lattice (the subsamples' `x + y` step by `1 / SUB`), so a 4 x 4 grid thinned a 1-point hatch
/// to 5/24 of its period instead of the geometric `sqrt 2 / 6` (0.208 against 0.236). At 16 the
/// error is under 0.01; tiles are a few dozen pixels, rasterised once per scale.
const SUB: usize = 16;

/// A primitive in pixels: its periods rounded to whole pixels, its other lengths scaled.
struct PxPrim {
    prim: Prim,
    /// The period on each axis, in whole pixels.
    period: [usize; 2],
    /// The primitive's lengths scaled to pixels (`width` / `radius` / checker size).
    size: f32,
    /// The pixel period its geometry repeats with (the rounded `period` / `size`).
    step: f32,
}

impl PxPrim {
    fn new(prim: Prim, ppp: f32) -> Result<Self, TileError> {
        let whole = |points: f32| -> Result<usize, TileError> {
            let pixels = points * ppp;
            if !pixels.is_finite() || pixels <= 0.0 {
                return Err(TileError::BadLength);
            }
            Ok((pixels.round() as usize).max(1))
        };
        let scaled = |points: f32| -> Result<f32, TileError> {
            let pixels = points * ppp;
            if !pixels.is_finite() || pixels <= 0.0 {
                return Err(TileError::BadLength);
            }
            Ok(pixels)
        };
        Ok(match prim {
            Prim::Lines { dir, period, width } => {
                let p = whole(period)?;
                let period = match dir {
                    Dir::Horizontal => [1, p],
                    Dir::Vertical => [p, 1],
                    Dir::Rising | Dir::Falling => [p, p],
                };
                Self {
                    prim,
                    period,
                    size: scaled(width)?,
                    step: p as f32,
                }
            }
            Prim::Dots {
                period,
                radius,
                stagger,
            } => {
                let p = whole(period)?;
                Self {
                    prim,
                    period: [p, if stagger { 2 * p } else { p }],
                    size: scaled(radius)?,
                    step: p as f32,
                }
            }
            Prim::Checker { size } => {
                let s = whole(size)?;
                Self {
                    prim,
                    period: [2 * s, 2 * s],
                    size: s as f32,
                    step: s as f32,
                }
            }
        })
    }

    /// Whether the point `(x, y)` (pixels from the pattern's origin) is covered.
    fn covers(&self, x: f32, y: f32) -> bool {
        let p = self.step;
        match self.prim {
            Prim::Lines { dir, .. } => {
                let half = self.size * 0.5;
                match dir {
                    Dir::Horizontal => centered(y, p).abs() <= half,
                    Dir::Vertical => centered(x, p).abs() <= half,
                    Dir::Rising => {
                        centered(x + y, p).abs() * std::f32::consts::FRAC_1_SQRT_2 <= half
                    }
                    Dir::Falling => {
                        centered(x - y, p).abs() * std::f32::consts::FRAC_1_SQRT_2 <= half
                    }
                }
            }
            Prim::Dots { stagger, .. } => {
                let row = (y / p).round() as i64;
                let shift = if stagger && row.rem_euclid(2) == 1 {
                    p * 0.5
                } else {
                    0.0
                };
                let dx = centered(x - shift, p);
                let dy = centered(y, p);
                dx * dx + dy * dy <= self.size * self.size
            }
            Prim::Checker { .. } => {
                ((x / p).floor() as i64 + (y / p).floor() as i64).rem_euclid(2) == 0
            }
        }
    }
}

/// `v` wrapped to the nearest multiple of `period`: its signed distance from it, in
/// `[-period / 2, period / 2]`.
fn centered(v: f32, period: f32) -> f32 {
    v - period * (v / period).round()
}

fn gcd(a: usize, b: usize) -> usize {
    if b == 0 { a } else { gcd(b, a % b) }
}

fn lcm(a: usize, b: usize) -> usize {
    a / gcd(a, b) * b
}

/// The coverage (0-255) of the pixel `(i, j)` of `prims`, from its `SUB x SUB` subsamples: the
/// fraction of them inside any primitive.
fn coverage(prims: &[PxPrim], i: usize, j: usize) -> u8 {
    let mut inside = 0usize;
    for b in 0..SUB {
        let y = j as f32 + (b as f32 + 0.5) / SUB as f32;
        for a in 0..SUB {
            let x = i as f32 + (a as f32 + 0.5) / SUB as f32;
            if prims.iter().any(|prim| prim.covers(x, y)) {
                inside += 1;
            }
        }
    }
    ((inside * 255 + SUB * SUB / 2) / (SUB * SUB)) as u8
}

/// `pattern` rasterised for a display of `pixels_per_point`: one seamless tile.
pub fn rasterize(pattern: &Pattern, pixels_per_point: f32) -> Result<Tile, TileError> {
    let prims = pattern
        .prims()
        .map(|prim| PxPrim::new(*prim, pixels_per_point))
        .collect::<Result<Vec<_>, _>>()?;
    if prims.is_empty() {
        return Err(TileError::Empty);
    }
    let [width, height] = prims.iter().fold([1, 1], |[w, h], prim| {
        [lcm(w, prim.period[0]), lcm(h, prim.period[1])]
    });
    if width > MAX_TILE || height > MAX_TILE {
        return Err(TileError::TooLarge { width, height });
    }
    let coverage = (0..height)
        .flat_map(|j| (0..width).map(move |i| (i, j)))
        .map(|(i, j)| coverage(&prims, i, j))
        .collect();
    Ok(Tile {
        width,
        height,
        coverage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRESETS: [(&str, Pattern); 7] = [
        ("hatch", Pattern::hatch()),
        ("back_hatch", Pattern::back_hatch()),
        ("cross_hatch", Pattern::cross_hatch()),
        ("grid", Pattern::grid()),
        ("stripes", Pattern::stripes()),
        ("dots", Pattern::dots()),
        ("checker", Pattern::checker()),
    ];

    /// The display scales a desktop meets: 100 %, 125 %, 150 %, 200 %.
    const SCALES: [f32; 4] = [1.0, 1.25, 1.5, 2.0];

    /// Repeating the tile repeats the pattern: the pattern rasterised one tile further along x, along
    /// y and along both is the tile itself. The oracle is the coverage function evaluated at the
    /// shifted pixels, independent of the tile's storage.
    #[test]
    fn every_preset_tiles_seamlessly_at_every_scale() {
        for (name, pattern) in PRESETS {
            for ppp in SCALES {
                let tile = rasterize(&pattern, ppp).expect("a tile");
                let prims: Vec<_> = pattern
                    .prims()
                    .map(|prim| PxPrim::new(*prim, ppp).expect("valid"))
                    .collect();
                for j in 0..tile.height {
                    for i in 0..tile.width {
                        let at = tile.coverage[j * tile.width + i];
                        for (di, dj) in
                            [(tile.width, 0), (0, tile.height), (tile.width, tile.height)]
                        {
                            assert_eq!(
                                coverage(&prims, i + di, j + dj),
                                at,
                                "{name} at {ppp}: pixel ({i}, {j}) differs one tile on ({di}, {dj})"
                            );
                        }
                    }
                }
            }
        }
    }

    /// The share of the plane a pattern covers, from geometry alone: a line family `w / (p / sqrt 2)`
    /// for a diagonal (lines `p / sqrt 2` apart across), `w / p` for an axis one; dots `pi r^2 / p^2`;
    /// a checker one half. The tile's mean coverage must match it.
    #[test]
    fn a_tile_covers_the_share_its_geometry_says() {
        let mean = |pattern: Pattern| {
            let tile = rasterize(&pattern, 1.0).expect("a tile");
            tile.coverage.iter().map(|c| f64::from(*c)).sum::<f64>()
                / (255.0 * tile.coverage.len() as f64)
        };
        let cases = [
            ("hatch", Pattern::hatch(), 1.0 / (6.0 / 2f64.sqrt())),
            ("stripes", Pattern::stripes(), 2.0 / 6.0),
            ("dots", Pattern::dots(), std::f64::consts::PI / 36.0),
            ("checker", Pattern::checker(), 0.5),
        ];
        for (name, pattern, share) in cases {
            let got = mean(pattern);
            assert!(
                (got - share).abs() < 0.02,
                "{name}: covers {got:.4}, geometry {share:.4}"
            );
        }
    }

    #[test]
    fn periods_are_whole_pixels_and_their_common_multiple() {
        // 6 points at 125 % is 7.5 pixels: rounded to 8, and the diagonal repeats on both axes.
        let tile = rasterize(&Pattern::hatch(), 1.25).expect("a tile");
        assert_eq!((tile.width, tile.height), (8, 8));
        // Horizontal stripes repeat along x at every pixel.
        let tile = rasterize(&Pattern::stripes(), 1.0).expect("a tile");
        assert_eq!((tile.width, tile.height), (1, 6));
        // Staggered dots repeat after two rows.
        let tile = rasterize(&Pattern::dots(), 1.0).expect("a tile");
        assert_eq!((tile.width, tile.height), (6, 12));
    }

    #[test]
    fn invalid_patterns_are_refused() {
        assert_eq!(
            rasterize(
                &Pattern {
                    prims: [None; MAX_PRIMS]
                },
                1.0
            ),
            Err(TileError::Empty)
        );
        let zero = Pattern::new(Prim::Checker { size: 0.0 });
        assert_eq!(rasterize(&zero, 1.0), Err(TileError::BadLength));
        assert_eq!(
            rasterize(&Pattern::hatch(), f32::NAN),
            Err(TileError::BadLength)
        );
        // Coprime periods near the limit do not repeat within MAX_TILE.
        let wide = Pattern::new(Prim::Lines {
            dir: Dir::Vertical,
            period: 1021.0,
            width: 1.0,
        })
        .with(Prim::Lines {
            dir: Dir::Vertical,
            period: 1019.0,
            width: 1.0,
        })
        .expect("room for two");
        assert!(matches!(
            rasterize(&wide, 1.0),
            Err(TileError::TooLarge { .. })
        ));
        assert!(
            Pattern::cross_hatch()
                .with(Prim::Checker { size: 4.0 })
                .is_some()
        );
        let full = Pattern::cross_hatch()
            .with(Prim::Checker { size: 4.0 })
            .expect("third slot");
        assert!(
            full.with(Prim::Checker { size: 2.0 }).is_none(),
            "three at most"
        );
    }
}
