//! Texture-space filter footprint math: the ONE port of OpenImageIO's anisotropic MIP filter.
//!
//! Everything here is a pure function of derivatives, options and MIP-chain dimensions. Nothing
//! fetches a texel: callers (vfx-io's `TextureSystem`, a warp's EWA mode, a GPU kernel's setup)
//! ask *which* levels, *where* along the major axis and *how much* each probe weighs, then read
//! their own pixels. The reference is `OpenImageIO/src/libtexture/texturesys.cpp` (3.1) plus
//! `texture_pvt.h`; each function cites the lines it ports.
//!
//! A lookup in OIIO's default (`MipMode::Default`/`Aniso`) mode is, in order
//! (`texture_lookup`, `texturesys.cpp:2261-2434`):
//!
//! 1. [`adjust_width`] - scale the derivatives by `swidth`/`twidth`, replace degenerate ones.
//! 2. [`ellipse_axes`] - the footprint ellipse's axis lengths and orientation.
//! 3. [`adjust_blur`] - fold `sblur`/`tblur` into those axes.
//! 4. [`anisotropic_aspect`] - clamp the aspect ratio to `anisotropic`.
//! 5. [`compute_miplevels`] - the one or two MIP levels and their blend weights.
//! 6. [`compute_ellipse_sampling`] - probe positions and weights along the major axis.
//!
//! then, per level, one interpolated read ([`st_to_texel`] + [`Wrap::wrap_texel`] +
//! [`bspline_weights`] or bilinear) per probe. The isotropic `OneLevel`/`Trilinear` modes use
//! [`isotropic_filter_width`] instead of steps 2-4 and 6 (`texturesys.cpp:2064-2078`).

use crate::st::st_to_px;

// ============================================================================
// Wrap modes
// ============================================================================

/// How texel coordinates outside the image are handled (OIIO `Tex::Wrap`, `texture.h`).
///
/// The discriminants are OIIO's enum order; `wrap_functions[]` in `texturesys.cpp:580-589` is
/// indexed by them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
#[repr(u8)]
pub enum Wrap {
    /// Use the mode the texture file declares (resolve with [`Wrap::resolve`]).
    #[default]
    Default,
    /// Black outside the image.
    Black,
    /// Clamp to the edge texel.
    Clamp,
    /// Periodic (tile).
    Periodic,
    /// Mirror at each edge.
    Mirror,
    /// Periodic, image size known to be a power of two (same result as [`Wrap::Periodic`]).
    PeriodicPow2,
    /// Periodic where the first and last column are the same texel (lat-long environments).
    PeriodicSharedBorder,
}

impl Wrap {
    /// Decodes a wrap mode name (OIIO `Tex::decode_wrapmode`). Unknown names are `Default`.
    pub fn decode(name: &str) -> Self {
        match name.to_lowercase().as_str() {
            "default" => Wrap::Default,
            "black" => Wrap::Black,
            "clamp" => Wrap::Clamp,
            "periodic" | "repeat" => Wrap::Periodic,
            "mirror" => Wrap::Mirror,
            "periodicpow2" => Wrap::PeriodicPow2,
            "periodicsharedborder" => Wrap::PeriodicSharedBorder,
            _ => Wrap::Default,
        }
    }

    /// Resolves `Default` against the wrap mode the texture file itself declares.
    ///
    /// OIIO resolves `WrapDefault` to the file's own mode at lookup time
    /// (`texturesys.cpp:1616-1621`); a file that declares no `wrapmodes` initializes to
    /// **Black** (`imagecache.cpp:361-362`), not Periodic.
    pub fn resolve(self, file_default: Wrap) -> Wrap {
        match self {
            Wrap::Default => match file_default {
                Wrap::Default => Wrap::Black,
                resolved => resolved,
            },
            explicit => explicit,
        }
    }

    /// Parses a `"swrap,twrap"` pair (OIIO `Tex::parse_wrapmodes`); one name applies to both.
    pub fn parse_pair(wrapmodes: &str) -> (Wrap, Wrap) {
        let mut parts = wrapmodes.split(',');
        let swrap = Wrap::decode(parts.next().unwrap_or("default"));
        let twrap = parts.next().map(Wrap::decode).unwrap_or(swrap);
        (swrap, twrap)
    }

    /// Wraps texel coordinate `coord` into `[origin, origin + width)`.
    ///
    /// Returns `None` when the texel is outside a [`Wrap::Black`] (or unresolved `Default`)
    /// image, i.e. the texel contributes black. Ports the `wrap_*_simd` table
    /// (`texturesys.cpp:595-678`) and `wrap_periodic_sharedborder` (`:565-577`).
    pub fn wrap_texel(self, coord: i32, origin: i32, width: i32) -> Option<i32> {
        match self {
            Wrap::Default | Wrap::Black => {
                (coord >= origin && coord < origin + width).then_some(coord)
            }
            Wrap::Clamp => {
                let c = if coord < origin { origin } else { coord };
                Some(if c >= origin + width {
                    origin + width - 1
                } else {
                    c
                })
            }
            Wrap::Periodic => {
                let c = (coord - origin) % width;
                Some(if c < 0 { c + width } else { c } + origin)
            }
            Wrap::PeriodicPow2 => Some(((coord - origin) & (width - 1)) + origin),
            Wrap::Mirror => {
                // Negatives reflect about -1/2, so -1 maps to texel 0 (`:648`).
                let c = coord - origin;
                let c = if c < 0 { -1 - c } else { c };
                let iter = c / width;
                let c = c - iter * width;
                Some(if iter & 1 != 0 { width - 1 - c } else { c } + origin)
            }
            Wrap::PeriodicSharedBorder => {
                // The first and last column are one texel, so the period is width-1.
                let w = width.max(2);
                let c = (coord - origin) % (w - 1);
                Some(if c < 0 { c + w } else { c } + origin)
            }
        }
    }
}

// ============================================================================
// Derivatives and the footprint ellipse
// ============================================================================

/// Screen-space derivatives of the texture coordinates (pixel-to-pixel, not half-pixel).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Derivs {
    /// ds/dx.
    pub dsdx: f32,
    /// dt/dx.
    pub dtdx: f32,
    /// ds/dy.
    pub dsdy: f32,
    /// dt/dy.
    pub dtdy: f32,
}

impl Derivs {
    /// Bundles the four derivatives.
    pub const fn new(dsdx: f32, dtdx: f32, dsdy: f32, dtdy: f32) -> Self {
        Self {
            dsdx,
            dtdx,
            dsdy,
            dtdy,
        }
    }
}

/// Scales the derivatives by `swidth`/`twidth` and replaces degenerate ones.
///
/// Ports `adjust_width` (`texturesys.cpp:1836-1888`): a derivative vector shorter than `1e-8`
/// is replaced by one of that length orthogonal to the other, or both by `(1e-8, 0, 0, 1e-8)`,
/// so the ellipse maths below never divides by zero. Blur is deliberately NOT added here; the
/// reference keeps its derivative-blur code disabled because it "is wrong" (`:1845-1857`).
pub fn adjust_width(d: Derivs, swidth: f32, twidth: f32) -> Derivs {
    let (mut dsdx, mut dtdx, mut dsdy, mut dtdy) = (
        d.dsdx * swidth,
        d.dtdx * twidth,
        d.dsdy * swidth,
        d.dtdy * twidth,
    );
    const EPS: f32 = 1.0e-8;
    const EPS2: f32 = EPS * EPS;
    let dxlen2 = dsdx * dsdx + dtdx * dtdx;
    let dylen2 = dsdy * dsdy + dtdy * dtdy;
    if dxlen2 < EPS2 {
        if dylen2 < EPS2 {
            // Essentially point sampling: a tiny but finite filter.
            dsdx = EPS;
            dsdy = 0.0;
            dtdx = 0.0;
            dtdy = EPS;
        } else {
            let scale = EPS / dylen2.sqrt();
            dsdx = dtdy * scale;
            dtdx = -dsdy * scale;
        }
    } else if dylen2 < EPS2 {
        let scale = EPS / dxlen2.sqrt();
        dsdy = -dtdx * scale;
        dtdy = dsdx * scale;
    }
    Derivs {
        dsdx,
        dtdx,
        dsdy,
        dtdy,
    }
}

/// A footprint ellipse in texture space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ellipse {
    /// Major axis length. For pixel-to-pixel derivatives this is the full diameter-scale value
    /// OIIO works in; see the note on [`ellipse_axes`].
    pub major: f32,
    /// Minor axis length, `<= major`.
    pub minor: f32,
    /// Orientation of the major axis, radians from +s.
    pub theta: f32,
}

/// The implicit form of the footprint ellipse, `A u^2 + B u v + C v^2 < 1` after division by F.
///
/// `ellipse_axes`' optional `ABCF` output (`texturesys.cpp:2172-2181`): what a true EWA filter
/// evaluates per texel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EllipseCoeffs {
    /// A / F.
    pub a: f64,
    /// B / F.
    pub b: f64,
    /// C / F.
    pub c: f64,
    /// F = A C - B^2 / 4 (before normalization).
    pub f: f64,
}

/// The unnormalized quadratic-form coefficients shared by [`ellipse_axes`] and
/// [`ellipse_coeffs`] (`texturesys.cpp:2152-2158`). Sums are formed in f32 as the reference
/// does, then widened.
fn quadratic_form(d: Derivs) -> (f64, f64, f64) {
    let a = (d.dtdx * d.dtdx + d.dtdy * d.dtdy) as f64;
    let b = -2.0 * (d.dsdx * d.dtdx + d.dsdy * d.dtdy) as f64;
    let c = (d.dsdx * d.dsdx + d.dsdy * d.dsdy) as f64;
    (a, b, c)
}

/// Axis lengths and orientation of the footprint ellipse (Greene's EWA, Mavridis 2011).
///
/// Ports `ellipse_axes` (`texturesys.cpp:2148-2189`), including the 1000 cap on each axis and
/// `fast_atan2` for the angle. With pixel-to-pixel derivatives (what every caller here passes)
/// the lengths are the ellipse's diameters in OIIO's terms; [`compute_ellipse_sampling`]
/// accounts for the resulting factor of two exactly as `texture_lookup` does (`:2320-2325`).
pub fn ellipse_axes(d: Derivs) -> Ellipse {
    let (a, b, c) = quadratic_form(d);
    let root = (a - c).hypot(b);
    let a_prime = (a + c - root) * 0.5;
    let c_prime = (a + c + root) * 0.5;
    let major = safe_sqrt(c_prime as f32).min(1000.0);
    let minor = safe_sqrt(a_prime as f32).min(1000.0);
    let theta = fast_atan2(b as f32, (a - c) as f32) * 0.5 + std::f32::consts::FRAC_PI_2;
    Ellipse {
        major,
        minor,
        theta,
    }
}

/// The normalized implicit-form coefficients of the footprint ellipse (`texturesys.cpp:2172-2181`).
pub fn ellipse_coeffs(d: Derivs) -> EllipseCoeffs {
    let (a, b, c) = quadratic_form(d);
    let f = a * c - b * b * 0.25;
    // The reference computes `1.0f / F` - a float literal divided by a double.
    let finv = 1.0 / f;
    EllipseCoeffs {
        a: a * finv,
        b: b * finv,
        c: c * finv,
        f,
    }
}

/// Folds `sblur`/`tblur` into the ellipse axes.
///
/// Ports `adjust_blur` (`texturesys.cpp:1900-1950`). `legacy` selects the simple additive form,
/// which OIIO 3.1 uses by default (`m_legacy_texture_blur = true` below 3.2,
/// `texture_pvt.h:470-476`); the non-legacy form adds the blur in quadrature. If blur makes the
/// minor axis the longer one the axes are swapped and `theta` turned by 90 degrees (`:1929-1948`).
///
/// The angle goes through [`fast_sincos`] as in the reference: at axis-aligned footprints its
/// cosine of pi/2 differs from `f32::cos`'s -4.4e-8 by enough to move the minor axis across an
/// integer in `2 * aspect - 1`, i.e. to change the probe count (seen against OIIO 3.1 with
/// `sblur = 0.15`).
pub fn adjust_blur(e: Ellipse, sblur: f32, tblur: f32, legacy: bool) -> Ellipse {
    if sblur + tblur == 0.0 {
        return e;
    }
    let Ellipse {
        mut major,
        mut minor,
        mut theta,
    } = e;
    let (sin, cos) = fast_sincos(theta);
    let (sin, cos) = (sin.abs(), cos.abs());
    if legacy {
        major += sblur * cos + tblur * sin;
        minor += sblur * sin + tblur * cos;
    } else {
        let (sin2, cos2) = (sin * sin, cos * cos);
        let (sblur2, tblur2) = (sblur * sblur, tblur * tblur);
        major += (sblur2 * cos2 + tblur2 * sin2).sqrt();
        minor += (sblur2 * sin2 + tblur2 * cos2).sqrt();
    }
    if minor > major {
        std::mem::swap(&mut minor, &mut major);
        theta += std::f32::consts::FRAC_PI_2;
    }
    Ellipse {
        major,
        minor,
        theta,
    }
}

/// Result of [`anisotropic_aspect`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aspect {
    /// The aspect ratio actually used, clamped to `[1, anisotropic]`.
    pub aspect: f32,
    /// The unclamped aspect ratio (clamped only to `[1, 1e6]`), for statistics.
    pub true_aspect: f32,
}

/// Clamps the ellipse's aspect ratio to `anisotropic`, reshaping the ellipse to fit.
///
/// Ports `anisotropic_aspect` (`texture_pvt.h:492-528`). With `conservative` (OIIO's default) the
/// major axis becomes the average of itself and `minor * anisotropic` and the minor axis follows
/// from it (solution "c"); otherwise the major axis is clamped to `minor * anisotropic`
/// (solution "b", may alias, never over-blurs).
pub fn anisotropic_aspect(e: &mut Ellipse, anisotropic: f32, conservative: bool) -> Aspect {
    let mut aspect = (e.major / e.minor).clamp(1.0, 1.0e6);
    let true_aspect = aspect;
    if aspect > anisotropic {
        aspect = anisotropic;
        if conservative {
            e.major = 0.5 * (e.major + e.minor * anisotropic);
            e.minor = e.major / anisotropic;
        } else {
            e.major = e.minor * anisotropic;
        }
    }
    Aspect {
        aspect,
        true_aspect,
    }
}

/// The isotropic filter width of the `OneLevel`/`Trilinear` modes.
///
/// Ports `texture_lookup_trilinear_mipmap` (`texturesys.cpp:2070-2075`): the larger (or, without
/// `conservative`, smaller) of the per-axis derivative extents, plus the larger blur. `d` is
/// expected to have gone through [`adjust_width`] already.
pub fn isotropic_filter_width(d: Derivs, conservative: bool, sblur: f32, tblur: f32) -> f32 {
    let sfilt = d.dsdx.abs().max(d.dsdy.abs());
    let tfilt = d.dtdx.abs().max(d.dtdy.abs());
    let filtwidth = if conservative {
        sfilt.max(tfilt)
    } else {
        sfilt.min(tfilt)
    };
    filtwidth + sblur.max(tblur)
}

// ============================================================================
// MIP level selection
// ============================================================================

/// A source with MIP levels, as far as level selection is concerned.
pub trait MipSource {
    /// Number of MIP levels (at least 1).
    fn num_levels(&self) -> usize;
    /// `(width, height)` of level `m` in texels.
    fn level_size(&self, m: usize) -> (u32, u32);
    /// The finest level lookups may use (OIIO `SubimageInfo::min_mip_level`, raised by
    /// `max_mip_res`, `imagecache.cpp:891-896`). Defaults to 0.
    fn min_level(&self) -> usize {
        0
    }
    /// `max(full_width, full_height)` of the image, used to cap degenerate aspect ratios
    /// (`texturesys.cpp:2013`). Defaults to the larger dimension of level 0.
    fn full_resolution(&self) -> u32 {
        let (w, h) = self.level_size(0);
        w.max(h)
    }
}

impl MipSource for [(u32, u32)] {
    fn num_levels(&self) -> usize {
        self.len()
    }
    fn level_size(&self, m: usize) -> (u32, u32) {
        self[m]
    }
}

/// Which levels [`compute_miplevels`] may return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MipSelect {
    /// Always the finest usable level (`MipModeNoMIP`).
    NoMip,
    /// The single coarser level of the bracketing pair (`MipModeOneLevel`).
    OneLevel,
    /// Blend the bracketing pair (`Default`, `Trilinear`, `Aniso`).
    Blend,
}

/// One or two MIP levels and their blend weights.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MipBlend {
    /// The two levels, finer first. Equal when only one contributes.
    pub levels: [usize; 2],
    /// Their weights, summing to 1. A zero weight means "skip this level" (`:2103`, `:2358`).
    pub weights: [f32; 2],
}

impl MipBlend {
    /// The contributing `(level, weight)` pairs, in the reference's order.
    pub fn contributions(&self) -> impl Iterator<Item = (usize, f32)> + '_ {
        (0..2)
            .filter(|&i| self.weights[i] != 0.0)
            .map(|i| (self.levels[i], self.weights[i]))
    }
}

/// Chooses the MIP level(s) so the minor axis is about one texel, and their blend weights.
///
/// Ports `compute_miplevels` (`texturesys.cpp:1962-2042`). The level is chosen off
/// `min(width, height)` of each level (`si.minwh`); the blend is `clamp(2*w - 1, 0, 1)` where
/// `w` is the minor axis in texels at the coarser level. When the ideal level is finer than the
/// finest available, the finest is used alone and, if the minor axis is under half a texel at
/// full resolution, `aspect` is re-derived from the major axis (`:2009-2017`), which is why it is
/// passed by reference. `rnd` enables OIIO's stochastic MIP choice (`:2026-2041`): one level is
/// picked by comparing `rnd` to the blend, and `rnd` is rescaled for reuse.
pub fn compute_miplevels<M: MipSource + ?Sized>(
    src: &M,
    select: MipSelect,
    anisotropic: f32,
    major: f32,
    minor: f32,
    aspect: &mut f32,
    rnd: Option<&mut f32>,
) -> MipBlend {
    let nlevels = src.num_levels().max(1);
    let min_level = src.min_level().min(nlevels - 1);
    let minwh = |m: usize| {
        let (w, h) = src.level_size(m);
        w.min(h) as f32
    };

    let mut found: Option<(usize, usize, f32)> = None;
    for m in min_level..nlevels {
        let filtwidth_ras = minor * minwh(m);
        if filtwidth_ras <= 1.0 {
            let blend = (2.0 * filtwidth_ras - 1.0).clamp(0.0, 1.0);
            // `m - 1` may be below `min_level` (even "-1" when m == 0); handled below.
            found = Some((m, m, blend));
            break;
        }
    }

    let Some((_, m1, blend)) = found else {
        // Blur more than even the coarsest level allows (`:1992-2000`).
        return MipBlend {
            levels: [nlevels - 1; 2],
            weights: [1.0, 0.0],
        };
    };
    let m0 = m1 as isize - 1;

    if m0 < min_level as isize || select == MipSelect::NoMip {
        let r = src.full_resolution() as f32;
        if minor * r < 0.5 {
            *aspect = (major * r * 2.0).clamp(1.0, anisotropic);
        }
        return MipBlend {
            levels: [min_level; 2],
            weights: [1.0, 0.0],
        };
    }
    let m0 = m0 as usize;
    if select == MipSelect::OneLevel {
        return MipBlend {
            levels: [m1; 2],
            weights: [1.0, 0.0],
        };
    }
    if let Some(rnd) = rnd {
        let level = if *rnd >= blend {
            *rnd = ((*rnd - blend) / (1.0 - blend)).clamp(0.0, 1.0);
            m0
        } else {
            *rnd = (*rnd / blend).clamp(0.0, 1.0);
            m1
        };
        return MipBlend {
            levels: [level; 2],
            weights: [1.0, 0.0],
        };
    }
    MipBlend {
        levels: [m0, m1],
        weights: [1.0 - blend, blend],
    }
}

// ============================================================================
// Probes along the major axis
// ============================================================================

/// One probe along the major axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Probe {
    /// Position in `[-1, 1]` along the sampling line.
    pub pos: f32,
    /// Normalized weight; all probes of a line sum to 1.
    pub weight: f32,
    /// Offset from the lookup's `s`.
    pub ds: f32,
    /// Offset from the lookup's `t`.
    pub dt: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Weighting {
    /// `n == 1` or `n == 2`: equal weights at fixed positions (`:2234-2241`).
    Flat,
    /// Gaussian `exp(-2 x^2)` with `x = pos * scale`, normalized by `sum` (`:2242-2255`).
    Gaussian { scale: f32, sum: f32 },
    /// A single stochastic probe at `pos` (`:2213-2224`).
    Stochastic { pos: f32 },
}

/// The probes a lookup takes along the ellipse's major axis.
///
/// Returned by [`compute_ellipse_sampling`]; iterate with [`ProbeLine::probes`]. Nothing is
/// allocated: each probe is recomputed on demand, bit-identically to the reference's arrays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProbeLine {
    /// s-extent of the line per unit of `pos`, already halved for pixel-to-pixel derivatives
    /// (`texturesys.cpp:2320-2325`).
    pub smajor: f32,
    /// t-extent of the line per unit of `pos`, already halved.
    pub tmajor: f32,
    count: usize,
    weighting: Weighting,
}

impl ProbeLine {
    /// Number of probes.
    pub fn len(&self) -> usize {
        self.count
    }

    /// True when there are no probes (never, for a line from [`compute_ellipse_sampling`]).
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Probe `i` (`0 <= i < len()`).
    pub fn probe(&self, i: usize) -> Probe {
        let (pos, weight) = match self.weighting {
            Weighting::Stochastic { pos } => (pos, 1.0),
            Weighting::Flat if self.count == 1 => (0.0, 1.0),
            Weighting::Flat => (if i == 0 { -0.5 } else { 0.5 }, 0.5),
            Weighting::Gaussian { scale, sum } => {
                let pos = line_pos(i, self.count);
                (pos, gaussian(pos, scale) / sum)
            }
        };
        Probe {
            pos,
            weight,
            ds: pos * self.smajor,
            dt: pos * self.tmajor,
        }
    }

    /// All probes, in order.
    pub fn probes(&self) -> impl Iterator<Item = Probe> + '_ {
        (0..self.count).map(|i| self.probe(i))
    }
}

/// `sp = 2*(i+0.5)/n - 1`, with `1/n` formed first as the reference does (`:2233`, `:2246`).
fn line_pos(i: usize, n: usize) -> f32 {
    let invsamples = 1.0 / n as f32;
    2.0 * (i as f32 + 0.5) * invsamples - 1.0
}

fn gaussian(pos: f32, scale: f32) -> f32 {
    let x = pos * scale;
    (-2.0 * x * x).exp()
}

/// Places the probes along the major axis and weighs them.
///
/// Ports `compute_ellipse_sampling` (`texturesys.cpp:2201-2257`): `max(1, int(2*aspect - 1))`
/// probes at `p_i = 2(i+0.5)/n - 1` spanning `2*(major - minor)`, flat-weighted for one or two
/// probes and Gaussian-weighted otherwise. `stochastic_rnd` selects OIIO's single stochastic probe
/// (`StochasticStrategy_Aniso`), placed by warping `rnd` to a windowed Gaussian.
///
/// The returned `smajor`/`tmajor` include `texture_lookup`'s 0.5 for pixel-to-pixel derivatives
/// (`:2320-2325`), so `s + probe.ds` is a probe's texture coordinate directly.
pub fn compute_ellipse_sampling(aspect: f32, e: Ellipse, stochastic_rnd: Option<f32>) -> ProbeLine {
    let (sin, cos) = e.theta.sin_cos();
    let ll = 2.0 * (e.major - e.minor);
    let smajor = cos * ll * 0.5;
    let tmajor = sin * ll * 0.5;
    if let Some(rnd) = stochastic_rnd {
        let sqrt2 = std::f32::consts::SQRT_2;
        let pos = sqrt2 * erfinv((2.0 * rnd - 1.0) * erf(1.0 / sqrt2));
        return ProbeLine {
            smajor,
            tmajor,
            count: 1,
            weighting: Weighting::Stochastic { pos },
        };
    }
    let count = ((2.0 * aspect - 1.0) as i32).max(1) as usize;
    let weighting = if count <= 2 {
        Weighting::Flat
    } else {
        let scale = e.major / ll;
        let sum = (0..count)
            .map(|i| gaussian(line_pos(i, count), scale))
            .sum();
        Weighting::Gaussian { scale, sum }
    };
    ProbeLine {
        smajor,
        tmajor,
        count,
        weighting,
    }
}

/// `round_to_multiple_of_pow2(2 * anisotropic, 4)` (`texturesys.cpp:2310`): the most probes a
/// lookup with this `anisotropic` limit can take, rounded up for 4-wide batches.
pub fn max_probes(anisotropic: u32) -> usize {
    ((2 * anisotropic as usize) + 3) & !3
}

// ============================================================================
// Texel addressing and interpolation weights
// ============================================================================

/// Converts a texture coordinate to the texel to its upper left and the fraction past it.
///
/// Ports `st_to_texel` (`texture_pvt.h:533-555`): texel centres are at `(i + 0.5)/res` unless
/// `sample_border`, where the first and last texel lie exactly on 0 and 1. `origin` is the data
/// window's origin at this level.
pub fn st_to_texel(s: f32, width: u32, origin: i32, sample_border: bool) -> (i32, f32) {
    let x = if sample_border {
        s * (width as f32 - 1.0) + origin as f32
    } else {
        st_to_px(s, width) + origin as f32
    };
    let f = x.floor();
    (f as i32, x - f)
}

/// Cubic B-spline weights of the four taps around `fraction`, and optionally their derivatives.
///
/// Ports `evalBSplineWeights_and_derivs` (`texturesys.cpp:2924-2939`); taps are at texel offsets
/// -1, 0, +1, +2 from the texel [`st_to_texel`] returns.
pub fn bspline_weights(fraction: f32) -> [f32; 4] {
    let f = fraction;
    let g = 1.0 - f;
    [
        (1.0 / 6.0) * g * g * g,
        (2.0 / 3.0) - 0.5 * f * f * (2.0 - f),
        (2.0 / 3.0) - 0.5 * g * g * (2.0 - g),
        (1.0 / 6.0) * f * f * f,
    ]
}

/// Derivatives of [`bspline_weights`] with respect to `fraction` (`texturesys.cpp:2933-2938`).
pub fn bspline_weight_derivs(fraction: f32) -> [f32; 4] {
    let f = fraction;
    let g = 1.0 - f;
    [
        -0.5 * g * g,
        0.5 * f * (3.0 * f - 4.0),
        -0.5 * g * (3.0 * g - 4.0),
        0.5 * f * f,
    ]
}

/// The B-spline weights as the reference's lerp formulation consumes them.
///
/// `sample_bicubic` (`texturesys.cpp:3282-3317`) evaluates the 4-tap B-spline as three lerps:
/// `lerp(lerp(t0, t1, h0), lerp(t2, t3, h1), g1)` with `g0 = w0 + w1`, `h0 = w1 / g0`,
/// `g1 = w2 + w3`, `h1 = w3 / g1`. Returns `(h0, h1, g1)`. Because every step is a convex
/// combination, non-negative texels always give a non-negative result, which a direct weighted
/// sum does not guarantee under rounding (`:3257-3261`).
pub fn bspline_lerp_weights(w: [f32; 4]) -> (f32, f32, f32) {
    let g0 = w[0] + w[1];
    let g1 = w[2] + w[3];
    (w[1] / g0, w[3] / g1, g1)
}

/// `(1 - t) * a + t * b`, OIIO's `lerp`.
#[inline]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    (1.0 - t) * a + t * b
}

/// Bilinear blend of four corners, OIIO's `bilerp(v0, v1, v2, v3, s, t)`.
#[inline]
pub fn bilerp(v00: f32, v10: f32, v01: f32, v11: f32, s: f32, t: f32) -> f32 {
    lerp(lerp(v00, v10, s), lerp(v01, v11, s), t)
}

// ============================================================================
// Scalar helpers ported from OpenImageIO/fmath.h
// ============================================================================

#[inline]
fn safe_sqrt(x: f32) -> f32 {
    if x >= 0.0 { x.sqrt() } else { 0.0 }
}

/// OIIO's `fast_sincos` (`fmath.h:1587-1613`): Cody-Waite reduction by pi, then the minimax
/// polynomials of `fast_sin`/`fast_cos`. Returns `(sin, cos)`, max error a few ulp.
///
/// `fast_rint` is the emulated `int(x + copysign(0.5, x))` of builds without SSE4.1
/// (`fmath.h:1510-1519`), which is what an MSVC x64 build of OIIO compiles, and `madd` is an
/// unfused `a * b + c` (`fmath.h:365-370`, no FP contraction under MSVC `/fp:precise`).
// Constants and operation order are `fmath.h`'s, digit for digit.
#[allow(clippy::excessive_precision, clippy::assign_op_pattern)]
pub fn fast_sincos(x: f32) -> (f32, f32) {
    let q = (x * std::f32::consts::FRAC_1_PI + 0.5f32.copysign(x)) as i32;
    let qf = q as f32;
    let mut x = qf * (-0.785_156_25 * 4.0) + x;
    x = qf * (-0.000_241_875_648_498_535_156_25 * 4.0) + x;
    x = qf * (-3.774_766_810_238_361_358_6e-8 * 4.0) + x;
    x = qf * (-1.281_672_034_128_544_801_5e-12 * 4.0) + x;
    x = std::f32::consts::FRAC_PI_2 - (std::f32::consts::FRAC_PI_2 - x);
    let s = x * x;
    if q & 1 != 0 {
        x = -x;
    }
    let mut su = 2.608_315_980_978_659_354_150_3e-6f32;
    su = su * s + -0.000_198_106_907_191_686_332_225_8;
    su = su * s + 0.008_333_078_585_565_090_179_443_36;
    su = su * s + -0.166_666_597_127_914_428_710_938;
    su = s * (su * x) + x;
    let mut cu = -2.718_118_423_672_422_068_193_55e-7f32;
    cu = cu * s + 2.479_904_469_510_074_704_885_48e-5;
    cu = cu * s + -0.001_388_887_874_782_085_418_701_17;
    cu = cu * s + 0.041_666_664_183_139_801_025_390_6;
    cu = cu * s + -0.5;
    cu = cu * s + 1.0;
    if q & 1 != 0 {
        cu = -cu;
    }
    (su.clamp(-1.0, 1.0), cu.clamp(-1.0, 1.0))
}

/// OIIO's `fast_atan2` (`fmath.h:1747-1782`, `OIIO_FMATH_SIMD_FRIENDLY == 0`), max error ~9e-6.
///
/// Ported rather than replaced by `f32::atan2` because it decides the probe line's direction;
/// using the same approximation keeps probe positions bit-comparable with the reference.
pub fn fast_atan2(y: f32, x: f32) -> f32 {
    let a = x.abs();
    let b = y.abs();
    let k = if b == 0.0 {
        0.0
    } else if a == b {
        1.0
    } else if b > a {
        a / b
    } else {
        b / a
    };
    let s = 1.0 - (1.0 - k);
    let t = s * s;
    let mut r = s * (0.430_165_68 * t + 1.0) / ((0.057_935_5 * t + 0.763_008) * t + 1.0);
    if b > a {
        r = 1.570_796_4 - r;
    }
    if x.is_sign_negative() {
        r = std::f32::consts::PI - r;
    }
    r.copysign(y)
}

/// Error function by its Maclaurin series in f64.
///
/// Only the stochastic probe uses it, on `|x| <= 1/sqrt(2)`, where the series converges in a
/// few terms. OIIO uses `fast_erf`/`fast_ierf` (`fmath.h`) there; those are approximations of
/// the same functions and a stochastic probe has no single reference value to match anyway.
fn erf(x: f32) -> f32 {
    // erf(x) = 1 - erfc(x); a Taylor series is exact enough on the |x| <= 1/sqrt(2) the
    // stochastic path uses, and converges for all |x| < 2.
    let x = x as f64;
    let mut term = x;
    let mut sum = x;
    let x2 = x * x;
    for n in 1..40 {
        term *= -x2 / n as f64;
        let add = term / (2 * n + 1) as f64;
        sum += add;
        if add.abs() < 1e-17 {
            break;
        }
    }
    (sum * 2.0 / std::f64::consts::PI.sqrt()) as f32
}

/// Inverse error function on `(-1, 1)`, by Newton iteration on [`erf`].
fn erfinv(y: f32) -> f32 {
    let y = y.clamp(-0.999_999, 0.999_999);
    let mut x = 0.0f32;
    for _ in 0..30 {
        let e = erf(x) - y;
        let d = 2.0 / std::f32::consts::PI.sqrt() * (-x * x).exp();
        let step = e / d;
        x -= step;
        if step.abs() < 1e-7 {
            break;
        }
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn wrap_texel_matches_reference_table() {
        // Black: outside is None.
        assert_eq!(Wrap::Black.wrap_texel(-1, 0, 10), None);
        assert_eq!(Wrap::Black.wrap_texel(10, 0, 10), None);
        assert_eq!(Wrap::Black.wrap_texel(4, 0, 10), Some(4));
        assert_eq!(Wrap::Default.wrap_texel(-1, 0, 10), None);
        // Clamp.
        assert_eq!(Wrap::Clamp.wrap_texel(15, 0, 10), Some(9));
        assert_eq!(Wrap::Clamp.wrap_texel(-3, 0, 10), Some(0));
        // Periodic, incl. negatives, and pow2 agreeing with it.
        assert_eq!(Wrap::Periodic.wrap_texel(15, 0, 10), Some(5));
        assert_eq!(Wrap::Periodic.wrap_texel(-3, 0, 10), Some(7));
        for c in -20..20 {
            assert_eq!(
                Wrap::PeriodicPow2.wrap_texel(c, 0, 8),
                Wrap::Periodic.wrap_texel(c, 0, 8)
            );
        }
        // Mirror reflects about -1/2: -1 -> 0, and flips sense each period.
        let m: Vec<i32> = (-2..9)
            .map(|c| Wrap::Mirror.wrap_texel(c, 0, 4).unwrap())
            .collect();
        assert_eq!(m, vec![1, 0, 0, 1, 2, 3, 3, 2, 1, 0, 0]);
        // Shared border: period width-1.
        assert_eq!(Wrap::PeriodicSharedBorder.wrap_texel(4, 0, 5), Some(0));
        assert_eq!(Wrap::PeriodicSharedBorder.wrap_texel(5, 0, 5), Some(1));
        // A data-window origin shifts everything.
        assert_eq!(Wrap::Periodic.wrap_texel(1, 2, 4), Some(5));
        assert_eq!(Wrap::Black.wrap_texel(1, 2, 4), None);
    }

    #[test]
    fn wrap_resolve_and_parse() {
        assert_eq!(Wrap::Default.resolve(Wrap::Default), Wrap::Black);
        assert_eq!(Wrap::Default.resolve(Wrap::Periodic), Wrap::Periodic);
        assert_eq!(Wrap::Clamp.resolve(Wrap::Periodic), Wrap::Clamp);
        assert_eq!(
            Wrap::parse_pair("periodic,clamp"),
            (Wrap::Periodic, Wrap::Clamp)
        );
        assert_eq!(Wrap::parse_pair("mirror"), (Wrap::Mirror, Wrap::Mirror));
    }

    #[test]
    fn adjust_width_scales_and_replaces_degenerate_derivs() {
        let d = adjust_width(Derivs::new(0.1, 0.2, 0.3, 0.4), 2.0, 3.0);
        assert_eq!(d, Derivs::new(0.2, 0.6, 0.6, 1.2));
        // Both tiny -> the fixed eps filter.
        assert_eq!(
            adjust_width(Derivs::default(), 1.0, 1.0),
            Derivs::new(1e-8, 0.0, 0.0, 1e-8)
        );
        // Tiny dy: replaced by an eps-long vector orthogonal to dx.
        let d = adjust_width(Derivs::new(0.1, 0.0, 0.0, 0.0), 1.0, 1.0);
        assert_eq!((d.dsdx, d.dtdx), (0.1, 0.0));
        assert!(close(d.dsdy, 0.0, 0.0) && close(d.dtdy, 1e-8, 1e-12));
    }

    #[test]
    fn ellipse_axes_of_axis_aligned_and_rotated_derivs() {
        let e = ellipse_axes(Derivs::new(4.0, 0.0, 0.0, 1.0));
        assert!(
            close(e.major, 4.0, 1e-5) && close(e.minor, 1.0, 1e-5),
            "{e:?}"
        );
        assert!(
            close(e.theta.cos().abs(), 1.0, 1e-4),
            "major along s: {e:?}"
        );
        let e = ellipse_axes(Derivs::new(0.0, 3.0, 0.5, 0.0));
        assert!(
            close(e.major, 3.0, 1e-5) && close(e.minor, 0.5, 1e-5),
            "{e:?}"
        );
        assert!(
            close(e.theta.sin().abs(), 1.0, 1e-4),
            "major along t: {e:?}"
        );
        // Rotating the derivative pair rotates the ellipse, not its axes.
        let (c, s) = (0.6f32, 0.8f32);
        let e = ellipse_axes(Derivs::new(4.0 * c, 4.0 * s, -s, c));
        assert!(
            close(e.major, 4.0, 1e-4) && close(e.minor, 1.0, 1e-4),
            "{e:?}"
        );
        assert!(close(e.theta.tan(), s / c, 1e-3), "{e:?}");
        // Axis caps at 1000.
        assert_eq!(
            ellipse_axes(Derivs::new(5000.0, 0.0, 0.0, 1.0)).major,
            1000.0
        );
    }

    #[test]
    fn ellipse_coeffs_describe_the_same_ellipse() {
        let d = Derivs::new(0.08, 0.03, -0.01, 0.015);
        let e = ellipse_axes(d);
        let k = ellipse_coeffs(d);
        // The major-axis end point lies on the ellipse (up to the pixel-to-pixel convention,
        // where A u^2 + B u v + C v^2 = 1 on the boundary).
        let (u, v) = (e.major * e.theta.cos(), e.major * e.theta.sin());
        let q = k.a * (u * u) as f64 + k.b * (u * v) as f64 + k.c * (v * v) as f64;
        assert!((q - 1.0).abs() < 1e-3, "q={q}");
    }

    #[test]
    fn fast_sincos_tracks_sin_cos() {
        for i in -400..=400 {
            let x = i as f32 * 0.0173;
            let (s, c) = fast_sincos(x);
            assert!(
                close(s, x.sin(), 2e-6) && close(c, x.cos(), 2e-6),
                "x={x}: {s} {c}"
            );
        }
    }

    #[test]
    fn fast_atan2_tracks_atan2() {
        for i in -50..=50 {
            for j in -50..=50 {
                let (y, x) = (i as f32 * 0.37, j as f32 * 0.41);
                assert!(close(fast_atan2(y, x), y.atan2(x), 2e-5), "atan2({y},{x})");
            }
        }
    }

    #[test]
    fn adjust_blur_legacy_and_quadrature() {
        let e = Ellipse {
            major: 0.1,
            minor: 0.02,
            theta: 0.0,
        };
        let l = adjust_blur(e, 0.01, 0.03, true);
        assert!(
            close(l.major, 0.11, 1e-6) && close(l.minor, 0.05, 1e-6),
            "{l:?}"
        );
        let q = adjust_blur(e, 0.01, 0.03, false);
        assert!(
            close(q.major, 0.11, 1e-6) && close(q.minor, 0.05, 1e-6),
            "{q:?}"
        );
        // No blur, no change.
        assert_eq!(adjust_blur(e, 0.0, 0.0, true), e);
        // Blur that makes the minor axis longer swaps the axes.
        let s = adjust_blur(e, 0.0, 0.2, true);
        assert!(
            s.major >= s.minor && close(s.theta, std::f32::consts::FRAC_PI_2, 1e-6),
            "{s:?}"
        );
    }

    #[test]
    fn anisotropic_aspect_clamps_both_ways() {
        let mut e = Ellipse {
            major: 1.0,
            minor: 0.01,
            theta: 0.0,
        };
        let a = anisotropic_aspect(&mut e, 8.0, true);
        assert!(a.aspect == 8.0 && close(a.true_aspect, 100.0, 1e-3));
        assert!(close(e.major, 0.5 * (1.0 + 0.08), 1e-6) && close(e.minor, e.major / 8.0, 1e-7));
        let mut e = Ellipse {
            major: 1.0,
            minor: 0.01,
            theta: 0.0,
        };
        anisotropic_aspect(&mut e, 8.0, false);
        assert!(close(e.major, 0.08, 1e-7) && e.minor == 0.01);
        let mut e = Ellipse {
            major: 0.3,
            minor: 0.1,
            theta: 0.0,
        };
        assert_eq!(anisotropic_aspect(&mut e, 8.0, true).aspect, 3.0);
    }

    fn chain(w: u32, h: u32) -> Vec<(u32, u32)> {
        (0..crate::mip::mip_level_count(w, h))
            .map(|m| crate::mip::mip_dimensions(w, h, m))
            .collect()
    }

    #[test]
    fn compute_miplevels_brackets_the_minor_axis() {
        let c = chain(96, 64); // minwh 64, 32, 16, 8, 4, 2, 1
        let mut aspect = 1.0;
        // 3 texels at level 0 of the 64-high axis: filtwidth_ras at level 1 = 1.5, level 2 = 0.75.
        let b = compute_miplevels(
            &c[..],
            MipSelect::Blend,
            32.0,
            3.0 / 64.0,
            3.0 / 64.0,
            &mut aspect,
            None,
        );
        assert_eq!(b.levels, [1, 2]);
        assert!(
            close(b.weights[1], 0.5, 1e-6) && close(b.weights[0], 0.5, 1e-6),
            "{b:?}"
        );
        let o = compute_miplevels(
            &c[..],
            MipSelect::OneLevel,
            32.0,
            3.0 / 64.0,
            3.0 / 64.0,
            &mut aspect,
            None,
        );
        assert_eq!((o.levels, o.weights), ([2, 2], [1.0, 0.0]));
        // Magnification pins the finest level, and a sub-half-texel minor axis re-derives aspect.
        let mut aspect = 32.0;
        let m = compute_miplevels(&c[..], MipSelect::Blend, 32.0, 0.1, 1e-8, &mut aspect, None);
        assert_eq!((m.levels, m.weights), ([0, 0], [1.0, 0.0]));
        assert!(close(aspect, 19.2, 1e-5)); // 0.1 * 96 * 2
        // Wider than the whole texture: coarsest level.
        let z = compute_miplevels(&c[..], MipSelect::Blend, 32.0, 5.0, 5.0, &mut aspect, None);
        assert_eq!((z.levels, z.weights), ([6, 6], [1.0, 0.0]));
        // NoMip always gives the finest.
        let n = compute_miplevels(&c[..], MipSelect::NoMip, 32.0, 0.5, 0.5, &mut aspect, None);
        assert_eq!(n.levels, [0, 0]);
        // Stochastic picks exactly one of the pair and rescales rnd.
        let mut rnd = 0.75;
        let s = compute_miplevels(
            &c[..],
            MipSelect::Blend,
            32.0,
            3.0 / 64.0,
            3.0 / 64.0,
            &mut aspect,
            Some(&mut rnd),
        );
        assert_eq!((s.levels, s.weights), ([1, 1], [1.0, 0.0]));
        assert!(close(rnd, 0.5, 1e-6));
    }

    #[test]
    fn ellipse_sampling_counts_positions_and_weights() {
        let e = Ellipse {
            major: 0.4,
            minor: 0.1,
            theta: 0.0,
        };
        let line = compute_ellipse_sampling(4.0, e, None);
        assert_eq!(line.len(), 7); // int(2*4 - 1)
        let probes: Vec<Probe> = line.probes().collect();
        let sum: f32 = probes.iter().map(|p| p.weight).sum();
        assert!(close(sum, 1.0, 1e-6));
        assert!(close(probes[0].pos, -6.0 / 7.0, 1e-6) && close(probes[6].pos, 6.0 / 7.0, 1e-6));
        assert!(
            probes[3].weight > probes[0].weight,
            "Gaussian peaks at the centre"
        );
        // Span: (major - minor) either side of the centre, i.e. LL/2 per unit pos.
        assert!(close(line.smajor, 0.3, 1e-6) && close(line.tmajor, 0.0, 1e-6));
        // One and two probes are flat.
        let one = compute_ellipse_sampling(1.0, e, None);
        assert_eq!(
            one.probes().collect::<Vec<_>>(),
            vec![Probe {
                pos: 0.0,
                weight: 1.0,
                ds: 0.0,
                dt: 0.0
            }]
        );
        let two: Vec<Probe> = compute_ellipse_sampling(1.6, e, None).probes().collect();
        assert_eq!((two[0].pos, two[1].pos, two[0].weight), (-0.5, 0.5, 0.5));
        // Stochastic: one probe, centred for rnd = 0.5, within the window otherwise.
        let st = compute_ellipse_sampling(4.0, e, Some(0.5));
        assert_eq!(st.len(), 1);
        assert!(close(st.probe(0).pos, 0.0, 1e-6));
        let edge = compute_ellipse_sampling(4.0, e, Some(0.999)).probe(0).pos;
        assert!(edge > 0.9 && edge <= 1.0 + 1e-4, "{edge}");
        assert_eq!(max_probes(32), 64);
        assert_eq!(max_probes(3), 8);
    }

    #[test]
    fn st_to_texel_centres_and_border() {
        assert_eq!(st_to_texel(0.5 / 8.0, 8, 0, false), (0, 0.0));
        assert_eq!(st_to_texel(0.0, 8, 0, false), (-1, 0.5));
        assert_eq!(st_to_texel(1.0, 5, 0, true), (4, 0.0));
        assert_eq!(st_to_texel(0.5 / 8.0, 8, 3, false), (3, 0.0));
    }

    #[test]
    fn bspline_weights_shape_and_lerp_form() {
        let w = bspline_weights(0.0);
        for (g, e) in w.iter().zip([1.0 / 6.0, 2.0 / 3.0, 1.0 / 6.0, 0.0]) {
            assert!(close(*g, e, 1e-6));
        }
        let w = bspline_weights(0.5);
        for (g, e) in w
            .iter()
            .zip([1.0 / 48.0, 23.0 / 48.0, 23.0 / 48.0, 1.0 / 48.0])
        {
            assert!(close(*g, e, 1e-6));
        }
        for step in 0..=10 {
            let f = step as f32 / 10.0;
            let w = bspline_weights(f);
            assert!(close(w.iter().sum::<f32>(), 1.0, 1e-5));
            let dw = bspline_weight_derivs(f);
            assert!(close(dw.iter().sum::<f32>(), 0.0, 1e-5));
            // The lerp form reproduces the weighted sum of arbitrary taps.
            let taps = [0.3f32, -1.0, 2.5, 0.7];
            let direct: f32 = (0..4).map(|i| w[i] * taps[i]).sum();
            let (h0, h1, g1) = bspline_lerp_weights(w);
            let via_lerp = lerp(lerp(taps[0], taps[1], h0), lerp(taps[2], taps[3], h1), g1);
            assert!(close(direct, via_lerp, 1e-5), "f={f}");
        }
    }
}
