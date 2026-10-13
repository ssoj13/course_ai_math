//! The workspace's ONE resampling-filter vocabulary.
//!
//! ## History — why this looks like it merged twice
//!
//! "Which resampling kernel?" was answered by three separate enums in this repository, each with
//! a different variant set, a different default, and a different name (`vfx_ops::resize::Filter`,
//! `vfx_warp::Filter`, `vfx_compute::ResizeFilter`). Those were unified into a single reduced
//! 5-kernel `Filter` type here first (Nearest/Bilinear/Bicubic/Lanczos3/Mitchell), sized to match
//! exactly what the two GPU backends (`vfx-compute`, `vfx-warp`) had kernels for.
//!
//! That reduced type turned out to be its own SSOT violation once OIIO's full 13-filter registry
//! (`filter.cpp`) was ported in alongside it as `FilterKind`/`FilterDesc` (bughunt lane "ops",
//! 2026-09-22) for `vfx-io`'s `ImageBufAlgo`: two vocabularies for one concept, two `resample_f32`
//! entry points, two `resize_f32` entry points. This module is the SECOND, real merge:
//! **`FilterKind` is now the ONE type.** `Filter`, `weight()`, `support()`, `taps()` are GONE. The
//! GPU wire format is a derived, backend-specific, PARTIAL property of `FilterKind`
//! ([`FilterKind::compute_index`], `Option<u32>`, for vfx-compute's fixed-kernel resize), not a
//! separate type sized to fit only what GPU code needs. The warp kernels take any separable
//! filter as a weight table built from [`FilterDesc::xfilt`] and need no numbering at all.
//!
//! One consequence worth calling out because it is a real, deliberate behavior change, not an
//! oversight: `FilterKind` adds a **`Nearest`** variant that is NOT one of OIIO's 13 named
//! filters. OIIO's own registry has no point-sampling entry — `"box"`/`"nearest"` are aliases of
//! the SAME closed-interval formula (`filter.cpp`'s box, `<=half_width`). The old reduced `Filter`
//! type's `Nearest`, however, was an OPEN-interval point sampler (`|x| < 0.5`) with its own
//! exact-integer-arithmetic fast path in `resample_f32`, kept distinct from `Box` on purpose (see
//! that module). Losing that distinction on unification would have silently changed every
//! existing `Filter::Nearest` caller's boundary behavior and broken the ulp-precision CPU/GPU
//! parity gate. So `FilterKind::Nearest` exists, `"nearest"` resolves to it (not to `Box`), and
//! `"box"` alone reaches the true OIIO box filter — a workspace extension to OIIO's vocabulary,
//! not a divergence from it (nothing named `"nearest"` in `filter.cpp` to diverge from).
//!
//! A second correction made during the merge, also deliberate: the old reduced `Filter::Bicubic`
//! (and its CLI/`from_name` alias `"bicubic"`) meant the classic Catmull-Rom cubic convolution
//! (`cubic_bc(x, B=0, C=0.5)`) — used throughout `vfx-ops::resize`'s docs, tests and CLI help as
//! "Bicubic = sharper than bilinear". `vfx-io`'s OIIO-ported alias table, independently, mapped
//! `"bicubic"` to `FilterKind::Cubic { a: 0.0 }` — a DIFFERENT, flatter curve (see
//! `plain_cubic_has_a_zero_outer_lobe_unlike_catmull_rom` in this module's tests; OIIO's own
//! `filter.cpp` has no `"bicubic"` entry at all, that alias was a workspace addition in both
//! places, independently, and they disagreed). Nothing at runtime called `filter_desc_from_name`
//! with `"bicubic"` (checked), so this was latent, not yet a live bug — but a caller relying on
//! either meaning existing was one merge away from silently getting the other. Resolved in favor
//! of the historically dominant, widely-documented meaning: `"bicubic"` now resolves to
//! [`FilterKind::CatmullRom`]. `"cubic"` alone still resolves to `FilterKind::Cubic { a: 0.0 }`,
//! matching OIIO's actual registry entry.
//!
//! ## GPU numbering: `compute_index`, never `as u32`
//!
//! vfx-compute's fixed-kernel resize sends the filter to its kernels as an integer:
//! [`FilterKind::compute_index`] / [`FilterKind::from_compute_index`] (Nearest 0, Triangle 1,
//! CatmullRom 2, Lanczos3 3; every other kind `None`, the explicit refusal). The ST-map warp
//! kernels (vfx-compute `backend::warp`, which vfx-warp runs on since ROADMAP G11) take any
//! separable filter as a weight table built from [`FilterDesc::xfilt`] and need no numbering.
//! vfx-warp's own shaders numbered the same filters differently (Lanczos3 4, Mitchell 3); they are
//! gone with them. A bare `filter as u32` is a bug.
//!
//! ## No blanket `Default`
//!
//! Each API that picks a filter states its own default at its own boundary: `vfx-ops::resize_f32`
//! takes `Option<&FilterDesc>` and applies OIIO's own direction-dependent rule
//! ([`default_resize_filter_name`]) when `None`; `vfx-warp` favours Mitchell's low ringing for its
//! own default. A `#[derive(Default)]` here would invite exactly the kind of silent,
//! API-boundary-crossing default this module's history is a cautionary tale about.

use std::fmt;

/// One of the workspace's resampling filters — the reduced 5-kernel GPU-backed set (Nearest,
/// Triangle, CatmullRom, Lanczos3, Mitchell) PLUS OIIO's full `filter.cpp` registry (Gaussian,
/// SharpGaussian, BlackmanHarris, Sinc, RadialLanczos3, BSpline, Disk, the Keys `Cubic` family,
/// and `Box` — the true OIIO box/nearest filter, distinct from this type's own `Nearest`, see the
/// module docs).
///
/// `Cubic` carries its own shape parameter `a` because OIIO's `cubic`/`keys`/`simon`/`rifman` are
/// the SAME formula at four different values of it (see [`FilterKind::from_name`]) — one variant,
/// not four.
///
/// No `Eq`/`Hash`/`Ord`: `Cubic { a: f32 }` cannot derive them (`f32` has neither). Nothing in
/// this workspace ever put the old reduced `Filter` type in a container requiring them (checked
/// before dropping the derives).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FilterKind {
    /// Point sampling, OPEN interval (`|x| < 0.5` at default width 1.0). NOT one of OIIO's 13
    /// named filters — a workspace extension kept distinct from [`FilterKind::Box`] specifically
    /// for [`crate::resample::resample_f32`]'s exact-integer-arithmetic fast path and its
    /// ulp-precision CPU/GPU parity gate. See the module docs for why this exists.
    Nearest,
    /// OIIO's own `"box"`/`"nearest"` filter: flat within the support, CLOSED interval
    /// (`|x| <= half_width`), zero outside. `"nearest"` as a name resolves to
    /// [`FilterKind::Nearest`] instead (see module docs) — reach this one by `"box"` alone.
    Box,
    /// Triangle / "bilinear". Linear tent.
    Triangle,
    /// Gaussian, `exp(-2x^2)`.
    Gaussian,
    /// A narrower Gaussian, `exp(-4x^2)` — half the variance, hence "sharp".
    SharpGaussian,
    /// Catmull-Rom cubic convolution (interpolating) — what the old reduced vocabulary's
    /// `Filter::Bicubic` meant, and what `"bicubic"` resolves to (see module docs).
    CatmullRom,
    /// 4-term Blackman-Harris window.
    BlackmanHarris,
    /// Windowed-at-its-own-radius sinc (unlike [`FilterKind::Lanczos3`], which windows a sinc
    /// with a SECOND, narrower sinc).
    Sinc,
    /// 3-lobe Lanczos, separable (the usual resize filter, and this workspace's most common
    /// default).
    Lanczos3,
    /// 3-lobe Lanczos evaluated on the RADIAL distance `sqrt(x^2+y^2)` — NOT separable, exists
    /// only as a 2D filter.
    RadialLanczos3,
    /// Mitchell-Netravali (B=C=1/3).
    Mitchell,
    /// Cubic B-spline (Stark et al, JGT 10(1)) — approximating, not interpolating.
    BSpline,
    /// A flat disk of the given radius — NOT separable (`x^2+y^2 < r^2`, not a product of 1D
    /// terms).
    Disk,
    /// The one-parameter Keys cubic-convolution family. `a = 0.0` is OIIO's plain `"cubic"`;
    /// `a = -0.5` is `"keys"`, the textbook Catmull-Rom cubic-convolution polynomial — but note
    /// [`FilterKind::CatmullRom`] is a SEPARATE OIIO registry entry that independently carries
    /// exactly double this scale at every x (see `keys_cubic_equals_catmull_rom_weight` in this
    /// module's tests), so the two are not numerically interchangeable despite one being the
    /// textbook definition of the other; `a = -0.75` is `"simon"`; `a = -1.0` is `"rifman"`.
    Cubic {
        /// The Keys family shape parameter.
        a: f32,
    },
}

impl FilterKind {
    /// Every DISTINCT named registry entry, canonical form — the four `Cubic` shape parameters
    /// each count once. Used for enumeration (clap's value list, round-trip tests); NOT every
    /// `FilterKind` value that can exist (`Cubic { a }` for an arbitrary `a` is a valid value with
    /// no canonical name and is not listed here).
    pub const ALL: [FilterKind; 17] = [
        FilterKind::Nearest,
        FilterKind::Box,
        FilterKind::Triangle,
        FilterKind::Gaussian,
        FilterKind::SharpGaussian,
        FilterKind::CatmullRom,
        FilterKind::BlackmanHarris,
        FilterKind::Sinc,
        FilterKind::Lanczos3,
        FilterKind::RadialLanczos3,
        FilterKind::Mitchell,
        FilterKind::BSpline,
        FilterKind::Disk,
        FilterKind::Cubic { a: 0.0 },
        FilterKind::Cubic { a: -0.5 },
        FilterKind::Cubic { a: -0.75 },
        FilterKind::Cubic { a: -1.0 },
    ];

    /// The canonical lowercase name. `Cubic { a }` outside the four named shape parameters
    /// (`0.0`/`-0.5`/`-0.75`/`-1.0`) has no canonical name and falls back to `"cubic"`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            FilterKind::Nearest => "nearest",
            FilterKind::Box => "box",
            FilterKind::Triangle => "triangle",
            FilterKind::Gaussian => "gaussian",
            FilterKind::SharpGaussian => "sharp-gaussian",
            FilterKind::CatmullRom => "catmull-rom",
            FilterKind::BlackmanHarris => "blackman-harris",
            FilterKind::Sinc => "sinc",
            FilterKind::Lanczos3 => "lanczos3",
            FilterKind::RadialLanczos3 => "radial-lanczos3",
            FilterKind::Mitchell => "mitchell",
            FilterKind::BSpline => "b-spline",
            FilterKind::Disk => "disk",
            FilterKind::Cubic { a: -0.5 } => "keys",
            FilterKind::Cubic { a: -0.75 } => "simon",
            FilterKind::Cubic { a: -1.0 } => "rifman",
            FilterKind::Cubic { .. } => "cubic",
        }
    }

    /// Accepted spellings, for CLI help text and error messages. Kept next to
    /// [`FilterKind::from_name`] so the help and the parser cannot drift apart.
    pub const VALID_NAMES: &'static str = "nearest, box, triangle (bilinear, linear), gaussian, \
        sharp-gaussian, catmull-rom (catrom, bicubic), blackman-harris, sinc, lanczos3 (lanczos, \
        nuke-lanczos6), radial-lanczos3 (radial-lanczos), mitchell, b-spline (bspline), disk, \
        cubic, keys, simon, rifman";

    /// Parse a user-supplied filter name, case-insensitively.
    ///
    /// The full alias set from both predecessor vocabularies, reconciled — see the module docs
    /// for the two deliberate corrections made merging them (`"nearest"` -> [`FilterKind::Nearest`]
    /// not [`FilterKind::Box`]; `"bicubic"` -> [`FilterKind::CatmullRom`] not
    /// `Cubic { a: 0.0 }`). [`filter_desc_from_name`] uses this for its own name resolution — one
    /// parser, not two.
    ///
    /// Returns `None` for an unknown name; the caller owns the error message and can name its own
    /// valid set (see [`FilterKind::VALID_NAMES`]).
    #[must_use]
    pub fn from_name(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "nearest" => Some(FilterKind::Nearest),
            "box" => Some(FilterKind::Box),
            "triangle" | "bilinear" | "linear" => Some(FilterKind::Triangle),
            "gaussian" => Some(FilterKind::Gaussian),
            "sharp-gaussian" => Some(FilterKind::SharpGaussian),
            "catmull-rom" | "catrom" | "bicubic" => Some(FilterKind::CatmullRom),
            "blackman-harris" => Some(FilterKind::BlackmanHarris),
            "sinc" => Some(FilterKind::Sinc),
            "lanczos3" | "lanczos" | "nuke-lanczos6" => Some(FilterKind::Lanczos3),
            "radial-lanczos3" | "radial-lanczos" => Some(FilterKind::RadialLanczos3),
            "mitchell" => Some(FilterKind::Mitchell),
            "b-spline" | "bspline" => Some(FilterKind::BSpline),
            "disk" => Some(FilterKind::Disk),
            "cubic" => Some(FilterKind::Cubic { a: 0.0 }),
            "keys" => Some(FilterKind::Cubic { a: -0.5 }),
            "simon" => Some(FilterKind::Cubic { a: -0.75 }),
            "rifman" => Some(FilterKind::Cubic { a: -1.0 }),
            _ => None,
        }
    }

    /// This filter's default OIIO full-width (`filter.cpp`'s `FilterDesc` width column; twice the
    /// support radius). [`FilterKind::Nearest`] and [`FilterKind::Box`] share `1.0` (both are
    /// half-support-0.5 point/box samplers); every other value matches the old reduced `Filter`
    /// type's `support() * 2.0` exactly where the two vocabularies overlap (Triangle 2.0,
    /// CatmullRom/Mitchell 4.0, Lanczos3 6.0), which is what made construction via
    /// [`FilterDesc::new`] a safe drop-in for the old fixed-support `Filter` variants.
    #[must_use]
    pub const fn default_width(self) -> f32 {
        match self {
            FilterKind::Nearest | FilterKind::Box | FilterKind::Disk => 1.0,
            FilterKind::Triangle | FilterKind::SharpGaussian => 2.0,
            FilterKind::Gaussian | FilterKind::BlackmanHarris => 3.0,
            FilterKind::CatmullRom
            | FilterKind::Mitchell
            | FilterKind::Sinc
            | FilterKind::BSpline
            | FilterKind::Cubic { .. } => 4.0,
            FilterKind::Lanczos3 | FilterKind::RadialLanczos3 => 6.0,
        }
    }

    /// The integer `vfx-compute`'s wgpu and CUDA resize kernels expect, or `None` when they have
    /// no implementation for this filter.
    ///
    /// Only 4 of the 17 named kinds have one — the same 4 the old reduced `Filter` type's
    /// `compute_index` covered (`Mitchell` returns `None` there too: no compute kernel, never
    /// did). Every OIIO filter this module added has no `vfx-compute` kernel at all, and returning
    /// `None` uniformly for them is what keeps a caller asking for `"gaussian"` on a GPU-resident
    /// image from silently receiving a different filter — there is no kernel to substitute with,
    /// so there is no entry point to reach one through.
    #[must_use]
    pub const fn compute_index(self) -> Option<u32> {
        match self {
            FilterKind::Nearest => Some(0),
            FilterKind::Triangle => Some(1),
            FilterKind::CatmullRom => Some(2),
            FilterKind::Lanczos3 => Some(3),
            _ => None,
        }
    }

    /// The inverse of [`FilterKind::compute_index`].
    #[must_use]
    pub const fn from_compute_index(index: u32) -> Option<Self> {
        match index {
            0 => Some(FilterKind::Nearest),
            1 => Some(FilterKind::Triangle),
            2 => Some(FilterKind::CatmullRom),
            3 => Some(FilterKind::Lanczos3),
            _ => None,
        }
    }
}

impl fmt::Display for FilterKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl std::str::FromStr for FilterKind {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        FilterKind::from_name(s).ok_or_else(|| {
            crate::Error::Other(format!(
                "unknown filter '{s}'. Valid: {}",
                FilterKind::VALID_NAMES
            ))
        })
    }
}

/// Manual `clap::ValueEnum`, not derived: `Cubic { a: f32 }` is not a unit variant, which
/// `#[derive(ValueEnum)]` requires. [`FilterKind::ALL`] supplies the 17 canonical values (the 4
/// named `Cubic` shape parameters each as their own entry); an arbitrary `Cubic { a }` outside
/// those four cannot come from a CLI argument, which is the correct restriction — a `--filter`
/// flag needs a finite, discoverable value set.
#[cfg(feature = "clap")]
impl clap::ValueEnum for FilterKind {
    fn value_variants<'a>() -> &'a [Self] {
        &FilterKind::ALL
    }

    fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
        let name = self.name();
        let mut pv = clap::builder::PossibleValue::new(name);
        pv = match name {
            "triangle" => pv.alias("bilinear").alias("linear"),
            "catmull-rom" => pv.alias("catrom").alias("bicubic"),
            "lanczos3" => pv.alias("lanczos").alias("nuke-lanczos6"),
            "radial-lanczos3" => pv.alias("radial-lanczos"),
            "b-spline" => pv.alias("bspline"),
            _ => pv,
        };
        Some(pv)
    }
}

/// A configured OIIO filter: a [`FilterKind`] plus its runtime width/height.
///
/// Mirrors OIIO's `Filter2D` — width and height are mutable fields, not derived, because callers
/// (notably `resize`'s magnify path) scale them after construction.
#[derive(Clone, Copy, Debug)]
pub struct FilterDesc {
    /// Full support width along X, in source-pixel units (i.e. twice the radius).
    pub width: f32,
    /// Full support width along Y.
    pub height: f32,
    /// Which named filter this is.
    pub kind: FilterKind,
}

impl FilterDesc {
    /// Construct a filter at its own [`FilterKind::default_width`] (isotropic — width equals
    /// height). The direct replacement for the old reduced `Filter` enum's fixed-support values:
    /// `FilterDesc::new(FilterKind::Lanczos3)` where a caller used to write `Filter::Lanczos3`.
    #[must_use]
    pub const fn new(kind: FilterKind) -> Self {
        let w = kind.default_width();
        FilterDesc {
            width: w,
            height: w,
            kind,
        }
    }

    /// Whether this filter is a product of independent X and Y terms.
    ///
    /// `false` only for [`FilterKind::RadialLanczos3`] and [`FilterKind::Disk`] — both are
    /// defined on the 2D radius directly and cannot be evaluated as `xfilt(x) * yfilt(y)`. A
    /// caller (or a GPU backend, which can only dispatch a separable two-pass kernel) MUST check
    /// this before assuming [`FilterDesc::weight`] factors.
    #[must_use]
    pub fn separable(&self) -> bool {
        !matches!(self.kind, FilterKind::RadialLanczos3 | FilterKind::Disk)
    }

    /// The X-axis 1D kernel at signed distance `x`.
    ///
    /// Meaningless for a non-separable kind (see [`FilterDesc::separable`]) beyond what
    /// [`FilterDesc::weight`] uses it for internally; callers should not treat this in isolation
    /// as "the filter" unless [`FilterDesc::separable`] is `true`.
    #[must_use]
    pub fn xfilt(&self, x: f32) -> f32 {
        match self.kind {
            FilterKind::Nearest => {
                if x.abs() < self.width * 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
            FilterKind::Box => {
                let half_w = self.width * 0.5;
                if x.abs() <= half_w { 1.0 } else { 0.0 }
            }
            FilterKind::Triangle => triangle_1d(x * (2.0 / self.width)),
            FilterKind::Gaussian => gaussian_1d(x * (2.0 / self.width)),
            FilterKind::SharpGaussian => sharp_gaussian_1d(x * (2.0 / self.width)),
            FilterKind::CatmullRom => catmull_rom_1d(x * (4.0 / self.width)),
            FilterKind::BlackmanHarris => blackman_harris_1d(x * (2.0 / self.width)),
            FilterKind::Sinc => sinc_1d(x, self.width * 0.5),
            FilterKind::Lanczos3 => lanczos3_1d_oiio(x * (6.0 / self.width)),
            FilterKind::RadialLanczos3 => lanczos3_1d_oiio(x * (6.0 / self.width)),
            FilterKind::Mitchell => mitchell_1d(x * (2.0 / self.width)),
            FilterKind::BSpline => bspline_1d(x * (4.0 / self.width)),
            FilterKind::Disk => {
                let half_w = self.width * 0.5;
                if x.abs() <= half_w { 1.0 } else { 0.0 }
            }
            FilterKind::Cubic { a } => cubic_1d(x * (2.0 / self.width), a),
        }
    }

    /// The Y-axis 1D kernel at signed distance `y`. See [`FilterDesc::xfilt`].
    #[must_use]
    pub fn yfilt(&self, y: f32) -> f32 {
        match self.kind {
            FilterKind::Nearest => {
                if y.abs() < self.height * 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
            FilterKind::Box => {
                let half_h = self.height * 0.5;
                if y.abs() <= half_h { 1.0 } else { 0.0 }
            }
            FilterKind::Triangle => triangle_1d(y * (2.0 / self.height)),
            FilterKind::Gaussian => gaussian_1d(y * (2.0 / self.height)),
            FilterKind::SharpGaussian => sharp_gaussian_1d(y * (2.0 / self.height)),
            FilterKind::CatmullRom => catmull_rom_1d(y * (4.0 / self.height)),
            FilterKind::BlackmanHarris => blackman_harris_1d(y * (2.0 / self.height)),
            FilterKind::Sinc => sinc_1d(y, self.height * 0.5),
            FilterKind::Lanczos3 => lanczos3_1d_oiio(y * (6.0 / self.height)),
            FilterKind::RadialLanczos3 => lanczos3_1d_oiio(y * (6.0 / self.height)),
            FilterKind::Mitchell => mitchell_1d(y * (2.0 / self.height)),
            FilterKind::BSpline => bspline_1d(y * (4.0 / self.height)),
            FilterKind::Disk => {
                let half_h = self.height * 0.5;
                if y.abs() <= half_h { 1.0 } else { 0.0 }
            }
            FilterKind::Cubic { a } => cubic_1d(y * (2.0 / self.height), a),
        }
    }

    /// The full 2D kernel at `(x, y)`.
    ///
    /// For a separable kind this is `xfilt(x) * yfilt(y)`; [`FilterKind::RadialLanczos3`] and
    /// [`FilterKind::Disk`] evaluate the 2D formula directly instead (see
    /// [`FilterDesc::separable`]).
    #[must_use]
    pub fn weight(&self, x: f32, y: f32) -> f32 {
        match self.kind {
            FilterKind::RadialLanczos3 => {
                let rx = x * (6.0 / self.width);
                let ry = y * (6.0 / self.height);
                lanczos3_1d_oiio((rx * rx + ry * ry).sqrt())
            }
            FilterKind::Disk => {
                let rx = x / (self.width * 0.5);
                let ry = y / (self.height * 0.5);
                if rx * rx + ry * ry < 1.0 { 1.0 } else { 0.0 }
            }
            _ => self.xfilt(x) * self.yfilt(y),
        }
    }
}

/// Look up an OIIO filter by name and construct it at the given width.
///
/// `filtername` empty means "use `default_name` instead" (OIIO's own convention — see
/// `get_resize_filter`/`get_warp_filter` in `imagebufalgo_xform.cpp`, which resolve an empty name
/// against a direction-dependent default before ever reaching the registry lookup this function
/// performs). `filterwidth <= 0.0` means "use this filter's own default width" (OIIO: `fwidth > 0
/// ? fwidth : fd.width`); both axes take the same value at construction — a caller that needs to
/// scale one axis differently (as `resize` does for its magnify path) mutates
/// [`FilterDesc::width`]/[`FilterDesc::height`] afterward, same as OIIO's `Filter2D` callers do.
///
/// Name resolution goes through [`FilterKind::from_name`] — one parser, not two.
///
/// Returns `None` for an unrecognised name; unlike [`FilterKind::from_name`] via `FromStr` this
/// has no caller-facing error channel of its own (OIIO's equivalent writes to an `ImageBuf`'s
/// error string, which this crate does not have a type for) — the caller names the rejected
/// string itself if it wants to report one, using
/// `if filtername.is_empty() { default_name } else { filtername }` to know which string was
/// actually looked up.
#[must_use]
pub fn filter_desc_from_name(
    filtername: &str,
    filterwidth: f32,
    default_name: &str,
) -> Option<FilterDesc> {
    let name = if filtername.is_empty() {
        default_name
    } else {
        filtername
    };
    let kind = FilterKind::from_name(name)?;
    let w = if filterwidth > 0.0 {
        filterwidth
    } else {
        kind.default_width()
    };
    Some(FilterDesc {
        width: w,
        height: w,
        kind,
    })
}

/// OIIO's `resize_`/`get_resize_filter` default-filter rule: `"blackman-harris"` when either axis
/// magnifies (`ratio > 1.0`), `"lanczos3"` otherwise (`imagebufalgo_xform.cpp:836-841`, verified
/// against the reference — earlier bughunt notes on this rule had the two names backwards).
///
/// Only applies when the caller supplied no filter name; a caller that named a filter uses it
/// regardless of direction.
#[must_use]
pub fn default_resize_filter_name(xratio: f32, yratio: f32) -> &'static str {
    if xratio > 1.0 || yratio > 1.0 {
        "blackman-harris"
    } else {
        "lanczos3"
    }
}

/// OIIO's `get_warp_filter` default: always `"lanczos3"` regardless of direction
/// (`imagebufalgo_xform.cpp:334` — warp has no magnify/minify split the way resize does).
#[must_use]
pub const fn default_warp_filter_name() -> &'static str {
    "lanczos3"
}

// =============================================================================
// The kernels themselves
//
// The vocabulary above named these filters in one place; these functions are what the names
// MEAN. `cubic_bc`/`catmull_rom_weight`/`mitchell_weight`/`lanczos_weight`/`lanczos3_weight` are
// consumed DIRECTLY (not through `FilterDesc`) by `vfx-compute`'s warp sampler (CPU
// `cpu_backend.rs`, and hand-copied — pinned by a textual-agreement test — into its WGSL and CUDA
// kernels) and by `vfx-io`'s texture mipmap generator (`imagebufalgo/texture.rs`, parameterised
// Lanczos radius), so they stay as free functions with stable names/signatures even though
// `FilterDesc`'s own kernels (`catmull_rom_1d`/`mitchell_1d`/`lanczos3_1d_oiio`/...) duplicate
// their SHAPE at a different absolute scale for Catmull-Rom specifically (documented at each pair
// below) — collapsing the two would require touching GPU shader source that this crate cannot
// itself keep in sync with a Rust-side test, and no caller of either needs it collapsed.
//
// The argument order is `(x, b, c)` — x first, like every other kernel here. `vfx-warp`'s copy
// took `(b, c, x)`, all three `f32`, so a call moved between files compiled silently and evaluated
// the kernel at `b`. Removing that trap is the main reason these functions were unified in the
// first place.
// =============================================================================

/// Mitchell-Netravali cubic family, parameterised by B and C.
///
/// One function for both cubics `vfx-compute`'s warp sampler uses: they differ only in (B, C).
/// Support is |x| < 2; outside it the kernel is exactly zero.
#[inline]
#[must_use]
pub fn cubic_bc(x: f32, b: f32, c: f32) -> f32 {
    let ax = x.abs();
    if ax < 1.0 {
        ((12.0 - 9.0 * b - 6.0 * c) * ax * ax * ax
            + (-18.0 + 12.0 * b + 6.0 * c) * ax * ax
            + (6.0 - 2.0 * b))
            / 6.0
    } else if ax < 2.0 {
        ((-b - 6.0 * c) * ax * ax * ax
            + (6.0 * b + 30.0 * c) * ax * ax
            + (-12.0 * b - 48.0 * c) * ax
            + (8.0 * b + 24.0 * c))
            / 6.0
    } else {
        0.0
    }
}

/// Catmull-Rom (B=0, C=1/2) — what [`FilterKind::CatmullRom`]/`"bicubic"` means, at HALF the
/// absolute scale of [`FilterDesc`]'s own `catmull_rom_1d` (OIIO's `catrom1d`) — see this module's
/// doc comment for why both exist and why the difference is harmless everywhere it is used
/// (normalization).
///
/// Interpolating: `catmull_rom_weight(0.0)` is exactly 1, so a sample landing on a pixel centre
/// returns that pixel unchanged.
#[inline]
#[must_use]
pub fn catmull_rom_weight(x: f32) -> f32 {
    cubic_bc(x, 0.0, 0.5)
}

/// Mitchell-Netravali (B=C=1/3) — what [`FilterKind::Mitchell`] means. Numerically IDENTICAL to
/// `FilterDesc`'s own `mitchell_1d` at `FilterKind::Mitchell`'s default width (verified —
/// `mitchell_matches_the_reduced_vocabulary_exactly` in this module's tests); unlike Catmull-Rom,
/// there is no scale difference here.
///
/// NOT interpolating: `mitchell_weight(0.0)` is 8/9, not 1. That is correct and is the usual
/// surprise — a Mitchell resample of an impulse returns less than the impulse.
#[inline]
#[must_use]
pub fn mitchell_weight(x: f32) -> f32 {
    cubic_bc(x, 1.0 / 3.0, 1.0 / 3.0)
}

/// Windowed sinc with `a` lobes: `sinc(pi x) * sinc(pi x / a)`.
///
/// Kept parameterised because `vfx-io`'s texture mipmap generator (`imagebufalgo/texture.rs`)
/// calls this with a runtime radius, not a fixed 3; [`lanczos3_weight`] is the cheap name for the
/// `a = 3` case `vfx-compute`'s warp sampler and most resize callers actually use.
///
/// The near-zero guard is not cosmetic — `sin(pi x) / (pi x)` is 0/0 at the origin, where the
/// limit is 1.
#[inline]
#[must_use]
pub fn lanczos_weight(x: f32, a: f32) -> f32 {
    let ax = x.abs();
    if ax < 0.0001 {
        return 1.0;
    }
    if ax >= a {
        return 0.0;
    }
    let pix = core::f32::consts::PI * x;
    let pix_a = pix / a;
    (pix.sin() / pix) * (pix_a.sin() / pix_a)
}

/// Lanczos-3 — what [`FilterKind::Lanczos3`] means. Support is |x| < 3. Numerically IDENTICAL to
/// `FilterDesc`'s own `lanczos3_1d_oiio` (algebraic identity, verified — `lanczos3_desc_matches_
/// lanczos3_weight` in this module's tests).
#[inline]
#[must_use]
pub fn lanczos3_weight(x: f32) -> f32 {
    lanczos_weight(x, 3.0)
}

// -- Kernels for the OIIO vocabulary, ported byte-for-byte from `filter.cpp` via `vfx-io`'s prior
// transcription (`imagebufalgo/filter2d.rs`). Deliberately NOT unified with the free functions
// above (`cubic_bc`/`mitchell_weight`/`catmull_rom_weight`/`lanczos3_weight`): those differ from
// OIIO's absolute scale for Catmull-Rom by a normalization-invariant constant, and this crate
// preserves OIIO's own scale here rather than reuse them, so a future non-normalizing caller of
// `FilterDesc` does not inherit a silent factor-of-N surprise. `lanczos3_1d_oiio` is numerically
// identical to `lanczos3_weight` (verified) and stays separate for the same reason — one path,
// unconditionally OIIO's own derivation, rather than "these two happen to agree today".

fn triangle_1d(x: f32) -> f32 {
    let x = x.abs();
    if x < 1.0 { 1.0 - x } else { 0.0 }
}

fn gaussian_1d(x: f32) -> f32 {
    let x = x.abs();
    if x < 1.0 { (-2.0 * x * x).exp() } else { 0.0 }
}

fn sharp_gaussian_1d(x: f32) -> f32 {
    let x = x.abs();
    if x < 1.0 { (-4.0 * x * x).exp() } else { 0.0 }
}

fn catmull_rom_1d(x: f32) -> f32 {
    let x = x.abs();
    let x2 = x * x;
    let x3 = x * x2;
    if x >= 2.0 {
        0.0
    } else if x < 1.0 {
        3.0 * x3 - 5.0 * x2 + 2.0
    } else {
        -x3 + 5.0 * x2 - 8.0 * x + 4.0
    }
}

fn blackman_harris_1d(x: f32) -> f32 {
    if !(-1.0..=1.0).contains(&x) {
        return 0.0;
    }
    let x = (x + 1.0) * 0.5;
    let a0 = 0.35875f32;
    let a1 = -0.48829f32;
    let a2 = 0.14128f32;
    let a3 = -0.01168f32;
    let pi = core::f32::consts::PI;
    let cos2pix = (2.0 * pi * x).cos();
    let cos4pix = 2.0 * cos2pix * cos2pix - 1.0;
    let cos6pix = cos2pix * (2.0 * cos4pix - 1.0);
    a0 + a1 * cos2pix + a2 * cos4pix + a3 * cos6pix
}

fn sinc_1d(x: f32, rad: f32) -> f32 {
    let x = x.abs();
    if x > rad {
        return 0.0;
    }
    if x < 0.0001 {
        1.0
    } else {
        let pix = core::f32::consts::PI * x;
        pix.sin() / pix
    }
}

/// OIIO's own `lanczos3` derivation (triple-angle identity). See the module note above for why
/// this stays separate from [`lanczos3_weight`] despite being numerically identical to it.
fn lanczos3_1d_oiio(x: f32) -> f32 {
    let a = 3.0f32;
    let ainv = 1.0f32 / a;
    let x = x.abs();
    if x > a {
        return 0.0;
    }
    if x < 0.0001 {
        return 1.0;
    }
    let pi = core::f32::consts::PI;
    let s1 = (x * ainv * pi).sin();
    let s3 = (-4.0 * s1 * s1 + 3.0) * s1;
    a / (x * x * (pi * pi)) * s1 * s3
}

fn mitchell_1d(x: f32) -> f32 {
    let mut x = x.abs();
    if x > 1.0 {
        return 0.0;
    }
    x *= 2.0;
    let x2 = x * x;
    let b = 1.0 / 3.0;
    let c = 1.0 / 3.0;
    let sixth = 1.0 / 6.0;
    if x >= 1.0 {
        ((-b - 6.0 * c) * x * x2
            + (6.0 * b + 30.0 * c) * x2
            + (-12.0 * b - 48.0 * c) * x
            + (8.0 * b + 24.0 * c))
            * sixth
    } else {
        ((12.0 - 9.0 * b - 6.0 * c) * x * x2 + (-18.0 + 12.0 * b + 6.0 * c) * x2 + (6.0 - 2.0 * b))
            * sixth
    }
}

fn bspline_1d(x: f32) -> f32 {
    let x = x.abs();
    if x <= 1.0 {
        bspline_b1(1.0 - x)
    } else if x < 2.0 {
        bspline_b0(2.0 - x)
    } else {
        0.0
    }
}

fn bspline_b0(t: f32) -> f32 {
    t * t * t / 6.0
}

fn bspline_b1(t: f32) -> f32 {
    0.5 * t * (t * (1.0 - t) + 1.0) + 1.0 / 6.0
}

fn cubic_1d(x: f32, a: f32) -> f32 {
    let mut x = x.abs();
    if x > 1.0 {
        return 0.0;
    }
    x *= 2.0;
    if x >= 1.0 {
        a * (x * (x * (x - 5.0) + 8.0) - 4.0)
    } else {
        x * x * ((a + 2.0) * x - (a + 3.0)) + 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // FilterKind — name parsing, GPU indices, defaults
    // =========================================================================

    #[test]
    fn compute_index_matches_the_original_resize_filter_numbering() {
        assert_eq!(FilterKind::Nearest.compute_index(), Some(0));
        assert_eq!(FilterKind::Triangle.compute_index(), Some(1));
        assert_eq!(FilterKind::CatmullRom.compute_index(), Some(2));
        assert_eq!(FilterKind::Lanczos3.compute_index(), Some(3));
    }

    #[test]
    fn compute_index_round_trips() {
        for f in [
            FilterKind::Nearest,
            FilterKind::Triangle,
            FilterKind::CatmullRom,
            FilterKind::Lanczos3,
        ] {
            assert_eq!(
                FilterKind::from_compute_index(f.compute_index().unwrap()),
                Some(f),
                "{f}"
            );
        }
        assert_eq!(FilterKind::from_compute_index(4), None);
    }

    #[test]
    fn mitchell_has_no_compute_kernel_and_says_so() {
        assert_eq!(FilterKind::Mitchell.compute_index(), None);
    }

    /// Every OIIO filter beyond the old reduced 5-kernel set has no GPU kernel on EITHER backend
    /// — this is the "explicit refusal, not silent substitution" contract the module docs
    /// describe.
    #[test]
    fn the_eight_new_oiio_filters_have_no_gpu_kernel_anywhere() {
        for f in [
            FilterKind::Box,
            FilterKind::Gaussian,
            FilterKind::SharpGaussian,
            FilterKind::BlackmanHarris,
            FilterKind::Sinc,
            FilterKind::RadialLanczos3,
            FilterKind::BSpline,
            FilterKind::Disk,
            FilterKind::Cubic { a: 0.0 },
        ] {
            assert_eq!(
                f.compute_index(),
                None,
                "{f:?} should have no compute kernel"
            );
        }
    }

    #[test]
    fn every_alias_the_predecessor_parsers_accepted_still_parses() {
        for (s, want) in [
            ("nearest", FilterKind::Nearest),
            ("NEAREST", FilterKind::Nearest),
            ("box", FilterKind::Box),
            (" bilinear ", FilterKind::Triangle),
            ("linear", FilterKind::Triangle),
            ("triangle", FilterKind::Triangle),
            ("catmull-rom", FilterKind::CatmullRom),
            ("catrom", FilterKind::CatmullRom),
            ("mitchell", FilterKind::Mitchell),
            ("lanczos", FilterKind::Lanczos3),
            ("lanczos3", FilterKind::Lanczos3),
        ] {
            assert_eq!(FilterKind::from_name(s), Some(want), "{s}");
        }
    }

    /// The two deliberate corrections made merging the vocabularies — see the module doc comment.
    #[test]
    fn nearest_resolves_to_the_open_interval_point_sampler_not_box() {
        assert_eq!(FilterKind::from_name("nearest"), Some(FilterKind::Nearest));
        assert_eq!(FilterKind::from_name("box"), Some(FilterKind::Box));
        assert_ne!(FilterKind::Nearest, FilterKind::Box);
    }

    #[test]
    fn bicubic_resolves_to_catmull_rom_not_plain_cubic() {
        assert_eq!(
            FilterKind::from_name("bicubic"),
            Some(FilterKind::CatmullRom)
        );
        assert_eq!(
            FilterKind::from_name("cubic"),
            Some(FilterKind::Cubic { a: 0.0 })
        );
        assert_ne!(
            FilterKind::from_name("bicubic"),
            FilterKind::from_name("cubic")
        );
    }

    #[test]
    fn names_that_never_worked_now_fail_instead_of_pretending() {
        // `kaiser` ran Lanczos in the old maketx parser.
        assert_eq!(FilterKind::from_name("kaiser"), None);
        assert_eq!(FilterKind::from_name("lanczos5"), None);
    }

    #[test]
    fn distinct_filters_stay_distinct() {
        // Guards the failure mode a "does it parse" test cannot see: a mapping that collapses
        // every name onto one kernel.
        let mut seen = std::collections::HashSet::new();
        for i in FilterKind::ALL.iter().filter_map(|f| f.compute_index()) {
            assert!(seen.insert(i), "two filters share compute index {i}");
        }
    }

    #[test]
    fn round_trips_through_its_own_name() {
        for f in FilterKind::ALL {
            assert_eq!(FilterKind::from_name(f.name()), Some(f), "{f}");
        }
    }

    /// Two parsers exist — `clap::ValueEnum` for CLI arguments and [`FilterKind::from_name`] for
    /// everyone else. They must accept the same words, or `oiio resize --filter linear` and a
    /// config file saying `linear` disagree.
    #[cfg(feature = "clap")]
    #[test]
    fn clap_and_from_name_accept_the_same_vocabulary() {
        use clap::ValueEnum;
        for spelling in [
            "nearest",
            "box",
            "triangle",
            "bilinear",
            "linear",
            "catmull-rom",
            "catrom",
            "bicubic",
            "lanczos3",
            "lanczos",
            "mitchell",
            "cubic",
            "keys",
            "simon",
            "rifman",
        ] {
            let via_clap = FilterKind::from_str(spelling, /* ignore_case */ true)
                .unwrap_or_else(|_| panic!("clap rejected '{spelling}'"));
            let via_name = FilterKind::from_name(spelling)
                .unwrap_or_else(|| panic!("from_name rejected '{spelling}'"));
            assert_eq!(via_clap, via_name, "parsers disagree on '{spelling}'");
        }
    }

    #[cfg(feature = "clap")]
    #[test]
    fn clap_value_list_covers_every_canonical_variant() {
        use clap::ValueEnum;
        assert_eq!(
            <FilterKind as ValueEnum>::value_variants().len(),
            FilterKind::ALL.len()
        );
    }

    #[test]
    fn default_widths_match_filter_cpp() {
        let w = |name: &str| filter_desc_from_name(name, 0.0, "").unwrap().width;
        assert_eq!(w("nearest"), 1.0);
        assert_eq!(w("box"), 1.0);
        assert_eq!(w("triangle"), 2.0);
        assert_eq!(w("bilinear"), 2.0);
        assert_eq!(w("gaussian"), 3.0);
        assert_eq!(w("sharp-gaussian"), 2.0);
        assert_eq!(w("catmull-rom"), 4.0);
        assert_eq!(w("catrom"), 4.0);
        assert_eq!(w("bicubic"), 4.0);
        assert_eq!(w("blackman-harris"), 3.0);
        assert_eq!(w("sinc"), 4.0);
        assert_eq!(w("lanczos3"), 6.0);
        assert_eq!(w("lanczos"), 6.0);
        assert_eq!(w("nuke-lanczos6"), 6.0);
        assert_eq!(w("radial-lanczos3"), 6.0);
        assert_eq!(w("mitchell"), 4.0);
        assert_eq!(w("b-spline"), 4.0);
        assert_eq!(w("bspline"), 4.0);
        assert_eq!(w("disk"), 1.0);
        assert_eq!(w("cubic"), 4.0);
        assert_eq!(w("keys"), 4.0);
        assert_eq!(w("simon"), 4.0);
        assert_eq!(w("rifman"), 4.0);
    }

    #[test]
    fn filter_desc_new_matches_default_width() {
        for f in [
            FilterKind::Nearest,
            FilterKind::Triangle,
            FilterKind::CatmullRom,
            FilterKind::Mitchell,
            FilterKind::Lanczos3,
        ] {
            let d = FilterDesc::new(f);
            assert_eq!(d.width, f.default_width());
            assert_eq!(d.height, f.default_width());
            assert_eq!(d.kind, f);
        }
    }

    #[test]
    fn unrecognised_name_is_none() {
        assert!(filter_desc_from_name("box-of-crayons", 0.0, "").is_none());
    }

    #[test]
    fn empty_name_resolves_the_default() {
        let d = filter_desc_from_name("", 0.0, "lanczos3").unwrap();
        assert_eq!(d.kind, FilterKind::Lanczos3);
        let d = filter_desc_from_name("", 0.0, "blackman-harris").unwrap();
        assert_eq!(d.kind, FilterKind::BlackmanHarris);
    }

    #[test]
    fn explicit_width_overrides_the_default() {
        let d = filter_desc_from_name("lanczos3", 10.0, "").unwrap();
        assert_eq!(d.width, 10.0);
        assert_eq!(d.height, 10.0);
    }

    #[test]
    fn resize_default_matches_get_resize_filter() {
        // imagebufalgo_xform.cpp:836-841 — `wratio > 1 || hratio > 1` (magnify on EITHER axis)
        // picks blackman-harris; otherwise (minify or 1:1 on both) lanczos3. Direction matters:
        // an earlier draft of this port had the two names backwards.
        assert_eq!(default_resize_filter_name(2.0, 0.5), "blackman-harris");
        assert_eq!(default_resize_filter_name(0.5, 2.0), "blackman-harris");
        assert_eq!(default_resize_filter_name(2.0, 2.0), "blackman-harris");
        assert_eq!(default_resize_filter_name(0.5, 0.5), "lanczos3");
        assert_eq!(default_resize_filter_name(1.0, 1.0), "lanczos3");
        assert_eq!(default_resize_filter_name(1.0, 0.5), "lanczos3");
    }

    #[test]
    fn warp_default_is_always_lanczos3() {
        assert_eq!(default_warp_filter_name(), "lanczos3");
    }

    /// `Nearest`'s OPEN interval vs `Box`'s CLOSED interval, at the boundary. Not asserted equal
    /// anywhere — this is the deliberate distinction the module docs describe.
    #[test]
    fn nearest_and_box_disagree_only_at_the_boundary() {
        let nearest = FilterDesc::new(FilterKind::Nearest);
        let boxf = FilterDesc::new(FilterKind::Box);
        for x in [-1.2f32, -0.1, 0.0, 0.3, 0.49, 0.9] {
            assert_eq!(
                nearest.xfilt(x),
                boxf.xfilt(x),
                "away from the boundary at {x}"
            );
        }
        assert_eq!(
            nearest.xfilt(0.5),
            0.0,
            "Nearest: open interval excludes 0.5"
        );
        assert_eq!(boxf.xfilt(0.5), 1.0, "Box: closed interval includes 0.5");
    }

    /// `filter.cpp`'s `FilterGaussian1D::gaussian1d` is `exp(-2x^2)` for `|x|<1` in the filter's
    /// OWN rescaled coordinate — `xfilt` feeds it `x * (2.0/width)`, and the default width is 3.0,
    /// so the support in ORIGINAL x is `|x| < width/2 = 1.5`, not `|x| < 1`.
    #[test]
    fn gaussian_matches_its_closed_form() {
        let g = filter_desc_from_name("gaussian", 0.0, "").unwrap();
        assert_eq!(g.width, 3.0);
        for x in [0.0f32, 0.25, 0.5, 0.75, 1.4] {
            let rescaled = x * (2.0 / g.width);
            let want = (-2.0 * rescaled * rescaled).exp();
            assert!((g.xfilt(x) - want).abs() < 1e-6, "gaussian at {x}");
            assert!((g.xfilt(-x) - want).abs() < 1e-6, "gaussian at {}", -x);
        }
        assert_eq!(g.xfilt(1.5), 0.0, "support edge, width/2");
        assert_eq!(g.xfilt(2.0), 0.0);
    }

    /// Sharp-gaussian is `exp(-4x^2)` — narrower than plain gaussian at every nonzero x.
    #[test]
    fn sharp_gaussian_is_narrower_than_gaussian() {
        let g = filter_desc_from_name("gaussian", 0.0, "").unwrap();
        let sg = filter_desc_from_name("sharp-gaussian", 0.0, "").unwrap();
        for x in [0.1f32, 0.3, 0.6, 0.9] {
            let want = (-4.0 * x * x).exp();
            assert!((sg.xfilt(x) - want).abs() < 1e-6, "sharp-gaussian at {x}");
            assert!(
                sg.xfilt(x) < g.xfilt(x),
                "sharp-gaussian must be narrower at {x}"
            );
        }
        assert_eq!(sg.xfilt(0.0), 1.0);
    }

    /// `FilterCatmullRom1D::catrom1d` (`filter.cpp:254-260`): piecewise cubic on `x` directly
    /// (default width 4 makes the caller's rescale the identity), NOT the same absolute scale as
    /// this crate's `catmull_rom_weight` (which is `cubic_bc(x,0,0.5)`, exactly half of OIIO's
    /// value at every x — see the module note; both are correct, they normalize to the same
    /// resample result, this test pins OIIO's own scale specifically).
    #[test]
    fn catmull_rom_matches_catrom1d_exactly() {
        let f = filter_desc_from_name("catmull-rom", 0.0, "").unwrap();
        assert!((f.xfilt(0.0) - 2.0).abs() < 1e-6, "catrom1d(0) = 2");
        assert!((f.xfilt(0.5) - 1.125).abs() < 1e-5, "catrom1d(0.5)");
        assert!((f.xfilt(1.0) - 0.0).abs() < 1e-6, "catrom1d(1) = 0");
        assert!((f.xfilt(1.5) - (-0.125)).abs() < 1e-5, "catrom1d(1.5)");
        assert_eq!(f.xfilt(2.0), 0.0);
        assert_eq!(f.xfilt(-2.0), 0.0);
        // Exactly double `catmull_rom_weight` at every sampled x (documented, not incidental).
        for x in [0.0f32, 0.3, 0.5, 1.0, 1.5, 1.9] {
            assert!(
                (f.xfilt(x) - 2.0 * catmull_rom_weight(x)).abs() < 1e-5,
                "catmull-rom scale relationship broke at {x}"
            );
        }
    }

    /// `FilterMitchell1D::mitchell1d` at default width 4: the external `2/width` rescale and the
    /// internal `x*=2` cancel, landing on the exact same domain and coefficients as this crate's
    /// `mitchell_weight` — unlike Catmull-Rom, there is NO scale difference here.
    #[test]
    fn mitchell_matches_the_reduced_vocabulary_exactly() {
        let f = filter_desc_from_name("mitchell", 0.0, "").unwrap();
        for x in [0.0f32, 0.3, 0.75, 1.0, 1.5, 1.9] {
            assert!(
                (f.xfilt(x) - mitchell_weight(x)).abs() < 1e-5,
                "mitchell at {x}: OIIO path vs reduced-vocabulary path disagree"
            );
        }
        assert!((f.xfilt(0.0) - 8.0 / 9.0).abs() < 1e-5);
    }

    /// Lanczos3 through the OIIO-vocabulary path must equal `lanczos3_weight` — proven
    /// algebraically identical (triple-angle identity), pinned numerically here.
    #[test]
    fn lanczos3_desc_matches_lanczos3_weight() {
        let f = filter_desc_from_name("lanczos3", 0.0, "").unwrap();
        for x in [
            -2.9f32, -1.5, -0.3, 0.0, 0.0001, 0.5, 1.0, 2.0, 2.99, 3.0, 4.0,
        ] {
            assert!(
                (f.xfilt(x) - lanczos3_weight(x)).abs() < 1e-5,
                "lanczos3 OIIO-vocab vs reduced-vocab at {x}"
            );
        }
    }

    /// `nuke-lanczos6` and `radial-lanczos3` are named differently but share lanczos3's 1D
    /// kernel; `radial-lanczos3` additionally must be non-separable.
    #[test]
    fn lanczos_aliases_and_radial_variant() {
        let a = filter_desc_from_name("lanczos3", 0.0, "").unwrap();
        let b = filter_desc_from_name("nuke-lanczos6", 0.0, "").unwrap();
        for x in [0.0f32, 1.0, 2.5] {
            assert_eq!(a.xfilt(x), b.xfilt(x));
        }
        let radial = filter_desc_from_name("radial-lanczos3", 0.0, "").unwrap();
        assert!(!radial.separable());
        assert!(a.separable());
        // On-axis (y=0) the radial and separable forms must agree: sqrt(x^2+0^2) = |x|.
        for x in [0.0f32, 1.2, 2.9] {
            assert!(
                (radial.weight(x, 0.0) - a.xfilt(x)).abs() < 1e-5,
                "radial-lanczos3 on-axis vs lanczos3 at {x}"
            );
        }
    }

    /// `FilterSinc1D::sinc1d` (`filter.cpp:388-395`): `sin(pi*x)/(pi*x)`, radius = width/2.
    #[test]
    fn sinc_matches_its_closed_form() {
        let f = filter_desc_from_name("sinc", 0.0, "").unwrap();
        assert_eq!(f.width, 4.0);
        assert!((f.xfilt(0.0) - 1.0).abs() < 1e-6);
        for x in [0.5f32, 1.0, 1.5, 1.9999] {
            let pix = core::f32::consts::PI * x;
            let want = pix.sin() / pix;
            assert!((f.xfilt(x) - want).abs() < 1e-5, "sinc at {x}");
        }
        // Radius is width/2 = 2.0, so it must vanish beyond that, not at width=4.
        assert_eq!(f.xfilt(2.1), 0.0);
    }

    /// `FilterBlackmanHarris1D` (`filter.cpp:301-345`): the 4-term window, `a0..a3` from the
    /// reference. Checked at the endpoints (where all cosines are known) and the center.
    #[test]
    fn blackman_harris_matches_its_closed_form() {
        let f = filter_desc_from_name("blackman-harris", 0.0, "").unwrap();
        let want_at_zero = 0.35875 + 0.48829 + 0.14128 + 0.01168;
        assert!(
            (f.xfilt(0.0) - want_at_zero).abs() < 1e-4,
            "blackman-harris(0) = {} want {want_at_zero}",
            f.xfilt(0.0)
        );
        assert!(f.xfilt(1.5).abs() < 0.01, "near the support edge");
        assert_eq!(f.xfilt(1.6), 0.0, "outside the support");
    }

    /// `FilterBSpline1D` (`filter.cpp`, Stark et al basis): two cubic pieces, `b1` near the
    /// center and `b0` in the outer ring, each evaluated in `t = 1-|x|` / `t = 2-|x|`.
    #[test]
    fn bspline_matches_its_closed_form() {
        let f = filter_desc_from_name("b-spline", 0.0, "").unwrap();
        // b1(1) = 0.5*1*(1*0+1) + 1/6 = 0.5 + 1/6 = 2/3
        assert!((f.xfilt(0.0) - 2.0 / 3.0).abs() < 1e-5, "bspline(0) = 2/3");
        // b0(1) = 1/6, at |x|=1 (boundary between the two pieces, t=1-1=0 for b1: b1(0)=1/6 too)
        assert!((f.xfilt(1.0) - 1.0 / 6.0).abs() < 1e-5, "bspline(1) = 1/6");
        assert_eq!(f.xfilt(2.0), 0.0);
        assert_eq!(f.xfilt(2.5), 0.0);
        // Symmetric and always non-negative (a genuine B-spline basis function).
        for x in [0.2f32, 0.8, 1.3, 1.9] {
            assert!(
                (f.xfilt(x) - f.xfilt(-x)).abs() < 1e-6,
                "bspline symmetry at {x}"
            );
            assert!(f.xfilt(x) >= 0.0, "bspline must be non-negative at {x}");
        }
    }

    /// The Keys one-parameter cubic family (`filter.cpp`'s `FilterCubic1D`): `a=0` is OIIO's
    /// plain `"cubic"`; `a=-0.5` (`"keys"`) is ALGEBRAICALLY IDENTICAL to the textbook Catmull-Rom
    /// cubic-convolution polynomial (verified by hand: both reduce to `1.5|x|^3-2.5|x|^2+1` for
    /// `|x|<1` and `-0.5|x|^3+2.5|x|^2-4|x|+2` for `1<=|x|<2`).
    ///
    /// **This is `catmull_rom_weight` (this crate's reduced-vocabulary kernel, `cubic_bc(x,0,0.5)`
    /// — un-doubled), NOT `FilterKind::CatmullRom`/`catrom1d`**, which is OIIO's own "catmull-rom"
    /// registry entry and independently carries exactly DOUBLE this scale at every x (see
    /// `catmull_rom_matches_catrom1d_exactly` above). So inside OIIO's own registry, `"keys"` and
    /// `"catmull-rom"` do NOT evaluate to the same numbers, despite `"keys"` being the textbook
    /// definition of the Catmull-Rom curve — a genuine, surprising OIIO-internal inconsistency.
    #[test]
    fn keys_cubic_equals_catmull_rom_weight() {
        let keys = filter_desc_from_name("keys", 0.0, "").unwrap();
        for x in [0.0f32, 0.3, 0.75, 1.0, 1.5, 1.99] {
            assert!(
                (keys.xfilt(x) - catmull_rom_weight(x)).abs() < 1e-4,
                "keys(a=-0.5) vs catmull_rom_weight at {x}: {} vs {}",
                keys.xfilt(x),
                catmull_rom_weight(x)
            );
        }
        let catrom = filter_desc_from_name("catmull-rom", 0.0, "").unwrap();
        for x in [0.0f32, 0.5, 1.5] {
            assert!(
                (keys.xfilt(x) * 2.0 - catrom.xfilt(x)).abs() < 1e-4,
                "keys*2 should equal catrom at {x}"
            );
        }
    }

    /// Plain `"cubic"` (`a=0`) is a DIFFERENT curve from Catmull-Rom/`"keys"` despite sharing a
    /// support radius of 2 — its outer lobe (`1<=|x|<2`) is identically zero because that branch
    /// is `a * (...)`, whereas Catmull-Rom's outer lobe is the nonzero `-x^3+5x^2-8x+4`. This is
    /// also why `"bicubic"` resolves to `FilterKind::CatmullRom`, not `Cubic{a:0}` — see the
    /// module doc comment.
    #[test]
    fn plain_cubic_has_a_zero_outer_lobe_unlike_catmull_rom() {
        let cubic = filter_desc_from_name("cubic", 0.0, "").unwrap();
        let catrom = filter_desc_from_name("catmull-rom", 0.0, "").unwrap();
        assert_eq!(
            cubic.xfilt(1.5),
            0.0,
            "plain cubic's outer lobe is flat zero"
        );
        assert!(
            catrom.xfilt(1.5) != 0.0,
            "catmull-rom's outer lobe is NOT zero — {}",
            catrom.xfilt(1.5)
        );
        for x in [0.0f32, 0.3, 0.7, 0.99] {
            let want = x * x * (2.0 * x - 3.0) + 1.0;
            assert!(
                (cubic.xfilt(x) - want).abs() < 1e-5,
                "plain cubic inner lobe at {x}"
            );
        }
    }

    #[test]
    fn simon_and_rifman_wire_distinct_a_values() {
        let simon = filter_desc_from_name("simon", 0.0, "").unwrap();
        let rifman = filter_desc_from_name("rifman", 0.0, "").unwrap();
        assert_eq!(simon.kind, FilterKind::Cubic { a: -0.75 });
        assert_eq!(rifman.kind, FilterKind::Cubic { a: -1.0 });
        assert_ne!(simon.xfilt(1.3), rifman.xfilt(1.3));
    }

    #[test]
    fn disk_is_flat_and_circular() {
        let f = filter_desc_from_name("disk", 0.0, "").unwrap();
        assert!(!f.separable());
        assert_eq!(f.width, 1.0);
        assert_eq!(f.weight(0.0, 0.0), 1.0);
        assert_eq!(f.weight(0.49, 0.0), 1.0);
        assert_eq!(f.weight(0.0, 0.49), 1.0);
        assert_eq!(f.weight(0.4, 0.4), 0.0);
        assert_eq!(f.weight(0.6, 0.0), 0.0);
    }

    // =========================================================================
    // Kernels — closed forms
    // =========================================================================

    #[test]
    fn the_cubics_match_their_closed_forms() {
        for (name, b, c, at_zero, at_one) in [
            ("catmull-rom", 0.0f32, 0.5f32, 1.0f32, 0.0f32),
            ("mitchell", 1.0 / 3.0, 1.0 / 3.0, 8.0 / 9.0, 1.0 / 18.0),
        ] {
            assert!((cubic_bc(0.0, b, c) - at_zero).abs() < 1e-6, "{name} at 0");
            assert!((cubic_bc(1.0, b, c) - at_one).abs() < 1e-6, "{name} at 1");
            assert!((cubic_bc(-1.0, b, c) - at_one).abs() < 1e-6, "{name} at -1");
            assert_eq!(cubic_bc(2.0, b, c), 0.0, "{name} at its support edge");
            assert_eq!(cubic_bc(-7.5, b, c), 0.0, "{name} well outside support");
            for x in [0.3f32, 0.75, 1.2, 1.9] {
                assert!(
                    (cubic_bc(x, b, c) - cubic_bc(-x, b, c)).abs() < 1e-7,
                    "{name} is not symmetric at {x} — an asymmetric kernel shifts the image"
                );
            }
        }
        assert!((catmull_rom_weight(0.0) - 1.0).abs() < 1e-6);
        assert!((mitchell_weight(0.0) - 8.0 / 9.0).abs() < 1e-6);
        assert!((catmull_rom_weight(0.5) - 0.5625).abs() < 1e-6);
        assert!(
            (catmull_rom_weight(1.5) + 0.0625).abs() < 1e-6,
            "no negative lobe"
        );
    }

    #[test]
    fn lanczos3_is_a_three_lobe_windowed_sinc() {
        assert_eq!(lanczos3_weight(0.0), 1.0);
        for n in [1.0f32, 2.0] {
            assert!(
                lanczos3_weight(n).abs() < 1e-6,
                "should vanish at integer {n}"
            );
            assert!(lanczos3_weight(-n).abs() < 1e-6);
        }
        assert_eq!(lanczos3_weight(3.0), 0.0);
        assert_eq!(lanczos3_weight(-3.0), 0.0);
        assert_eq!(lanczos3_weight(4.7), 0.0);
        assert!((lanczos3_weight(1.5) + 0.13509491).abs() < 1e-6);
        for x in [-3.5f32, -1.7, -0.0001, 0.0, 0.3, 1.5, 2.99, 3.0, 5.0] {
            assert_eq!(lanczos3_weight(x), lanczos_weight(x, 3.0), "at {x}");
        }
    }

    /// PINNED BOUND: the canonical kernel against the pre-merge reference.
    ///
    /// Before unification, `vfx-warp`'s copy precomputed `x3 = (x*x)*x` and wrote `coef * x3`,
    /// while every other copy wrote `coef * ax * ax * ax` — which associates as
    /// `((coef*ax)*ax)*ax`. Identical in exact arithmetic, different rounding in `f32`.
    ///
    /// **Measured on the quantity that reaches a pixel** — the summed absolute difference of
    /// NORMALISED weights over a full tap set:
    ///
    /// * max single raw weight difference: 1.274e-06
    /// * **max summed normalised difference: 3.571e-06**
    ///
    /// The bound is **1e-5**, deliberately below one 16-bit quantisation step (1.5e-5).
    #[test]
    fn the_association_difference_stays_below_a_16_bit_step() {
        fn reference_cubic_bc(b: f32, c: f32, x: f32) -> f32 {
            let x = x.abs();
            if x < 1.0 {
                let x2 = x * x;
                let x3 = x2 * x;
                ((12.0 - 9.0 * b - 6.0 * c) * x3
                    + (-18.0 + 12.0 * b + 6.0 * c) * x2
                    + (6.0 - 2.0 * b))
                    / 6.0
            } else if x < 2.0 {
                let x2 = x * x;
                let x3 = x2 * x;
                ((-b - 6.0 * c) * x3
                    + (6.0 * b + 30.0 * c) * x2
                    + (-12.0 * b - 48.0 * c) * x
                    + (8.0 * b + 24.0 * c))
                    / 6.0
            } else {
                0.0
            }
        }

        const BOUND: f32 = 1e-5;
        let mut worst = 0.0f32;
        for (b, c) in [(0.0f32, 0.5f32), (1.0 / 3.0, 1.0 / 3.0)] {
            for step in 0..=1000 {
                let fx = step as f32 / 1000.0;
                let ours: Vec<f32> = (-1..=2).map(|d| cubic_bc(d as f32 - fx, b, c)).collect();
                let theirs: Vec<f32> = (-1..=2)
                    .map(|d| reference_cubic_bc(b, c, d as f32 - fx))
                    .collect();
                let (sa, sb): (f32, f32) = (ours.iter().sum(), theirs.iter().sum());
                if sa == 0.0 || sb == 0.0 {
                    continue;
                }
                let summed: f32 = ours
                    .iter()
                    .zip(theirs.iter())
                    .map(|(p, q)| (p / sa - q / sb).abs())
                    .sum();
                worst = worst.max(summed);
            }
        }
        assert!(
            worst < BOUND,
            "normalised weight difference {worst:e} exceeds the pinned bound {BOUND:e} — \
             the kernels have diverged, not merely been reformatted"
        );
    }
}
