//! High-level in-memory HTJ2K / JPEG 2000 codec API (**Layer 2**).
//!
//! # Why this module exists
//!
//! The OpenJPH core (ported 1:1 in the rest of this crate) drives encode/decode through a
//! **per-line streaming protocol**: you call [`Codestream::exchange`] / [`Codestream::pull`],
//! get back a raw `*mut LineBuf`, and read/write its samples through a raw pointer
//! (`(*line).storage.i32_`). That protocol is faithful to the C++ and maximally flexible
//! (streaming, tiling, custom buffers), but it forces every consumer to repeat the same
//! bookkeeping *and* to write `unsafe`.
//!
//! Downstream consumers — `heif-rs` (HTJ2K-in-HEIF) and `exr-core` (EXR `HT256`) — don't want
//! that. They have a whole image in memory and want one call. They also forbid `unsafe` in their
//! own code, so they cannot use the raw-pointer line protocol directly.
//!
//! This module is the thin **safe, whole-image** wrapper that solves both:
//! - It is built entirely on the *safe* `LineBuf` accessors ([`LineBuf::get_i32`] /
//!   [`LineBuf::get_i32_mut`]) and the safe exchange helpers ([`Codestream::pull_ref`],
//!   [`Codestream::fill_and_exchange`]). **No `unsafe` block appears here** — all of it stays
//!   encapsulated inside the 1:1 port, where it belongs.
//! - It speaks in plain owned buffers ([`Image`] / [`Plane`]), so a caller maps its own pixels
//!   to/from `i32` samples and never sees a codestream marker or a line pointer.
//!
//! Layer 1 (the safe primitives) + Layer 2 (this one-call API) together let `heif-codec-j2k`
//! integrate HTJ2K with zero `unsafe` on its side.

use crate::codestream::ojph_params_local::ParamSiz;
use crate::openjph::ojph_base::{Point, Size};
use crate::openjph::ojph_codestream::Codestream;
use crate::openjph::ojph_defs::Ui32;
use crate::openjph::ojph_file::{MemInfile, MemOutfile};

/// Deepest component a [`Plane`] can hold: 32 bits (unsigned 32-bit samples ride as their `i32` bit
/// pattern, which is what the GPU/float consumers rely on).
pub const MAX_BIT_DEPTH: u32 = 32;

/// One image component (plane) as `i32` samples, row-major and tightly packed.
///
/// JPEG 2000 codes every component independently and as signed integers, so the in-memory unit is
/// `i32` regardless of the source bit depth. The caller is responsible for packing/unpacking its
/// own pixel format (`u8` / `u16` / planar / interleaved) to and from these samples; keeping that
/// out of jph-rs is deliberate — the codec must not assume a colour model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plane {
    /// Component width in samples (may be smaller than the image width when subsampled).
    pub width: u32,
    /// Component height in samples.
    pub height: u32,
    /// Bits per sample actually used (e.g. 8/10/12/16). Stored in the codestream's `SIZ` marker.
    pub bit_depth: u32,
    /// Whether the samples are signed (`true`) or unsigned (`false`).
    pub signed: bool,
    /// `width * height` samples, row-major (row 0 first).
    pub samples: Vec<i32>,
}

/// A full image: a set of components on a shared canvas.
///
/// Components may differ in size from each other and from `width`/`height` (chroma subsampling);
/// each component's downsampling factor `d` is derived on encode as the one with
/// `ceil(image_size / d) == component_size` (an error if none exists) and taken from the
/// codestream on decode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// Full image width on the reference grid.
    pub width: u32,
    /// Full image height on the reference grid.
    pub height: u32,
    /// One entry per coded component (e.g. `[Y]`, `[R,G,B]`, `[Y,Cb,Cr]`, `[R,G,B,A]`).
    pub planes: Vec<Plane>,
}

/// Quantization / transform choice of an encode: one value instead of three interacting fields, so
/// "lossy without a step" or "Qfactor on a lossless encode" cannot even be written.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Quant {
    /// Reversible 5/3 wavelet: bit-exact lossless. The default.
    #[default]
    Lossless,
    /// Irreversible 9/7 wavelet with a flat base quantization step (finite, `> 0`; smaller =
    /// higher quality).
    Step(f32),
    /// Irreversible 9/7 with OpenJPH v0.31.0's JPEG-style quality factor, 1 (worst) ..= 100
    /// (best), applied via visually-weighted quantization.
    Factor(u8),
}

impl Quant {
    /// Whether this selects the reversible (lossless) wavelet.
    pub const fn is_lossless(self) -> bool {
        matches!(self, Quant::Lossless)
    }
}

/// Packet progression order (COD `SGcod`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Progression {
    Lrcp,
    Rlcp,
    /// Resolution - position - component - layer; the default (what OpenJPH writes).
    #[default]
    Rpcl,
    Pcrl,
    Cprl,
}

impl Progression {
    /// The marker name the codestream layer takes (`"LRCP"`, ...).
    pub const fn name(self) -> &'static str {
        match self {
            Progression::Lrcp => "LRCP",
            Progression::Rlcp => "RLCP",
            Progression::Rpcl => "RPCL",
            Progression::Pcrl => "PCRL",
            Progression::Cprl => "CPRL",
        }
    }
}

impl std::str::FromStr for Progression {
    type Err = String;

    /// Case-insensitive marker name.
    fn from_str(s: &str) -> Result<Self, String> {
        [Self::Lrcp, Self::Rlcp, Self::Rpcl, Self::Pcrl, Self::Cprl]
            .into_iter()
            .find(|p| p.name().eq_ignore_ascii_case(s))
            .ok_or_else(|| format!("`{s}` is not one of LRCP/RLCP/RPCL/PCRL/CPRL"))
    }
}

/// Encoder configuration.
///
/// The default is **lossless reversible, no colour transform** — the only setting that is always
/// correct regardless of the caller's colour model (each component is coded independently and
/// reconstructs bit-exactly). A caller that knows it has RGB and wants the reversible colour
/// transform (RCT) can opt in via [`EncodeParams::color_transform`].
#[derive(Debug, Clone)]
pub struct EncodeParams {
    /// Lossless (reversible 5/3), or lossy (9/7) with a step or quality factor.
    pub quant: Quant,
    /// Number of wavelet decomposition levels (resolution levels − 1).
    pub num_decompositions: u8,
    /// Code-block width / height (must be powers of two, 4..=64, product ≤ 4096).
    pub block_width: u32,
    /// Code-block height.
    pub block_height: u32,
    /// Packet progression order.
    pub progression: Progression,
    /// Apply the colour transform across the first three components (RCT when lossless, ICT when
    /// not). Only meaningful for ≥3-component RGB images; leave `false` to code components as-is.
    pub color_transform: bool,
}

impl Default for EncodeParams {
    fn default() -> Self {
        // Lossless, component-independent: the safe universal default for archival / HEIF storage.
        Self {
            quant: Quant::Lossless,
            num_decompositions: 5,
            block_width: 64,
            block_height: 64,
            progression: Progression::Rpcl,
            color_transform: false,
        }
    }
}

/// Hard ceiling on the total number of `i32` samples [`try_decode`] will allocate across all
/// component planes of one image.
///
/// Mirrors `jpg-rs`'s `MAX_PIXELS` (`jpg-rs/src/codestream.rs:364`, `1 << 28`) — the reference is
/// law, so the value and rationale are copied here. `2^28` samples = `16384 x 16384`, ~1 GiB as
/// `i32`: generous for real imagery. The cap exists because the plane buffers are sized straight
/// from the codestream's untrusted `SIZ` extent (`Xsiz`/`Ysiz`) — a crafted huge extent would
/// otherwise drive a multi-gigabyte `vec![0i32; w*h]` into an **OOM abort**, which `catch_unwind`
/// cannot intercept. Raise it if a consumer legitimately needs larger images.
pub const MAX_IMAGE_PIXELS: usize = 1 << 28;

/// Error returned by [`try_decode`] instead of panicking or aborting on hostile input.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DecodeError {
    /// The codestream is malformed or truncated. The 1:1 OpenJPH port reports such input by
    /// panicking internally; [`try_decode`] traps that panic and surfaces it here.
    Malformed(String),
    /// The `SIZ`-declared geometry would exceed [`MAX_IMAGE_PIXELS`]. Returned **before** any plane
    /// buffer is allocated, so an adversarial extent cannot trigger an OOM abort.
    TooLarge {
        /// Declared image width on the reference grid (`Xsiz - XOsiz`).
        width: u32,
        /// Declared image height on the reference grid (`Ysiz - YOsiz`).
        height: u32,
        /// Declared component count (`Csiz`).
        components: usize,
    },
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::Malformed(msg) => {
                write!(f, "malformed HTJ2K/JPEG 2000 codestream: {msg}")
            }
            DecodeError::TooLarge { width, height, components } => write!(
                f,
                "declared image geometry {width}x{height} x {components} component(s) exceeds \
                 MAX_IMAGE_PIXELS ({MAX_IMAGE_PIXELS} samples)"
            ),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Reject geometry whose worst-case sample budget would exceed [`MAX_IMAGE_PIXELS`], *before* any
/// buffer is allocated.
///
/// `width * height * components` is a sound upper bound on the real allocation — the sum of every
/// component's recon-plane area, each of which is `<=` the image area — so passing this check
/// guarantees the per-plane `vec![0i32; w*h]` loop stays within budget. Uses checked `u64`
/// arithmetic so an adversarial `Xsiz`/`Ysiz` cannot overflow into a small in-budget value.
fn check_geometry(width: u32, height: u32, components: usize) -> Result<(), DecodeError> {
    let budget = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|wh| wh.checked_mul(components as u64));
    match budget {
        Some(px) if px <= MAX_IMAGE_PIXELS as u64 => Ok(()),
        _ => Err(DecodeError::TooLarge { width, height, components }),
    }
}

/// Geometry of a codestream as declared by `SIZ`, in the units the decoded planes will have.
pub(crate) struct SizGeom {
    /// Active image width on the reference grid (`Xsiz - XOsiz`).
    pub width: u32,
    /// Active image height on the reference grid (`Ysiz - YOsiz`).
    pub height: u32,
    /// Per component `(recon_width, recon_height, bit_depth, signed)`.
    pub comps: Vec<(u32, u32, u32, bool)>,
}

/// Snapshot the declared geometry from `SIZ` and reject it if it exceeds [`MAX_IMAGE_PIXELS`].
///
/// This is the single untrusted-input gate shared by [`try_decode`] and `parse::enumerate`. It must
/// run right after `read_headers` and **before** `Codestream::create`, because `create` allocates
/// arenas (code-block headers, precincts, line buffers) proportional to this geometry. Width and
/// height are `extent - offset`, the same sizes `get_recon_*` use for the planes, so the image and
/// its planes always agree (including when `XOsiz`/`YOsiz` > 0).
pub(crate) fn check_siz(siz: &ParamSiz) -> Result<SizGeom, DecodeError> {
    let extent = siz.get_image_extent();
    let offset = siz.get_image_offset();
    // `get_num_components` is the raw `Csiz` (u16); widen to the `Ui32` the per-component getters take.
    let n = u32::from(siz.get_num_components());
    let comps: Vec<(u32, u32, u32, bool)> = (0..n)
        .map(|c| (siz.get_recon_width(c), siz.get_recon_height(c), siz.get_bit_depth(c), siz.is_signed(c)))
        .collect();
    let width = extent.x.saturating_sub(offset.x);
    let height = extent.y.saturating_sub(offset.y);
    check_geometry(width, height, comps.len())?;
    Ok(SizGeom { width, height, comps })
}

/// Fallible, bounded decode of a complete JPEG 2000 / HTJ2K codestream into an [`Image`].
///
/// The codestream's own `SIZ` marker supplies the geometry, component count, per-component size,
/// bit depth and signedness — so the caller passes only the bytes. Each pulled line is routed to
/// its component (OpenJPH reports the component index per line, so this is robust to whatever
/// progression order the encoder used). All sample access goes through the safe
/// [`LineBuf::get_i32`] accessor — no `unsafe` here.
///
/// Unlike [`decode`], this is safe on hostile input:
/// - **Malformed / truncated** codestreams are reported natively as `Err(OjphError)` by the 1:1
///   port (see `BUG1.md` "Round 7" — the crate-wide panic-to-`Result` conversion), mapped here to
///   [`DecodeError::Malformed`]. No `catch_unwind` is needed any more.
/// - **Oversized** declared geometry is rejected via [`check_geometry`] *before* the plane buffers
///   are allocated, returning [`DecodeError::TooLarge`] — so a crafted huge `SIZ` extent cannot
///   drive an unbounded allocation into an OOM abort (see [`MAX_IMAGE_PIXELS`]).
pub fn try_decode(codestream: &[u8]) -> Result<Image, DecodeError> {
    try_decode_with(codestream, &DecodeParams::default())
}

/// Decoder options for [`try_decode_with`]. `Default` is a full-resolution, strict decode, i.e.
/// exactly what [`try_decode`] does.
#[derive(Clone, Copy, Debug, Default)]
pub struct DecodeParams {
    /// Number of highest resolution levels to drop (each halves the plane size, rounding up). Must
    /// not exceed the stream's decomposition levels.
    pub reduce: u32,
    /// Tolerate damaged tile-parts/packets instead of failing on the first error (OpenJPH
    /// "resilient" mode).
    pub resilient: bool,
}

/// [`try_decode`] with options: reduced-resolution and/or resilient decoding. The returned
/// [`Image`] (and its planes) have the reduced size, `ceil(extent / 2^reduce) - ceil(offset / 2^reduce)`.
pub fn try_decode_with(codestream: &[u8], params: &DecodeParams) -> Result<Image, DecodeError> {
    (|| -> Result<Image, DecodeError> {
        let mut infile = MemInfile::default();
        // `open` borrows the slice's bytes for the lifetime of `infile` (which lives only within
        // this closure, fully covering the read below) — no copy of the codestream is made.
        infile.open(codestream);

        let mut cs = Codestream::new();
        if params.resilient {
            cs.enable_resilience().map_err(|e| DecodeError::Malformed(e.to_string()))?;
        }
        cs.read_headers(&mut infile)
            .map_err(|e| DecodeError::Malformed(e.to_string()))?;
        if params.reduce > 0 {
            cs.restrict_input_resolution(params.reduce, params.reduce)
                .map_err(|e| DecodeError::Malformed(e.to_string()))?;
        }

        // Untrusted-SIZ OOM guard: must precede `create()`, which allocates geometry-sized arenas.
        let SizGeom { mut width, mut height, comps } = check_siz(cs.access_siz())?;
        if params.reduce > 0 {
            // Same rounding as the codestream's reconstruction rectangles (`pre_alloc`).
            let (ext, off) = (cs.access_siz().get_image_extent(), cs.access_siz().get_image_offset());
            let ds = 1u32 << params.reduce;
            width = ext.x.div_ceil(ds) - off.x.div_ceil(ds);
            height = ext.y.div_ceil(ds) - off.y.div_ceil(ds);
        }
        // `Plane.samples` are `i32`: up to 32-bit components fit (an unsigned 32-bit sample is
        // carried as its two's-complement bit pattern); deeper ones are carried on i64 lines by the
        // port and would be silently truncated when copied out, so refuse them instead of
        // returning garbage. (`parse::enumerate` has no such limit: it never materialises samples.)
        if let Some(c) = comps.iter().position(|&(_, _, bd, _)| bd > MAX_BIT_DEPTH) {
            return Err(DecodeError::Malformed(format!(
                "component {c}: bit depth {} exceeds the {MAX_BIT_DEPTH} bits an i32 plane can hold",
                comps[c].2
            )));
        }
        cs.create().map_err(|e| DecodeError::Malformed(e.to_string()))?;

        let mut planes: Vec<Plane> = comps
            .iter()
            .map(|&(w, h, bit_depth, signed)| Plane {
                width: w,
                height: h,
                bit_depth,
                signed,
                samples: vec![0i32; (w as usize) * (h as usize)],
            })
            .collect();

        // One pull per (component, row). The codestream interleaves components by progression
        // order; `comp` tells us which plane each line belongs to, and we keep a per-plane cursor.
        let total_lines: u32 = planes.iter().map(|p| p.height).sum();
        let mut next_row = vec![0u32; planes.len()];
        for _ in 0..total_lines {
            let mut comp: Ui32 = 0;
            let line = cs.pull_ref(&mut comp).map_err(|e| DecodeError::Malformed(e.to_string()))?;
            let c = comp as usize;
            let bad = || DecodeError::Malformed(format!("decoder produced an unexpected line for component {comp}"));
            let plane = planes.get_mut(c).ok_or_else(bad)?;
            let w = plane.width as usize;
            let src = line.get_i32(w); // safe slice over the line's `w` samples
            let row = next_row[c] as usize;
            plane.samples.get_mut(row * w..row * w + w).ok_or_else(bad)?.copy_from_slice(src);
            next_row[c] += 1;
        }

        cs.close();
        Ok(Image { width, height, planes })
    })()
}

/// Decode a complete JPEG 2000 / HTJ2K codestream into an [`Image`], panicking on bad input.
///
/// Backward-compatible infallible entry point (`exr-core`, `heif-rs` and `vfx-io` call it and rely
/// on the panic-on-bad-input contract, wrapping it in their own `catch_unwind`). It now delegates
/// to [`try_decode`], so the [`MAX_IMAGE_PIXELS`] cap protects these callers too: an oversized
/// codestream becomes a bounded panic-with-message instead of a silent multi-GB OOM abort. New code
/// that wants typed, non-panicking errors should call [`try_decode`] directly.
pub fn decode(codestream: &[u8]) -> Image {
    try_decode(codestream).expect(
        "jph_rs::mem::decode: malformed or oversized codestream — use try_decode for fallible handling",
    )
}

/// Error returned by [`try_encode`] instead of letting an internal error escape the public API.
///
/// The 1:1 OpenJPH port reports invalid configurations as `Err(OjphError)` (`ojph_error!`,
/// mirroring C++'s `OJPH_ERROR` — see `BUG1.md` "Round 7" for the crate-wide panic-to-`Result`
/// conversion); [`try_encode`] maps that into this type, the same way [`try_decode`] does for
/// decode. See `BUG1.md` ("Round 5") for the confirmed-upstream-bug case this exists to guard:
/// `planar = 1` combined with the colour transform corrupts the codestream in real OpenJPH 0.31.0
/// too, and `write_headers` now hard-errors on it rather than silently producing bad output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodeError(String);

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "failed to encode HTJ2K/JPEG 2000 codestream: {}", self.0)
    }
}

impl std::error::Error for EncodeError {}

/// Fallible encode: see [`encode`] for the full contract. Returns [`EncodeError`] instead of
/// panicking when the 1:1 port rejects `image`/`params` (e.g. an invalid codeblock size, or the
/// confirmed-upstream `planar` + colour-transform combination `write_headers` now hard-errors on —
/// unreachable through this function's own scheduling choice below, but guarded regardless so no
/// internal panic can ever cross this API boundary).
pub fn try_encode(image: &Image, params: &EncodeParams) -> Result<Vec<u8>, EncodeError> {
    try_encode_with(image, params, &EncodeOptions::default())
}

/// Forward-quantizer rounding for **lossy** (irreversible) encodes; lossless output is unaffected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Rounding {
    /// Bit-compatible with OpenJPH's C++ encoder: the scalar kernel truncates, the SSE2/AVX2 kernels
    /// round to nearest, so the lossy bitstream depends on the CPU tier (differences are
    /// sub-quantization-step). Default; what [`try_encode`] uses.
    #[default]
    Cpp,
    /// Truncate toward zero (the T.800 dead-zone quantizer) on every tier: identical bitstreams on
    /// all CPUs, at the price of differing from the C++ AVX2/SSE2 output.
    Trunc,
}

/// Codestream-layout extras for [`try_encode_with`] beyond the coding parameters in
/// [`EncodeParams`]. `Default` is what [`try_encode`] uses: one tile, no TLM, no comment.
#[derive(Clone, Debug, Default)]
pub struct EncodeOptions {
    /// Lossy forward-quantizer rounding.
    pub rounding: Rounding,
    /// Tile width/height; `None` = a single tile covering the image.
    pub tile_size: Option<(u32, u32)>,
    /// Write a TLM (tile-part lengths) marker.
    pub tlm: bool,
    /// Text of a COM marker.
    pub comment: Option<String>,
    /// Start a new tile-part at every resolution.
    pub tilepart_res: bool,
    /// Start a new tile-part at every component.
    pub tilepart_comp: bool,
}

/// [`try_encode`] with explicit [`EncodeOptions`].
pub fn try_encode_with(image: &Image, params: &EncodeParams, opts: &EncodeOptions) -> Result<Vec<u8>, EncodeError> {
    encode_with_planar_override(image, params, None, opts)
}

/// Encode an [`Image`] into a JPEG 2000 / HTJ2K codestream with the given [`EncodeParams`].
///
/// Components are fed in planar (component-major) order via [`Codestream::fill_and_exchange`],
/// which keeps OpenJPH's raw line pointer entirely inside the port; the fill closure writes each
/// row through the safe [`LineBuf::get_i32_mut`] accessor — so no `unsafe` appears here either.
///
/// Returns the raw codestream bytes (the `.j2c` payload that HEIF stores in a `j2k1` item, or that
/// EXR wraps in an `HT256` block).
///
/// Backward-compatible infallible entry point (mirrors [`decode`] vs [`try_decode`]): delegates to
/// [`try_encode`] and panics with a message on failure. New code that wants a typed error should
/// call [`try_encode`] directly.
pub fn encode(image: &Image, params: &EncodeParams) -> Vec<u8> {
    try_encode(image, params)
        .unwrap_or_else(|e| panic!("jph_rs::mem::encode: {e} — use try_encode for fallible handling"))
}

/// Smallest downsampling factor `d` (`XRsiz`/`YRsiz`, a `u8` in `SIZ`) for which a component of
/// `plane` samples covers `full` reference-grid samples, i.e. `ceil(full / d) == plane`.
fn find_sampling(full: u32, plane: u32) -> Option<u32> {
    (1..=255u32).find(|&d| full.div_ceil(d) == plane)
}

/// Validate an [`Image`] + [`EncodeParams`] pair and derive each component's `(dx, dy)` sampling.
///
/// Covers what `try_encode` documents as "no internal panic crosses this boundary": plane buffers
/// must hold exactly `width * height` samples, the component count must fit `Csiz`, bit depths
/// must be representable in the `i32` planes, every plane size must correspond to an integral
/// sampling of the image grid (odd sizes included, e.g. 33 -> 17 is `dx = 2`), and a lossy encode
/// must say how coarse to quantize instead of silently using a zero step.
fn check_image(image: &Image, params: &EncodeParams) -> Result<Vec<(u32, u32)>, EncodeError> {
    let err = |m: String| EncodeError(m);
    if image.width == 0 || image.height == 0 {
        return Err(err(format!("image size {}x{} must be non-zero", image.width, image.height)));
    }
    // T.800 limits Csiz to 16384 components.
    if image.planes.is_empty() || image.planes.len() > 16384 {
        return Err(err(format!("component count {} must be in 1..=16384", image.planes.len())));
    }
    let mut sampling = Vec::with_capacity(image.planes.len());
    for (c, p) in image.planes.iter().enumerate() {
        if !(1..=MAX_BIT_DEPTH).contains(&p.bit_depth) {
            return Err(err(format!("component {c}: bit depth {} must be in 1..={MAX_BIT_DEPTH}", p.bit_depth)));
        }
        let expected = (p.width as usize).checked_mul(p.height as usize);
        if p.width == 0 || p.height == 0 || expected != Some(p.samples.len()) {
            return Err(err(format!(
                "component {c}: {} samples do not fill a {}x{} plane",
                p.samples.len(), p.width, p.height
            )));
        }
        match (find_sampling(image.width, p.width), find_sampling(image.height, p.height)) {
            (Some(dx), Some(dy)) => sampling.push((dx, dy)),
            _ => {
                return Err(err(format!(
                    "component {c}: plane {}x{} is not an integral subsampling of the {}x{} image",
                    p.width, p.height, image.width, image.height
                )));
            }
        }
    }
    match params.quant {
        Quant::Step(d) if !(d.is_finite() && d > 0.0) => {
            return Err(err(format!("Quant::Step({d}) must be finite and > 0")));
        }
        Quant::Factor(q) if !(1..=100).contains(&q) => {
            return Err(err(format!("Quant::Factor({q}) must be 1..=100")));
        }
        _ => {}
    }
    Ok(sampling)
}

/// `planar_override`: `None` uses the normal, verified-correct scheduling (`!color_transform`,
/// see below); `Some(v)` forces `set_planar(v)` regardless of `color_transform`, bypassing that
/// safety choice. Only reachable from within this crate — used by tests to drive the confirmed-
/// upstream-bug combination through this exact function and prove [`try_encode`] converts the
/// resulting panic into an `Err` rather than letting it escape, without weakening the public API
/// (`encode`/`try_encode` never expose a way to request the wrong scheduling themselves).
fn encode_with_planar_override(
    image: &Image,
    params: &EncodeParams,
    planar_override: Option<bool>,
    opts: &EncodeOptions,
) -> Result<Vec<u8>, EncodeError> {
    (|| -> Result<Vec<u8>, EncodeError> {
    let map_e = |e: crate::openjph::ojph_message::OjphError| EncodeError(e.to_string());
    // Everything the codestream writer would otherwise index or cast blindly is checked up front,
    // so no malformed `Image`/`EncodeParams` can panic or wrap inside the port.
    let sampling = check_image(image, params)?;
    let mut cs = Codestream::new();
    cs.set_trunc_quant(opts.rounding == Rounding::Trunc);

    // --- SIZ: image grid + per-component geometry ---
    {
        let siz = cs.access_siz();
        siz.set_image_extent(Point::new(image.width, image.height));
        // Default: a single tile covering the whole image, the simplest valid still-picture layout.
        let (tw, th) = opts.tile_size.unwrap_or((image.width, image.height));
        if tw == 0 || th == 0 {
            return Err(EncodeError(format!("tile size {tw}x{th} must be non-zero")));
        }
        siz.set_tile_size(Size::new(tw, th));
        siz.set_num_components(image.planes.len() as Ui32);
        for (c, (p, &(dx, dy))) in image.planes.iter().zip(&sampling).enumerate() {
            siz.set_comp_info(c as Ui32, Point::new(dx, dy), p.bit_depth, p.signed);
        }
    }

    // --- COD: coding style (wavelet, blocks, progression, colour transform) ---
    {
        let cod = cs.access_cod();
        cod.set_reversible(params.quant.is_lossless());
        cod.set_num_decompositions(params.num_decompositions).map_err(map_e)?;
        cod.set_block_dims(params.block_width, params.block_height).map_err(map_e)?;
        cod.set_progression_order(params.progression.name()).map_err(map_e)?;
        cod.employ_color_transform(u8::from(params.color_transform));
    }

    // Lossy (irreversible) path: install the quantization step / quality factor.
    match params.quant {
        Quant::Lossless => {}
        Quant::Step(d) => cs.access_qcd_mut().set_delta(d),
        Quant::Factor(q) => cs.access_qcd_mut().set_qfactor(q).map_err(map_e)?,
    }

    // Request the line-feeding order the codestream actually needs. When the colour transform is
    // employed, the encoder needs one row from each of the first three components together (to
    // compute Y/Cb/Cr or RCT), i.e. *interleaved* (`planar = false`) — matching what the `ojph`
    // CLI (`src/bin/ojph.rs::encode`) always requests. This isn't just about avoiding the
    // `write_headers` panic (`0x00030021`, "the planar interface option cannot be used when
    // colour transform is employed"): `write_headers`'s own untouched-default auto-select
    // (`planar == -1`) resolves to `planar = 1` for colour-transform-on too
    // (`ojph_codestream_local.cpp`: `planar = is_employing_color_transform() ? 1 : 0`), which does
    // NOT panic but silently corrupts every component but the last one fed (verified: each
    // non-last component's plane comes back as its last row repeated for every row — a latent bug
    // in that code path, unreachable before this fix since explicitly requesting `planar = 1` with
    // colour transform always hit the panic instead). Requesting `planar = false` explicitly, as
    // the CLI does, avoids that branch entirely and is the verified-correct scheduling.
    cs.set_planar(planar_override.unwrap_or(!params.color_transform));
    if opts.tlm {
        cs.request_tlm_marker(true);
    }
    if opts.tilepart_res || opts.tilepart_comp {
        cs.set_tilepart_divisions(opts.tilepart_res, opts.tilepart_comp);
    }

    let mut outfile = MemOutfile::default();
    // Initial capacity is a hint only — `MemOutfile` grows as needed; size it near the raw sample
    // footprint so a compressible image never reallocates.
    // Capped so a huge declared size never pre-reserves gigabytes before a single byte is coded.
    let raw = (image.width as usize).saturating_mul(image.height as usize).saturating_mul(image.planes.len());
    let initial = raw.saturating_add(64 * 1024).min(64 << 20).next_power_of_two();
    outfile.open(initial, false).map_err(map_e)?;
    let comments = match &opts.comment {
        Some(text) => {
            let mut c = crate::openjph::ojph_params::CommentExchange::default();
            c.set_string(text).map_err(map_e)?;
            vec![c]
        }
        None => Vec::new(),
    };
    cs.write_headers_slice(&mut outfile, &comments).map_err(map_e)?;

    // Stream every (component, row). `fill_and_exchange` hands our closure the current line and the
    // component index it expects; we copy that component's next row into it.
    let mut next_row = vec![0u32; image.planes.len()];
    let mut next_comp: Ui32 = 0;
    let mut cur_line = cs.exchange(core::ptr::null_mut(), &mut next_comp).map_err(map_e)?;
    while !cur_line.is_null() {
        cs.fill_and_exchange(&mut cur_line, &mut next_comp, |line, comp| {
            let c = comp as usize;
            let p = &image.planes[c];
            let w = p.width as usize;
            let row = next_row[c] as usize;
            let dst = line.get_i32_mut(w); // safe mutable slice over the line's `w` samples
            dst.copy_from_slice(&p.samples[row * w..row * w + w]);
            next_row[c] += 1;
        }).map_err(map_e)?;
    }

    cs.flush().map_err(map_e)?;
    cs.close();
    Ok(outfile.get_data().to_vec())
    })()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lossless roundtrip through the whole-image API: a 3-component RGB image must come back
    /// bit-identical. This proves the Layer-2 wrapper drives the safe Layer-1 primitives correctly
    /// (geometry snapshot, per-component line routing, reversible coding).
    #[test]
    fn rgb_lossless_roundtrip() {
        let (w, h) = (40u32, 24u32);
        let mut planes = Vec::new();
        for c in 0..3u32 {
            let mut samples = vec![0i32; (w * h) as usize];
            for y in 0..h {
                for x in 0..w {
                    samples[(y * w + x) as usize] = ((x * 2 + y * 3 + c * 37) & 0xff) as i32;
                }
            }
            planes.push(Plane { width: w, height: h, bit_depth: 8, signed: false, samples });
        }
        let img = Image { width: w, height: h, planes };

        let bytes = encode(&img, &EncodeParams::default());
        assert!(!bytes.is_empty(), "encoder produced an empty codestream");

        let back = decode(&bytes);
        assert_eq!(back, img, "lossless roundtrip must reconstruct the image bit-exactly");
    }

    /// Subsampled component (half-resolution second plane) must roundtrip with its own geometry,
    /// confirming per-component sizes survive encode→decode.
    #[test]
    fn subsampled_component_roundtrip() {
        let (w, h) = (32u32, 16u32);
        let luma = Plane {
            width: w,
            height: h,
            bit_depth: 8,
            signed: false,
            samples: (0..w * h).map(|i| (i % 200) as i32).collect(),
        };
        let chroma = Plane {
            width: w / 2,
            height: h / 2,
            bit_depth: 8,
            signed: false,
            samples: (0..(w / 2) * (h / 2)).map(|i| (i % 100) as i32).collect(),
        };
        let img = Image { width: w, height: h, planes: vec![luma, chroma] };

        let bytes = encode(&img, &EncodeParams::default());
        let back = decode(&bytes);
        assert_eq!(back, img);
    }

    /// Lossy (irreversible) encode via `quant_step`: the codestream decodes to a close-but-not-exact
    /// approximation of the source. Proves the quantization knob is wired through `mem`.
    #[test]
    fn lossy_gray_roundtrip_within_bound() {
        let (w, h) = (32u32, 32u32);
        let samples: Vec<i32> = (0..w * h).map(|i| (i % 251) as i32).collect();
        let img = Image {
            width: w,
            height: h,
            planes: vec![Plane {
                width: w,
                height: h,
                bit_depth: 8,
                signed: false,
                samples: samples.clone(),
            }],
        };
        let lossy = encode(
            &img,
            &EncodeParams { quant: Quant::Step(0.05), ..EncodeParams::default() },
        );
        assert!(!lossy.is_empty());
        let back = decode(&lossy);
        assert_eq!((back.width, back.height), (w, h));
        let max_err = back.planes[0]
            .samples
            .iter()
            .zip(&samples)
            .map(|(a, b)| (a - b).abs())
            .max()
            .unwrap_or(0);
        assert!(max_err <= 96, "lossy max abs error {max_err} unexpectedly large");
    }

    /// The old "lossy without a step" and "Qfactor on a lossless encode" states are now
    /// unrepresentable; the remaining invalid values are range-checked.
    #[test]
    fn quant_values_are_validated() {
        let img = Image { width: 8, height: 8, planes: vec![plane(8, 8, 8)] };
        let enc = |quant| try_encode(&img, &EncodeParams { quant, ..EncodeParams::default() });
        assert!(enc(Quant::Lossless).is_ok());
        assert!(enc(Quant::Step(0.05)).is_ok());
        assert!(enc(Quant::Step(0.0)).is_err());
        assert!(enc(Quant::Step(f32::NAN)).is_err());
        assert!(enc(Quant::Step(-1.0)).is_err());
        assert!(enc(Quant::Factor(0)).is_err());
        assert!(enc(Quant::Factor(101)).is_err());
    }

    /// `Progression` round-trips through its marker name.
    #[test]
    fn progression_names_roundtrip() {
        for p in [Progression::Lrcp, Progression::Rlcp, Progression::Rpcl, Progression::Pcrl, Progression::Cprl] {
            assert_eq!(p.name().parse::<Progression>(), Ok(p));
        }
        assert_eq!("rpcl".parse::<Progression>(), Ok(Progression::Rpcl));
        assert!("XXXX".parse::<Progression>().is_err());
    }

    /// A single-component (grayscale) image is a "one or two components" case in
    /// `check_validity`'s qfactor-expansion loop, which auto-assigns it the Y visual weighting.
    /// Higher qfactor (better quality) must decode closer to the source than a lower one.
    #[test]
    fn qfactor_higher_quality_reduces_error() {
        let (w, h) = (48u32, 48u32);
        let samples: Vec<i32> = (0..w * h).map(|i| ((i * 7) % 251) as i32).collect();
        let img = Image {
            width: w,
            height: h,
            planes: vec![Plane {
                width: w,
                height: h,
                bit_depth: 8,
                signed: false,
                samples: samples.clone(),
            }],
        };
        let max_err = |qfactor: u8| -> i32 {
            let bytes = encode(
                &img,
                &EncodeParams {
                    quant: Quant::Factor(qfactor),
                    ..EncodeParams::default()
                },
            );
            let back = decode(&bytes);
            back.planes[0]
                .samples
                .iter()
                .zip(&samples)
                .map(|(a, b)| (a - b).abs())
                .max()
                .unwrap_or(0)
        };
        let err_low_quality = max_err(10);
        let err_high_quality = max_err(95);
        assert!(
            err_high_quality <= err_low_quality,
            "qfactor 95 (max error {err_high_quality}) should not be worse than qfactor 10 (max error {err_low_quality})"
        );
    }

    /// Qfactor out of the documented 1..=100 range must be rejected, matching
    /// `param_qcd::set_qfactor`'s `OJPH_ERROR` (ported as a panic via `ojph_error!`).
    #[test]
    #[should_panic]
    fn qfactor_out_of_range_panics() {
        let img = Image {
            width: 8,
            height: 8,
            planes: vec![Plane {
                width: 8,
                height: 8,
                bit_depth: 8,
                signed: false,
                samples: vec![0; 64],
            }],
        };
        let _ = encode(
            &img,
            &EncodeParams { quant: Quant::Factor(0), ..EncodeParams::default() },
        );
    }

    /// `mem::encode()` used to unconditionally force `planar = true`. `write_headers` errors
    /// (`0x00030021`) when `planar == 1` is requested *explicitly* while the colour transform is
    /// employed — even though the auto-select default (`planar == -1`, untouched) resolves to the
    /// same value (1) through a different, non-erroring branch of the same three-way check. This
    /// reproduces the crash report and proves the fix: `color_transform: true` must succeed, not
    /// panic, matching `EncodeParams`'s own doc comment that lossy/lossless + colour transform is a
    /// normal, supported combination.
    #[test]
    fn encode_with_color_transform_does_not_panic() {
        let (w, h) = (24u32, 16u32);
        let mk = |seed: i32| Plane {
            width: w,
            height: h,
            bit_depth: 8,
            signed: false,
            samples: (0..w * h).map(|i| ((i as i32 * 7 + seed) % 251).abs()).collect(),
        };
        let img = Image { width: w, height: h, planes: vec![mk(0), mk(11), mk(23)] };
        let params = EncodeParams { color_transform: true, num_decompositions: 2, ..EncodeParams::default() };
        let bytes = encode(&img, &params); // must not panic
        assert!(!bytes.is_empty());
        let back = decode(&bytes);
        assert_eq!(back, img, "lossless colour-transform roundtrip must be bit-exact");
    }

    /// Byte-identity between `mem::encode()` (the safe `fill_and_exchange` whole-image API) and an
    /// independent encode built directly on the same low-level `Codestream` calls the `ojph` CLI
    /// binary uses for an RGB + colour-transform image (`src/bin/ojph.rs::encode`: same SIZ/COD
    /// setup, `set_planar(false)`, per-line feeding via `exchange`). Two different code paths
    /// driving the same 1:1 port for the same image and parameters must produce the identical
    /// codestream.
    #[test]
    fn encode_with_color_transform_matches_cli_code_path() {
        let (w, h) = (20u32, 12u32);
        let mk = |seed: i32| -> Vec<i32> {
            (0..w * h).map(|i| ((i as i32 * 5 + seed) % 251).abs()).collect()
        };
        let (r, g, b) = (mk(0), mk(13), mk(29));

        // Path 1: the mem::encode() whole-image API under test.
        let img = Image {
            width: w,
            height: h,
            planes: vec![
                Plane { width: w, height: h, bit_depth: 8, signed: false, samples: r.clone() },
                Plane { width: w, height: h, bit_depth: 8, signed: false, samples: g.clone() },
                Plane { width: w, height: h, bit_depth: 8, signed: false, samples: b.clone() },
            ],
        };
        let ours = encode(&img, &EncodeParams { color_transform: true, ..EncodeParams::default() });

        // Path 2: the same low-level Codestream calls src/bin/ojph.rs's `encode()` makes for an RGB
        // + colour-transform image with matching (default) COD settings.
        let mut cs = Codestream::new();
        {
            let siz = cs.access_siz();
            siz.set_image_extent(Point::new(w, h));
            siz.set_tile_size(Size::new(w, h));
            siz.set_num_components(3);
            for c in 0..3u32 {
                siz.set_comp_info(c, Point::new(1, 1), 8, false);
            }
        }
        {
            let cod = cs.access_cod();
            cod.set_num_decompositions(5).expect("set_num_decompositions");
            cod.set_block_dims(64, 64).expect("set_block_dims");
            cod.set_progression_order("RPCL").expect("set_progression_order");
            cod.set_reversible(true);
            cod.employ_color_transform(1);
        }
        cs.set_planar(false);
        let mut outfile = MemOutfile::default();
        outfile.open(4096, false).expect("open");
        cs.write_headers_slice(&mut outfile, &[]).expect("write_headers_slice");
        let planes = [&r, &g, &b];
        let mut next_row = [0u32; 3];
        let mut next_comp: Ui32 = 0;
        let mut cur_line = cs.exchange(core::ptr::null_mut(), &mut next_comp).expect("exchange");
        while !cur_line.is_null() {
            cs.fill_and_exchange(&mut cur_line, &mut next_comp, |line, comp| {
                let c = comp as usize;
                let row = next_row[c] as usize;
                let dst = line.get_i32_mut(w as usize);
                dst.copy_from_slice(&planes[c][row * w as usize..row * w as usize + w as usize]);
                next_row[c] += 1;
            })
            .expect("fill_and_exchange");
        }
        cs.flush().expect("flush");
        cs.close();
        let cli_equivalent = outfile.get_data().to_vec();

        assert_eq!(
            ours, cli_equivalent,
            "mem::encode() must match the CLI's own low-level Codestream call sequence byte-for-byte"
        );
    }

    /// `try_encode` must never let an internal error escape the public API as anything but `Err`,
    /// the same contract `try_decode` already provides for decode. `encode`/`try_encode` never
    /// expose a way to request the confirmed-upstream-bug scheduling themselves (round 4 already
    /// picks the correct one), so this drives it through the crate-internal
    /// `encode_with_planar_override` test seam to prove `write_headers`'s hard error surfaces as
    /// `Err` — not that ordinary `EncodeParams` usage can reach it.
    #[test]
    fn try_encode_converts_planar_colour_transform_panic_to_err() {
        let (w, h) = (32u32, 32u32);
        let mk = |seed: i32| Plane {
            width: w,
            height: h,
            bit_depth: 8,
            signed: false,
            samples: (0..w * h).map(|i| (i as i32 + seed) % 251).collect(),
        };
        let img = Image { width: w, height: h, planes: vec![mk(0), mk(11), mk(23)] };
        let params = EncodeParams { color_transform: true, ..EncodeParams::default() };

        // Force planar=true (the confirmed-upstream-bug combination with colour transform) —
        // ordinary encode()/try_encode() would never request this themselves.
        let result = encode_with_planar_override(&img, &params, Some(true), &EncodeOptions::default());
        assert!(
            matches!(result, Err(_)),
            "the planar=1 + colour-transform combination must surface as Err, not Ok and not a panic"
        );

        // A normal, unrelated encode still works fine afterward (no poisoned state left behind).
        let ok = try_encode(&img, &EncodeParams::default());
        assert!(ok.is_ok());
    }

    /// Small deterministic 3-component RGB image reused by the `try_decode` hardening tests.
    fn small_rgb_image() -> Image {
        let (w, h) = (40u32, 24u32);
        let mut planes = Vec::new();
        for c in 0..3u32 {
            let mut samples = vec![0i32; (w * h) as usize];
            for y in 0..h {
                for x in 0..w {
                    samples[(y * w + x) as usize] = ((x * 2 + y * 3 + c * 37) & 0xff) as i32;
                }
            }
            planes.push(Plane { width: w, height: h, bit_depth: 8, signed: false, samples });
        }
        Image { width: w, height: h, planes }
    }

    /// A truncated codestream must come back as `Err(DecodeError::Malformed)`, never a panic.
    /// (First quarter of a real stream: valid enough to enter header parsing, too little to
    /// finish — the same known-safe malformed case the `oiio-rs` `vfx-io` guard exercises.)
    #[test]
    fn try_decode_truncated_is_malformed_err() {
        let bytes = encode(&small_rgb_image(), &EncodeParams::default());
        let truncated = &bytes[..bytes.len() / 4];
        match try_decode(truncated) {
            Err(DecodeError::Malformed(_)) => {}
            other => panic!("expected Err(Malformed) on truncated input, got {other:?}"),
        }
    }

    /// A valid codestream must decode via `try_decode` and round-trip bit-exactly, proving the
    /// pre-allocation cap check doesn't disturb the happy path.
    #[test]
    fn try_decode_roundtrips_ok() {
        let img = small_rgb_image();
        let bytes = encode(&img, &EncodeParams::default());
        let back = try_decode(&bytes).expect("valid codestream must decode");
        assert_eq!(back, img, "try_decode must reconstruct a valid image bit-exactly");
    }

    /// The cap check must accept geometry at the limit and reject anything over it — including the
    /// overflow case, where naive `w*h*comps` arithmetic would wrap into a small in-budget value.
    /// Tested directly (forging a huge-`SIZ` codestream is impractical), which is exactly the path
    /// `try_decode` runs before any plane buffer is allocated.
    #[test]
    fn check_geometry_caps_oversized() {
        // Exactly MAX_IMAGE_PIXELS (16384 x 16384 x 1) is allowed.
        assert!(check_geometry(16384, 16384, 1).is_ok());
        // One more component tips it over the cap.
        match check_geometry(16384, 16384, 2) {
            Err(DecodeError::TooLarge { width: 16384, height: 16384, components: 2 }) => {}
            other => panic!("expected TooLarge, got {other:?}"),
        }
        // Overflow must be rejected, not wrapped into a small value that slips past the cap.
        match check_geometry(u32::MAX, u32::MAX, 4) {
            Err(DecodeError::TooLarge { .. }) => {}
            other => panic!("expected TooLarge on overflow, got {other:?}"),
        }
    }

    /// A real stream whose `SIZ` extent and tile size are patched to 2^31 must be rejected with
    /// `TooLarge` by both front-ends *before* `create()` allocates geometry-sized arenas (a
    /// regression guard for the gate running after the allocation).
    #[test]
    fn hostile_siz_rejected_before_create() {
        let mut bytes = encode(&small_rgb_image(), &EncodeParams::default());
        // SOC(2) + SIZ marker(2) + Lsiz(2) + Rsiz(2) -> Xsiz@8 Ysiz@12 XOsiz@16 YOsiz@20 XTsiz@24 YTsiz@28.
        assert_eq!(&bytes[..4], &[0xFF, 0x4F, 0xFF, 0x51], "stream must start with SOC+SIZ");
        for off in [8usize, 12, 24, 28] {
            bytes[off..off + 4].copy_from_slice(&0x7FFF_FFFFu32.to_be_bytes());
        }
        assert!(matches!(try_decode(&bytes), Err(DecodeError::TooLarge { .. })));
        assert!(matches!(
            crate::parse::enumerate(&bytes),
            Err(crate::parse::ParseError::ParseFailed(ref m)) if m.contains("exceeds")
        ));
    }

    /// Byte offset of the first SOT's `Psot` field (marker 2 + Lsot 2 + Isot 2 bytes in).
    fn psot_offset(bytes: &[u8]) -> usize {
        bytes.windows(2).position(|w| w == [0xFF, 0x90]).expect("SOT marker") + 6
    }

    /// `Psot == 0` (last tile-part runs to EOC) is legal T.800 and must decode exactly like the
    /// explicit length; a nonzero `Psot` below the SOT size is corrupt and must be an error.
    #[test]
    fn psot_zero_is_open_ended_and_tiny_psot_is_rejected() {
        let img = small_rgb_image();
        let bytes = encode(&img, &EncodeParams::default());
        let at = psot_offset(&bytes);

        let mut open = bytes.clone();
        open[at..at + 4].copy_from_slice(&0u32.to_be_bytes());
        assert_eq!(try_decode(&open).expect("Psot=0 must decode"), img);

        let mut tiny = bytes.clone();
        tiny[at..at + 4].copy_from_slice(&5u32.to_be_bytes());
        assert!(matches!(try_decode(&tiny), Err(DecodeError::Malformed(_))));
    }

    fn plane(w: u32, h: u32, bit_depth: u32) -> Plane {
        let samples = (0..w * h).map(|i| (i * 7 % 251) as i32).collect();
        Plane { width: w, height: h, bit_depth, signed: false, samples }
    }

    /// Odd sizes: a 17x17 chroma plane of a 33x33 image is `dx = dy = 2` (ceil), not the floor
    /// quotient 1, and must survive the roundtrip with its own geometry.
    #[test]
    fn odd_subsampling_roundtrip() {
        let img = Image { width: 33, height: 33, planes: vec![plane(33, 33, 8), plane(17, 17, 8), plane(17, 17, 8)] };
        let back = try_decode(&try_encode(&img, &EncodeParams::default()).expect("encode")).expect("decode");
        assert_eq!(back, img);
    }

    /// `reduce` drops resolution levels (ceil-halving each) and rejects more levels than exist.
    #[test]
    fn reduced_decode_halves_planes_and_rejects_excess() {
        let img = Image { width: 33, height: 20, planes: vec![plane(33, 20, 8)] };
        let bytes = try_encode(&img, &EncodeParams::default()).expect("encode");
        let half = try_decode_with(&bytes, &DecodeParams { reduce: 1, ..Default::default() }).expect("reduce 1");
        assert_eq!((half.width, half.height), (17, 10));
        assert_eq!((half.planes[0].width, half.planes[0].height), (17, 10));
        assert_eq!(half.planes[0].samples.len(), 17 * 10);
        assert!(try_decode_with(&bytes, &DecodeParams { reduce: 99, ..Default::default() }).is_err());
    }

    /// Layout options (tiles, TLM, COM, tile-parts) change the codestream but not the pixels.
    #[test]
    fn encode_options_roundtrip() {
        let img = Image { width: 70, height: 50, planes: vec![plane(70, 50, 8), plane(70, 50, 8)] };
        let opts = EncodeOptions {
            tile_size: Some((32, 32)),
            tlm: true,
            comment: Some("jph-rs test".into()),
            tilepart_res: true,
            ..Default::default()
        };
        let bytes = try_encode_with(&img, &EncodeParams::default(), &opts).expect("encode");
        assert!(bytes.windows(11).any(|w| w == b"jph-rs test"), "COM text present");
        assert!(bytes.windows(2).any(|w| w == [0xFF, 0x55]), "TLM marker present");
        assert_eq!(try_decode(&bytes).expect("decode"), img);
        let bad = EncodeOptions { tile_size: Some((0, 8)), ..Default::default() };
        assert!(try_encode_with(&img, &EncodeParams::default(), &bad).is_err());
    }

    /// `try_encode` must report (not panic on) every malformed image/parameter combination.
    #[test]
    fn try_encode_rejects_malformed_input() {
        let ok = || Image { width: 8, height: 8, planes: vec![plane(8, 8, 8)] };
        let p = EncodeParams::default();
        assert!(try_encode(&ok(), &p).is_ok());

        let mut short = ok();
        short.planes[0].samples.pop(); // one sample short of 8x8
        assert!(try_encode(&short, &p).is_err());

        let mut depth0 = ok();
        depth0.planes[0].bit_depth = 0;
        assert!(try_encode(&depth0, &p).is_err());
        let mut depth33 = ok();
        depth33.planes[0].bit_depth = MAX_BIT_DEPTH + 1;
        assert!(try_encode(&depth33, &p).is_err());

        assert!(try_encode(&Image { width: 8, height: 8, planes: vec![] }, &p).is_err());
        assert!(try_encode(&Image { width: 0, height: 8, planes: vec![plane(0, 8, 8)] }, &p).is_err());
        // A plane wider than the image has no integral sampling.
        let wide = Image { width: 8, height: 8, planes: vec![plane(9, 8, 8)] };
        assert!(try_encode(&wide, &p).is_err());
        // A lossy encode with a zero step is rejected (it used to encode with delta = 0).
        let lossy = EncodeParams { quant: Quant::Step(0.0), ..EncodeParams::default() };
        assert!(try_encode(&ok(), &lossy).is_err());
    }

    /// `Image` size and plane sizes must agree when the image offset is non-zero: both are
    /// `extent - offset` on the reference grid, and `ceil(X/d) - ceil(XO/d)` per subsampled component.
    #[test]
    fn check_siz_uses_recon_sizes_with_offset() {
        let mut cs = Codestream::new();
        let siz = cs.access_siz();
        siz.set_image_extent(Point::new(74, 50));
        siz.set_image_offset(Point::new(10, 2));
        siz.set_tile_size(Size::new(74, 50));
        siz.set_num_components(2);
        siz.set_comp_info(0, Point::new(1, 1), 8, false);
        siz.set_comp_info(1, Point::new(2, 2), 8, false);
        let g = check_siz(siz).expect("in-budget geometry");
        assert_eq!((g.width, g.height), (64, 48));
        assert_eq!((g.comps[0].0, g.comps[0].1), (64, 48));
        assert_eq!((g.comps[1].0, g.comps[1].1), (32, 24)); // ceil(74/2)-ceil(10/2), ceil(50/2)-ceil(2/2)
    }
}
