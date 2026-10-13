//! The ONE texture lookup: OpenImageIO's `texture_lookup*` drivers and `sample_*` samplers,
//! generic over where the texels come from.
//!
//! [`crate::texture_filter`] is the footprint maths (which levels, which probes, which weights).
//! This module is what runs it: the three lookup drivers of `texturesys.cpp` (`NoMIP`, the
//! isotropic `OneLevel`/`Trilinear` path and the anisotropic ellipse path behind `Default`/`Aniso`)
//! and the `closest`/`bilinear`/`bicubic` samplers, including OIIO's black-wrap bookkeeping, `fill`
//! for missing channels and the analytic `dresultds`/`dresultdt`. Texels are read through
//! [`TexelSource`], so the same code serves vfx-io's `TextureSystem` (texels from the
//! `ImageCache`) and vfx-warp's EWA mode (texels from an in-memory MIP chain).
//!
//! What stays with the caller: resolving the file, UDIM, `firstchannel` (the source returns the
//! lookup's four-channel window), `flip_t`, resolving `Wrap::Default` against the file, more than
//! four channels (the caller recurses four at a time, `texturesys.cpp:1540-1560`), and
//! `gray_to_rgb`. Not ported: environment lookups (lat-long pole fade), `sample_border` textures
//! and stochastic sampling (the pure functions support it; no caller enables it).

use crate::texture_filter::{self, Derivs, MipBlend, MipSelect, MipSource, ProbeLine, Wrap};

/// How a lookup uses the MIP chain (OIIO `Tex::MipMode`, `texture.h`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
#[repr(u8)]
pub enum MipMode {
    /// OIIO's default high-quality lookup: the anisotropic ellipse path.
    #[default]
    Default,
    /// Only the highest-resolution level, no MIP-mapping.
    NoMIP,
    /// One MIP level, isotropic footprint.
    OneLevel,
    /// Two blended MIP levels, isotropic footprint.
    Trilinear,
    /// Two blended MIP levels, anisotropic footprint (same path as `Default`).
    Aniso,
}

/// How a lookup interpolates within a MIP level (OIIO `Tex::InterpMode`, `texture.h`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
#[repr(u8)]
pub enum InterpMode {
    /// The closest texel.
    Closest,
    /// Bilinear within a level.
    Bilinear,
    /// Cubic B-spline within a level.
    Bicubic,
    /// Bicubic when magnifying, else bilinear (only the ellipse path distinguishes; the other
    /// paths treat it as bilinear).
    #[default]
    SmartBicubic,
}

/// The scalar `TextureOpt` fields the lookup reads, plus the `legacy_texture_blur` attribute.
///
/// Defaults are OIIO 3.1's (`texture.h` `TextureOpt`, `texture_pvt.h:470-476`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LookupOpt {
    /// MIP mode.
    pub mipmode: MipMode,
    /// Interpolation within a level.
    pub interpmode: InterpMode,
    /// Over-blur rather than alias.
    pub conservative_filter: bool,
    /// Maximum anisotropy.
    pub anisotropic: u16,
    /// Blur along s, in s units.
    pub sblur: f32,
    /// Blur along t, in t units.
    pub tblur: f32,
    /// Multiplier on the s derivatives.
    pub swidth: f32,
    /// Multiplier on the t derivatives.
    pub twidth: f32,
    /// OIIO's `legacy_texture_blur` (add blur to the axes linearly, 3.1's default).
    pub legacy_blur: bool,
}

impl Default for LookupOpt {
    fn default() -> Self {
        Self {
            mipmode: MipMode::Default,
            interpmode: InterpMode::SmartBicubic,
            conservative_filter: true,
            anisotropic: 32,
            sblur: 0.0,
            tblur: 0.0,
            swidth: 1.0,
            twidth: 1.0,
            legacy_blur: true,
        }
    }
}

/// What one lookup resolved before sampling: the channel counts and the wrap modes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LookupContext {
    /// Channels the texture really has from `firstchannel` on, at most `nchannels`.
    pub actualchannels: usize,
    /// Channels the caller asked for, at most 4.
    pub nchannels: usize,
    /// Wrap along s, already resolved against the file (`Wrap::Default` reads as black).
    pub swrap: Wrap,
    /// Wrap along t, already resolved.
    pub twrap: Wrap,
    /// OIIO's `fill`: the value of channels the texture does not have.
    pub fill: f32,
    /// Whether to compute d(result)/ds and d(result)/dt.
    pub derivs: bool,
}

impl LookupContext {
    /// A context for a texture that has every requested channel, with no fill and no
    /// derivatives: the common case of a caller that owns its own RGBA texels.
    #[must_use]
    pub const fn all_channels(nchannels: usize, swrap: Wrap, twrap: Wrap) -> Self {
        Self {
            actualchannels: nchannels,
            nchannels,
            swrap,
            twrap,
            fill: 0.0,
            derivs: false,
        }
    }

    /// `use_fill` of the samplers (`texturesys.cpp:2681`): channels past the texture's own are
    /// requested and the fill value is non-zero.
    fn use_fill(&self) -> bool {
        self.nchannels > self.actualchannels && self.fill != 0.0
    }

    /// `blend0(accum, channel_masks[actualchannels])` (`:2901-2902`): zero every channel the
    /// texture does not have.
    fn mask_channels(&self, acc: &mut Accum) {
        for c in self.actualchannels.min(4)..4 {
            acc.value[c] = 0.0;
            acc.ds[c] = 0.0;
            acc.dt[c] = 0.0;
        }
    }

    /// Adds the fill value to the missing channels, weighted by how much of the footprint landed
    /// inside the texture (`:2903-2907`).
    fn add_fill(&self, acc: &mut Accum, nonfill: f32) {
        let f = (1.0 - nonfill) * self.fill;
        for c in self.actualchannels.min(4)..4 {
            acc.value[c] += f;
        }
    }
}

/// Where a lookup reads its texels.
///
/// `texel` returns the lookup's four-channel window of texel `(x, y)` of MIP level `level`: the
/// channels from the caller's `firstchannel` on, zero past the texture's own channels (they are
/// masked afterwards anyway). `x` and `y` have already been wrapped into `[0, width)` and
/// `[0, height)` of that level by [`Wrap::wrap_texel`]; texels a black wrap excludes are never
/// asked for.
pub trait TexelSource: MipSource {
    /// Why a texel could not be read.
    type Error;
    /// The four-channel texel `(x, y)` of level `level`.
    fn texel(&mut self, level: usize, x: i32, y: i32) -> Result<[f32; 4], Self::Error>;
}

/// A four-channel result and its s/t derivatives (OIIO's `vfloat4` accumulators).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Accum {
    /// The filtered value.
    pub value: [f32; 4],
    /// d(value)/ds.
    pub ds: [f32; 4],
    /// d(value)/dt.
    pub dt: [f32; 4],
}

impl Accum {
    /// `r_sum += lw * r` (`texturesys.cpp:2112-2117`, `:2409-2414`).
    fn add_weighted(&mut self, r: &Self, weight: f32) {
        for c in 0..4 {
            self.value[c] += weight * r.value[c];
            self.ds[c] += weight * r.ds[c];
            self.dt[c] += weight * r.dt[c];
        }
    }
}

/// One probe: texture coordinate and weight.
type Probe = (f32, f32, f32);

// ============================================================================
// Lookup drivers
// ============================================================================

/// One texture lookup: `TextureSystemImpl::texture`'s dispatch on `mipmode`
/// (`texturesys.cpp:1562-1572`) and the lookup it selects.
///
/// `(s, t)` and the derivatives are in texture space (`[0, 1]` spans the image), already flipped
/// if the caller flips t.
pub fn texture_lookup<S: TexelSource + ?Sized>(
    src: &mut S,
    opt: &LookupOpt,
    ctx: &LookupContext,
    s: f32,
    t: f32,
    d: Derivs,
) -> Result<Accum, S::Error> {
    match opt.mipmode {
        MipMode::NoMIP => lookup_nomip(src, opt, ctx, s, t),
        MipMode::OneLevel | MipMode::Trilinear => lookup_trilinear_mipmap(src, opt, ctx, s, t, d),
        MipMode::Default | MipMode::Aniso => lookup_aniso(src, opt, ctx, s, t, d),
    }
}

/// `texture_lookup_nomip` (`texturesys.cpp:1786-1830`): one probe at the finest level.
fn lookup_nomip<S: TexelSource + ?Sized>(
    src: &mut S,
    opt: &LookupOpt,
    ctx: &LookupContext,
    s: f32,
    t: f32,
) -> Result<Accum, S::Error> {
    let level = src.min_level();
    // `sample_functions[]` (`:1801-1807`): SmartBicubic is bilinear here.
    let interp = match opt.interpmode {
        InterpMode::SmartBicubic => InterpMode::Bilinear,
        other => other,
    };
    sample(src, ctx, interp, [(s, t, 1.0)], level)
}

/// `texture_lookup_trilinear_mipmap` (`texturesys.cpp:2046-2139`): an isotropic footprint, one
/// probe per contributing level. Serves `OneLevel` and `Trilinear`.
fn lookup_trilinear_mipmap<S: TexelSource + ?Sized>(
    src: &mut S,
    opt: &LookupOpt,
    ctx: &LookupContext,
    s: f32,
    t: f32,
    d: Derivs,
) -> Result<Accum, S::Error> {
    let d = texture_filter::adjust_width(d, opt.swidth, opt.twidth);
    let filtwidth =
        texture_filter::isotropic_filter_width(d, opt.conservative_filter, opt.sblur, opt.tblur);
    let select = if opt.mipmode == MipMode::OneLevel {
        MipSelect::OneLevel
    } else {
        MipSelect::Blend
    };
    let mut aspect = 1.0;
    let blend = texture_filter::compute_miplevels(
        src,
        select,
        f32::from(opt.anisotropic),
        filtwidth,
        filtwidth,
        &mut aspect,
        None,
    );
    // `sample_functions[]` (`:2080-2087`): SmartBicubic is bilinear here too.
    let interp = match opt.interpmode {
        InterpMode::SmartBicubic => InterpMode::Bilinear,
        other => other,
    };
    let mut sum = Accum::default();
    for (level, weight) in blend.contributions() {
        let r = sample(src, ctx, interp, [(s, t, 1.0)], level)?;
        sum.add_weighted(&r, weight);
    }
    Ok(sum)
}

/// The anisotropic footprint of one lookup: everything `texture_lookup` decides before it reads a
/// texel (`texturesys.cpp:2261-2380`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnisoFootprint {
    /// The one or two MIP levels and their weights.
    pub blend: MipBlend,
    /// The probes along the major axis (offsets from `(s, t)`).
    pub line: ProbeLine,
    /// `int(1 / sfilt)` of the bare derivatives: the s resolution SmartBicubic compares levels to.
    pub naturalsres: i32,
    /// `int(1 / tfilt)` of the bare derivatives.
    pub naturaltres: i32,
}

impl AnisoFootprint {
    /// Which sampler reads `level` under `interpmode`: `SmartBicubic` is cubic at the finest
    /// level or where the level is coarser than half the natural resolution, else bilinear
    /// (`texturesys.cpp:2386-2406`).
    #[must_use]
    pub fn interp_at<M: MipSource + ?Sized>(
        &self,
        src: &M,
        interpmode: InterpMode,
        level: usize,
    ) -> InterpMode {
        match interpmode {
            InterpMode::SmartBicubic => {
                let (w, h) = src.level_size(level);
                if level == 0
                    || (w as i32) < self.naturalsres / 2
                    || (h as i32) < self.naturaltres / 2
                {
                    InterpMode::Bicubic
                } else {
                    InterpMode::Bilinear
                }
            }
            other => other,
        }
    }
}

/// The footprint of an anisotropic (`Default`/`Aniso`) lookup with derivatives `d`.
///
/// Steps 1-6 of [`texture_filter`]'s module docs, in `texture_lookup`'s order. Callers that need
/// to know which texels a lookup will touch (a tiler sizing a source region) use this; the lookup
/// itself calls it too, so the two cannot disagree.
pub fn aniso_footprint<M: MipSource + ?Sized>(
    src: &M,
    opt: &LookupOpt,
    d: Derivs,
) -> AnisoFootprint {
    // The resolution the bare derivatives ask for decides whether SmartBicubic is magnifying
    // (`:2275-2278`); taken before `swidth`/`twidth` scaling, as there.
    let sfilt_noblur = d.dsdx.abs().max(d.dsdy.abs()).max(1e-8);
    let tfilt_noblur = d.dtdx.abs().max(d.dtdy.abs()).max(1e-8);
    let naturalsres = (1.0 / sfilt_noblur) as i32;
    let naturaltres = (1.0 / tfilt_noblur) as i32;

    let d = texture_filter::adjust_width(d, opt.swidth, opt.twidth);
    let ellipse = texture_filter::ellipse_axes(d);
    let mut ellipse = texture_filter::adjust_blur(ellipse, opt.sblur, opt.tblur, opt.legacy_blur);
    let anisotropic = f32::from(opt.anisotropic);
    let mut aspect =
        texture_filter::anisotropic_aspect(&mut ellipse, anisotropic, opt.conservative_filter)
            .aspect;
    let blend = texture_filter::compute_miplevels(
        src,
        MipSelect::Blend,
        anisotropic,
        ellipse.major,
        ellipse.minor,
        &mut aspect,
        None,
    );
    let line = texture_filter::compute_ellipse_sampling(aspect, ellipse, None);
    AnisoFootprint {
        blend,
        line,
        naturalsres,
        naturaltres,
    }
}

/// `texture_lookup` (`texturesys.cpp:2261-2434`): the anisotropic ellipse lookup behind
/// `MipMode::Default` and `MipMode::Aniso`.
fn lookup_aniso<S: TexelSource + ?Sized>(
    src: &mut S,
    opt: &LookupOpt,
    ctx: &LookupContext,
    s: f32,
    t: f32,
    d: Derivs,
) -> Result<Accum, S::Error> {
    let fp = aniso_footprint(src, opt, d);
    let mut sum = Accum::default();
    for (level, weight) in fp.blend.contributions() {
        let interp = fp.interp_at(src, opt.interpmode, level);
        let probes = fp.line.probes().map(|p| (s + p.ds, t + p.dt, p.weight));
        let r = sample(src, ctx, interp, probes, level)?;
        sum.add_weighted(&r, weight);
    }
    Ok(sum)
}

// ============================================================================
// Samplers
// ============================================================================

/// The texel offsets, relative to the texel [`texture_filter::st_to_texel`] returns, that a
/// sampler may read on each axis: `(first, last)`, inclusive. What a tiler pads a source region
/// by so every texel a lookup reads is resident.
#[must_use]
pub const fn texel_span(interp: InterpMode) -> (i32, i32) {
    match interp {
        // Closest rounds up past a half (`:2548-2551`); bilinear reads the 2x2 block.
        InterpMode::Closest | InterpMode::Bilinear | InterpMode::SmartBicubic => (0, 1),
        // Taps at -1..=2 (`:3084-3092`).
        InterpMode::Bicubic => (-1, 2),
    }
}

/// Runs one level's sampler over the probes. `interp` is resolved (never `SmartBicubic`; a
/// stray one reads as bilinear, which is what every path but the ellipse path means by it).
fn sample<S, P>(
    src: &mut S,
    ctx: &LookupContext,
    interp: InterpMode,
    probes: P,
    level: usize,
) -> Result<Accum, S::Error>
where
    S: TexelSource + ?Sized,
    P: IntoIterator<Item = Probe>,
{
    match interp {
        InterpMode::Closest => sample_closest(src, ctx, probes, level),
        InterpMode::Bicubic => sample_bicubic(src, ctx, probes, level),
        InterpMode::Bilinear | InterpMode::SmartBicubic => sample_bilinear(src, ctx, probes, level),
    }
}

/// `sample_closest` (`texturesys.cpp:2525-2622`): the nearest texel of each probe.
fn sample_closest<S, P>(
    src: &mut S,
    ctx: &LookupContext,
    probes: P,
    level: usize,
) -> Result<Accum, S::Error>
where
    S: TexelSource + ?Sized,
    P: IntoIterator<Item = Probe>,
{
    let (w, h) = src.level_size(level);
    let mut acc = Accum::default();
    let mut nonfill = 0.0f32;
    for (s, t, weight) in probes {
        let (mut stex, sfrac) = texture_filter::st_to_texel(s, w, 0, false);
        let (mut ttex, tfrac) = texture_filter::st_to_texel(t, h, 0, false);
        if sfrac > 0.5 {
            stex += 1;
        }
        if tfrac > 0.5 {
            ttex += 1;
        }
        let (Some(x), Some(y)) = (
            ctx.swrap.wrap_texel(stex, 0, w as i32),
            ctx.twrap.wrap_texel(ttex, 0, h as i32),
        ) else {
            // Out of range under black wrap.
            nonfill += weight;
            continue;
        };
        let texel = src.texel(level, x, y)?;
        for (a, v) in acc.value.iter_mut().zip(texel) {
            *a += weight * v;
        }
    }
    ctx.mask_channels(&mut acc);
    if nonfill < 1.0 && ctx.use_fill() {
        ctx.add_fill(&mut acc, nonfill);
    }
    // Constant interpolation has zero derivatives (`:2617-2620`).
    acc.ds = [0.0; 4];
    acc.dt = [0.0; 4];
    Ok(acc)
}

/// `sample_bilinear` (`texturesys.cpp:2661-2915`), without the lat-long pole fade that only
/// environment lookups use.
// Per-channel loops over a texel grid; indexing reads closest to the reference.
#[allow(clippy::needless_range_loop)]
fn sample_bilinear<S, P>(
    src: &mut S,
    ctx: &LookupContext,
    probes: P,
    level: usize,
) -> Result<Accum, S::Error>
where
    S: TexelSource + ?Sized,
    P: IntoIterator<Item = Probe>,
{
    let (w, h) = src.level_size(level);
    let mut acc = Accum::default();
    let mut nonfill = 0.0f32;
    let use_fill = ctx.use_fill();
    for (s, t, weight) in probes {
        let (sint, sfrac) = texture_filter::st_to_texel(s, w, 0, false);
        let (tint, tfrac) = texture_filter::st_to_texel(t, h, 0, false);
        let sx = [
            ctx.swrap.wrap_texel(sint, 0, w as i32),
            ctx.swrap.wrap_texel(sint + 1, 0, w as i32),
        ];
        let ty = [
            ctx.twrap.wrap_texel(tint, 0, h as i32),
            ctx.twrap.wrap_texel(tint + 1, 0, h as i32),
        ];
        if sx.iter().chain(&ty).all(Option::is_none) {
            // Every texel is in the black-wrap region (`:2747-2750`).
            nonfill += weight;
            continue;
        }
        // A texel is read only where both its column and its row are valid (`:2814-2825`).
        let mut tex = [[[0.0f32; 4]; 2]; 2];
        for (j, y) in ty.iter().enumerate() {
            let Some(y) = *y else { continue };
            for (i, x) in sx.iter().enumerate() {
                let Some(x) = *x else { continue };
                tex[j][i] = src.texel(level, x, y)?;
            }
        }
        for c in 0..4 {
            let (v00, v01, v10, v11) = (tex[0][0][c], tex[0][1][c], tex[1][0][c], tex[1][1][c]);
            acc.value[c] += weight * texture_filter::bilerp(v00, v01, v10, v11, sfrac, tfrac);
            if ctx.derivs {
                // `:2880-2889`
                acc.ds[c] += weight * w as f32 * texture_filter::lerp(v01 - v00, v11 - v10, tfrac);
                acc.dt[c] += weight * h as f32 * texture_filter::lerp(v10 - v00, v11 - v01, sfrac);
            }
        }
        let all_valid = sx.iter().chain(&ty).all(Option::is_some);
        if use_fill && !all_valid {
            // How much of this probe landed on valid texels, which is where the fill value (not
            // black) applies to the missing channels (`:2890-2898`).
            let v = |i: usize, j: usize| f32::from(u8::from(sx[i].is_some() && ty[j].is_some()));
            let f = texture_filter::bilerp(v(0, 0), v(1, 0), v(0, 1), v(1, 1), sfrac, tfrac);
            nonfill += (1.0 - f) * weight;
        }
    }
    ctx.mask_channels(&mut acc);
    if use_fill {
        ctx.add_fill(&mut acc, nonfill);
    }
    Ok(acc)
}

/// `sample_bicubic` (`texturesys.cpp:3010-3398`): a 4x4 cubic B-spline per probe, evaluated in the
/// reference's lerp form, without the lat-long pole fade.
// Per-channel loops over a texel grid; indexing reads closest to the reference.
#[allow(clippy::needless_range_loop)]
fn sample_bicubic<S, P>(
    src: &mut S,
    ctx: &LookupContext,
    probes: P,
    level: usize,
) -> Result<Accum, S::Error>
where
    S: TexelSource + ?Sized,
    P: IntoIterator<Item = Probe>,
{
    let (w, h) = src.level_size(level);
    let mut acc = Accum::default();
    let mut nonfill = 0.0f32;
    let use_fill = ctx.use_fill();
    for (s, t, weight) in probes {
        let (sint, sfrac) = texture_filter::st_to_texel(s, w, 0, false);
        let (tint, tfrac) = texture_filter::st_to_texel(t, h, 0, false);
        // Taps at texel offsets -1..=2 (`:3084-3092`).
        let sx: [Option<i32>; 4] =
            std::array::from_fn(|i| ctx.swrap.wrap_texel(sint - 1 + i as i32, 0, w as i32));
        let ty: [Option<i32>; 4] =
            std::array::from_fn(|j| ctx.twrap.wrap_texel(tint - 1 + j as i32, 0, h as i32));
        let allvalid = sx.iter().chain(&ty).all(Option::is_some);
        let anyvalid = sx.iter().chain(&ty).any(Option::is_some);
        if !anyvalid {
            nonfill += weight;
            continue;
        }
        let mut tex = [[[0.0f32; 4]; 4]; 4];
        for (j, y) in ty.iter().enumerate() {
            let Some(y) = *y else { continue };
            for (i, x) in sx.iter().enumerate() {
                let Some(x) = *x else { continue };
                tex[j][i] = src.texel(level, x, y)?;
            }
        }

        let wx = texture_filter::bspline_weights(sfrac);
        let wy = texture_filter::bspline_weights(tfrac);
        let (h0x, h1x, g1x) = texture_filter::bspline_lerp_weights(wx);
        let (h0y, h1y, g1y) = texture_filter::bspline_lerp_weights(wy);
        let cubic = |row: [f32; 4], h0: f32, h1: f32, g1: f32| {
            texture_filter::lerp(
                texture_filter::lerp(row[0], row[1], h0),
                texture_filter::lerp(row[2], row[3], h1),
                g1,
            )
        };
        let (dwx, dwy) = if ctx.derivs {
            (
                texture_filter::bspline_weight_derivs(sfrac),
                texture_filter::bspline_weight_derivs(tfrac),
            )
        } else {
            ([0.0; 4], [0.0; 4])
        };
        for c in 0..4 {
            let col: [f32; 4] = std::array::from_fn(|j| {
                cubic(
                    [tex[j][0][c], tex[j][1][c], tex[j][2][c], tex[j][3][c]],
                    h0x,
                    h1x,
                    g1x,
                )
            });
            acc.value[c] += weight * cubic(col, h0y, h1y, g1y);
            if ctx.derivs {
                // `:3318-3363`
                let mut ds = 0.0f32;
                let mut dt = 0.0f32;
                for i in 0..4 {
                    ds += dwx[i]
                        * (wy[0] * tex[0][i][c]
                            + wy[1] * tex[1][i][c]
                            + wy[2] * tex[2][i][c]
                            + wy[3] * tex[3][i][c]);
                }
                for j in 0..4 {
                    dt += dwy[j]
                        * (wx[0] * tex[j][0][c]
                            + wx[1] * tex[j][1][c]
                            + wx[2] * tex[j][2][c]
                            + wx[3] * tex[j][3][c]);
                }
                acc.ds[c] += weight * w as f32 * ds;
                acc.dt[c] += weight * h as f32 * dt;
            }
        }
        if !allvalid && use_fill {
            // The same cubic over "is this texel valid" (`:3365-3381`).
            let col: [f32; 4] = std::array::from_fn(|j| {
                let v = |i: usize| f32::from(u8::from(ty[j].is_some() && sx[i].is_some()));
                cubic([v(0), v(1), v(2), v(3)], h0x, h1x, g1x)
            });
            nonfill += weight * (1.0 - cubic(col, h0y, h1y, g1y));
        }
    }
    ctx.mask_channels(&mut acc);
    if use_fill {
        ctx.add_fill(&mut acc, nonfill);
    }
    Ok(acc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;

    /// An in-memory RGBA MIP chain whose level `m` texel `(x, y)` is `(m, x, y, 1)`, so every
    /// read is traceable.
    struct Traced {
        sizes: Vec<(u32, u32)>,
        reads: Vec<(usize, i32, i32)>,
    }

    impl MipSource for Traced {
        fn num_levels(&self) -> usize {
            self.sizes.len()
        }
        fn level_size(&self, m: usize) -> (u32, u32) {
            self.sizes[m]
        }
    }

    impl TexelSource for Traced {
        type Error = Infallible;
        fn texel(&mut self, level: usize, x: i32, y: i32) -> Result<[f32; 4], Infallible> {
            self.reads.push((level, x, y));
            Ok([level as f32, x as f32, y as f32, 1.0])
        }
    }

    fn traced(w: u32, h: u32) -> Traced {
        Traced {
            sizes: (0..crate::mip::mip_level_count(w, h))
                .map(|m| crate::mip::mip_dimensions(w, h, m))
                .collect(),
            reads: Vec::new(),
        }
    }

    #[test]
    fn bilinear_nomip_interpolates_texel_coordinates() {
        // Texel values are their own coordinates, so a bilinear read at a texel centre returns
        // that texel and half-way between two returns the midpoint.
        let mut src = traced(8, 4);
        let ctx = LookupContext::all_channels(4, Wrap::Clamp, Wrap::Clamp);
        let opt = LookupOpt {
            mipmode: MipMode::NoMIP,
            interpmode: InterpMode::Bilinear,
            ..LookupOpt::default()
        };
        let r = texture_lookup(
            &mut src,
            &opt,
            &ctx,
            2.5 / 8.0,
            1.5 / 4.0,
            Derivs::default(),
        )
        .unwrap();
        assert_eq!(r.value, [0.0, 2.0, 1.0, 1.0]);
        let r = texture_lookup(
            &mut src,
            &opt,
            &ctx,
            3.0 / 8.0,
            1.5 / 4.0,
            Derivs::default(),
        )
        .unwrap();
        assert!((r.value[1] - 2.5).abs() < 1e-6, "{r:?}");
    }

    #[test]
    fn aniso_reads_only_what_the_footprint_and_span_allow() {
        // Every texel the lookup reads lies inside the footprint's probe extent widened by the
        // sampler's texel span: the contract a tiler sizes source regions by.
        let mut src = traced(64, 48);
        let ctx = LookupContext::all_channels(4, Wrap::Black, Wrap::Black);
        let opt = LookupOpt::default();
        let d = Derivs::new(0.08, 0.03, -0.01, 0.015);
        let (s, t) = (0.41, 0.57);
        texture_lookup(&mut src, &opt, &ctx, s, t, d).unwrap();
        let fp = aniso_footprint(&src, &opt, d);
        for &(level, x, y) in &src.reads {
            let (w, h) = src.level_size(level);
            let (lo, hi) = texel_span(fp.interp_at(&src, opt.interpmode, level));
            let sx0 = texture_filter::st_to_texel(s - fp.line.smajor.abs(), w, 0, false).0 + lo;
            let sx1 = texture_filter::st_to_texel(s + fp.line.smajor.abs(), w, 0, false).0 + hi;
            let ty0 = texture_filter::st_to_texel(t - fp.line.tmajor.abs(), h, 0, false).0 + lo;
            let ty1 = texture_filter::st_to_texel(t + fp.line.tmajor.abs(), h, 0, false).0 + hi;
            assert!(
                (sx0..=sx1).contains(&x) && (ty0..=ty1).contains(&y),
                "level {level} texel ({x},{y}) outside [{sx0},{sx1}]x[{ty0},{ty1}]"
            );
        }
        assert!(!src.reads.is_empty());
    }

    #[test]
    fn fill_applies_to_missing_channels_only() {
        let mut src = traced(4, 4);
        let ctx = LookupContext {
            actualchannels: 2,
            nchannels: 4,
            swrap: Wrap::Clamp,
            twrap: Wrap::Clamp,
            fill: 0.25,
            derivs: false,
        };
        let opt = LookupOpt {
            mipmode: MipMode::NoMIP,
            interpmode: InterpMode::Closest,
            ..LookupOpt::default()
        };
        let r = texture_lookup(&mut src, &opt, &ctx, 0.6, 0.6, Derivs::default()).unwrap();
        assert_eq!(r.value, [0.0, 2.0, 0.25, 0.25]);
    }
}
