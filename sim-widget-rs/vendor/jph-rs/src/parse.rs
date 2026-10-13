//! Parse-only per-code-block enumeration — a bounded, production front-end (`O0.1`).
//!
//! # Why this module exists
//! The GPU HTJ2K decoder (`jph-wgpu-rs`) needs, per code-block, only its **decode plan**
//! (coded bytes + lengths + `missing_msbs` + `num_passes` + flags + geometry) so it can run
//! entropy + DWT on the GPU. The existing [`crate::mem::decode`] (and the `debug-extract` oracle
//! tap) run the **full CPU entropy decode of the whole image** to obtain that — fatal at 16K.
//!
//! [`enumerate`] instead drives only the cheap first half of the pipeline that already exists in
//! this crate: `read_headers` + `create` parse all markers, tile-parts, packets, precincts and
//! tag-trees, populating every `CodedCbHeader`. It then walks the resolution/subband/code-block
//! tree and reads each block's plan straight out of the parsed state — **without** ever pulling a
//! line (no entropy decode, no inverse DWT). The walk is strictly additive to the 1:1 OpenJPH
//! port: the byte-identical decode path is untouched.
//!
//! Unlike [`crate::others::debug_extract`], this is a normal production API (not feature-gated)
//! and carries **no** decoded coefficients — only the plan.

use crate::openjph::ojph_codestream::Codestream;
use crate::openjph::ojph_file::MemInfile;

/// Codec-wide configuration, read from the `SIZ`/`COD` markers after `read_headers`.
///
/// All geometry is on the reference grid (samples). `num_decomp` is authoritative from the `COD`
/// marker (the number of wavelet decomposition levels), *not* inferred from the block set — a
/// coarse-quantised stream can have empty subbands whose absence would otherwise undercount it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodecConfig {
    /// Full image width (`Xsiz - XOsiz`, via `get_image_extent().x` minus offset).
    pub width: u32,
    /// Full image height.
    pub height: u32,
    /// Image horizontal offset on the reference grid (`XOsiz`).
    pub x0: u32,
    /// Image vertical offset on the reference grid (`YOsiz`).
    pub y0: u32,
    /// Tile width (`XTsiz`); equals the image extent when untiled.
    pub tile_w: u32,
    /// Tile height (`YTsiz`).
    pub tile_h: u32,
    /// Tile-grid horizontal offset (`XTOsiz`).
    pub tile_x0: u32,
    /// Tile-grid vertical offset (`YTOsiz`).
    pub tile_y0: u32,
    /// Number of image components (`Csiz`).
    pub num_comps: u32,
    /// Bit depth of component 0 (samples are coded as signed `i32` regardless).
    pub bit_depth: u32,
    /// Whether component 0 is signed.
    pub signed: bool,
    /// Wavelet decomposition levels from `COD` (authoritative; resolution count = `num_decomp + 1`).
    pub num_decomp: u32,
    /// Nominal code-block width (`get_block_dims().w`).
    pub cbw: u32,
    /// Nominal code-block height (`get_block_dims().h`).
    pub cbh: u32,
    /// Raw `COD` progression-order code (0=LRCP, 1=RLCP, 2=RPCL, 3=PCRL, 4=CPRL).
    pub prog: u8,
    /// Whether the (reversible RCT / irreversible ICT) multi-component transform is employed.
    pub color_transform: bool,
    /// `true` = reversible (5/3, lossless); `false` = irreversible (9/7, lossy).
    pub reversible: bool,
}

/// Where a code-block sits in the wavelet decomposition (carried so the GPU port can assemble
/// subbands for the inverse DWT). Same shape as [`crate::others::debug_extract::Geom`] plus a
/// `tile` index, but re-exported as `ParseGeom` from the crate root to avoid the name clash.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Geom {
    /// Image component index (0-based).
    pub comp: u32,
    /// Resolution level index (0 = lowest/LL resolution).
    pub res: u32,
    /// Subband index within the resolution (0=LL, 1=HL, 2=LH, 3=HH; res 0 has only LL).
    pub band: u32,
    /// Flat tile index in the tile grid (row-major).
    pub tile: u32,
    /// X origin of the block within its subband (samples).
    pub bx0: u32,
    /// Y origin of the block within its subband (samples).
    pub by0: u32,
    /// Subband width (samples).
    pub band_w: u32,
    /// Subband height (samples).
    pub band_h: u32,
}

/// One code-block's decode plan: everything the entropy stage needs, with **no** decoded output.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodeblockPlan {
    /// The block's coded byte-stream, `len_cup + len_ref` bytes (CUP then SPP/MRP). Owned copy
    /// out of jph-rs's elastic buffer (compressed bytes are bounded, far smaller than decoded).
    pub coded: Vec<u8>,
    /// Cleanup-pass byte length (`pass_length[0]`).
    pub len_cup: u32,
    /// SigProp+MagRef byte length (`pass_length[1]`); 0 when CUP-only.
    pub len_ref: u32,
    /// Coding-pass count (1 on the supported HT cleanup-only path).
    pub num_passes: u32,
    /// Number of all-zero (missing) most-significant bit-planes for this block.
    pub missing_msbs: u32,
    /// Code-block width in samples (clipped to the subband edge).
    pub width: u32,
    /// Code-block height in samples (clipped to the subband edge).
    pub height: u32,
    /// `true` if the reference uses the 64-bit magnitude path (GPU decodes only 32-bit; the
    /// planner must loud-reject these).
    pub is_64bit: bool,
    /// `true` for HT (HTJ2K) block coding; `false` for Part-1 (legacy MQ/EBCOT).
    pub is_ht: bool,
    /// Placement geometry within the wavelet decomposition.
    pub geom: Geom,
}

/// Per-component metadata needed to reconstruct final sample values after a GPU/parse-only decode:
/// bit depth, signedness, and whether the binary-complement NLT (type 3) is in effect for the
/// component. The NLT is applied on encode to signed (typically float-bit) components, so a decoder
/// that bypasses the 1:1 CPU path (e.g. `jph-wgpu`) must invert it per component to get correct
/// samples. `enumerate` surfaces this from `SIZ` + the `NLT` marker (the top-level `enumerate`'s
/// `config` only carries component 0).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompInfo {
    /// Component bit depth (`SIZ`); the NLT/level-shift magnitude derives from this.
    pub bit_depth: u32,
    /// Whether the component is signed (`SIZ`).
    pub signed: bool,
    /// `true` when the component carries the binary-complement NLT (`Tnlt == 3`) — its samples must
    /// be inverse-NLT'd after decode.
    pub nlt_type3: bool,
    /// Reconstructed component width in samples (`ParamSiz::get_recon_width`) — the same value
    /// `mem::decode` uses to size this component's plane. For 1:1 components this equals the image
    /// width; for subsampled chroma (XRsiz>1) it is the `ojph_div_ceil`-reduced dimension. Carried
    /// so a bypass decoder (e.g. `jph-wgpu`) can size per-component planes without re-deriving it.
    pub width: u32,
    /// Reconstructed component height in samples (`ParamSiz::get_recon_height`). See [`Self::width`].
    pub height: u32,
}

/// Result of [`enumerate`]: codec config plus every non-empty code-block's plan, in **resolution-
/// major storage order** (tile → component → resolution 0 LL, then resolution r>0 HL/LH/HH). This is
/// the parsed-tree walk order, NOT the temporal order the decoder pulls/decodes blocks in; consumers
/// must key by [`Geom`], never by index.
#[derive(Clone, Debug, Default)]
pub struct ParsedImage {
    /// Codec-wide configuration from `SIZ`/`COD`.
    pub config: CodecConfig,
    /// Per-component (`bit_depth`, `signed`, `nlt_type3`), index 0..num_comps. Lets a bypass decoder
    /// invert the per-component NLT (the `config` fields cover only component 0).
    pub comps: Vec<CompInfo>,
    /// One plan per non-empty code-block (empty blocks are filtered, matching the decode path).
    pub blocks: Vec<CodeblockPlan>,
}

/// Failure modes of [`enumerate`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// jph-rs rejected the codestream while reading headers/packets (an internal `ojph_error`,
    /// caught so it surfaces as a `Result` instead of unwinding through the caller).
    ParseFailed(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::ParseFailed(msg) => write!(f, "jph parse failed: {msg}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Parse a JPEG 2000 / HTJ2K codestream and enumerate every code-block's decode plan, **without**
/// entropy decode or inverse DWT.
///
/// Drives [`Codestream::read_headers`] + [`Codestream::create`] (which parse all markers,
/// tile-parts, packets, precincts and tag-trees), reads [`CodecConfig`] from `SIZ`/`COD`, then
/// walks the parsed tile tree collecting one [`CodeblockPlan`] per non-empty code-block.
///
/// The 1:1 port reports malformed input as `Err(OjphError)` (see `BUG1.md` "Round 7" — the
/// crate-wide panic-to-`Result` conversion), which this function maps directly to
/// [`ParseError::ParseFailed`]; no `catch_unwind` is needed here any more.
///
/// # Set semantics vs the `debug_extract` oracle
/// `enumerate` lists every **non-empty** code-block (`pass_length[0]>0 && num_passes>0 &&
/// next_coded != null`). This is a *superset* of what `debug_extract` records in one narrow case: a
/// 64-bit-magnitude **Part-1** block, which the decoder force-fails (so the oracle, recording only
/// successfully-decoded blocks, skips it) — `enumerate` still emits it, carrying `is_64bit==true`
/// / `is_ht==false` so the GPU planner can **loud-reject** it (that is the whole point of carrying
/// the flags). A geometry-keyed parity test against the oracle must therefore filter
/// `is_64bit && !is_ht` before comparing. HT blocks (incl. HT-64) never diverge.
///
/// # Assumption: full decode, no resolution reduction
/// This walk enumerates every resolution's bands and assumes no resolution reduction is in effect
/// (`skipped_res_for_read`/`skipped_res_for_recon == 0`, the default for a fresh `Codestream`). When
/// region/LOD decode is added (O3), resolution skipping must be modelled here to stay aligned with
/// the decode path's reduced pull (see `Resolution::pull_line`'s `skipped_res_for_recon` early-out).
pub fn enumerate(codestream: &[u8]) -> Result<ParsedImage, ParseError> {
    let to_parse_error = |e: crate::openjph::ojph_message::OjphError| ParseError::ParseFailed(e.to_string());
    (|| -> Result<ParsedImage, ParseError> {
        let mut infile = MemInfile::default();
        // `open` borrows the slice for `infile`'s lifetime, which fully covers the parse below.
        infile.open(codestream);

        let mut cs = Codestream::new();
        cs.read_headers(&mut infile).map_err(to_parse_error)?;
        // Same untrusted-SIZ gate as `mem::try_decode`, before `create()` allocates its arenas.
        crate::mem::check_siz(cs.access_siz()).map_err(|e| ParseError::ParseFailed(e.to_string()))?;
        cs.create().map_err(to_parse_error)?;

        // Read SIZ first; the `&mut ParamSiz` borrow is dropped at the end of this scope so the
        // `&mut ParamCod` borrow below can re-borrow `cs`.
        let (width, height, x0, y0, tile_w, tile_h, tile_x0, tile_y0, num_comps, bit_depth, signed, comp_bd_signed) = {
            let siz = cs.access_siz();
            let extent = siz.get_image_extent();
            let offset = siz.get_image_offset();
            let tsize = siz.get_tile_size();
            let toff = siz.get_tile_offset();
            let n = u32::from(siz.get_num_components());
            // Per-component (bit_depth, signed, recon_width, recon_height) for every component
            // (config carries only comp 0). Recon dims are the authoritative per-component sizes
            // (`get_recon_*`, same as `mem::decode`); for subsampled chroma they differ from the
            // image extent.
            let mut bs: Vec<(u32, bool, u32, u32)> = Vec::with_capacity(n as usize);
            for c in 0..n {
                bs.push((
                    siz.get_bit_depth(c),
                    siz.is_signed(c),
                    siz.get_recon_width(c),
                    siz.get_recon_height(c),
                ));
            }
            // `width`/`height` are the active image area (extent minus offset), matching how the
            // recon planes are sized in `mem::decode`.
            (
                extent.x.saturating_sub(offset.x),
                extent.y.saturating_sub(offset.y),
                offset.x,
                offset.y,
                tsize.w,
                tsize.h,
                toff.x,
                toff.y,
                n,
                siz.get_bit_depth(0),
                siz.is_signed(0),
                bs,
            )
        };

        // Per-component NLT (binary-complement type 3) from the NLT marker. `get_nonlinear_transform`
        // returns false when no NLT applies to the component (then nlt_type3 = false).
        let comps: Vec<CompInfo> = {
            let nlt = cs.access_nlt();
            comp_bd_signed
                .iter()
                .enumerate()
                .map(|(c, &(bd, sg, w, h))| {
                    let (mut nbd, mut nsg, mut nty) = (0u8, false, 0u8);
                    let has = nlt.get_nonlinear_transform(c as u32, &mut nbd, &mut nsg, &mut nty);
                    CompInfo {
                        bit_depth: bd,
                        signed: sg,
                        nlt_type3: has && nty == 3,
                        width: w,
                        height: h,
                    }
                })
                .collect()
        };

        let (num_decomp, cbw, cbh, prog, color_transform, reversible) = {
            let cod = cs.access_cod();
            (
                u32::from(cod.get_num_decompositions()),
                cod.get_block_dims().w,
                cod.get_block_dims().h,
                cod.get_progression_order(),
                cod.is_employing_color_transform(),
                cod.is_reversible(),
            )
        };

        let config = CodecConfig {
            width,
            height,
            x0,
            y0,
            tile_w,
            tile_h,
            tile_x0,
            tile_y0,
            num_comps,
            bit_depth,
            signed,
            num_decomp,
            cbw,
            cbh,
            prog,
            color_transform,
            reversible,
        };

        let mut blocks: Vec<CodeblockPlan> = Vec::new();
        cs.enumerate_blocks(&mut |p| blocks.push(p));
        cs.close();

        Ok(ParsedImage { config, comps, blocks })
    })()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::{EncodeParams, Image, Plane, encode};

    /// Build a tiny multi-component image, encode it with jph-rs, then enumerate the result and
    /// check the plan is structurally sound: blocks present, every `coded` slice non-empty, at
    /// least one coding pass each, and `num_decomp` round-trips from the encode params. Full
    /// geom-keyed parity vs `debug_extract` is task O0.2 (not here).
    #[test]
    fn enumerate_tiny_multicomp_roundtrips_plan() {
        let w = 40u32;
        let h = 24u32;
        // Two independent components with a simple ramp so the entropy coder produces real blocks.
        let planes: Vec<Plane> = (0..2)
            .map(|c| {
                let samples = (0..w * h)
                    .map(|i| ((i + c * 7) % 251) as i32)
                    .collect::<Vec<i32>>();
                Plane {
                    width: w,
                    height: h,
                    bit_depth: 8,
                    signed: false,
                    samples,
                }
            })
            .collect();
        let image = Image { width: w, height: h, planes };

        let params = EncodeParams {
            num_decompositions: 3,
            ..EncodeParams::default()
        };
        let bytes = encode(&image, &params);

        let parsed = enumerate(&bytes).expect("enumerate must parse a jph-rs-encoded stream");

        assert_eq!(parsed.config.num_comps, 2, "two components expected");
        assert_eq!(
            parsed.config.num_decomp, params.num_decompositions as u32,
            "num_decomp must come authoritatively from COD"
        );
        assert!(parsed.config.reversible, "default encode is reversible");
        assert!(!parsed.blocks.is_empty(), "a non-trivial image must yield blocks");

        for (i, b) in parsed.blocks.iter().enumerate() {
            assert!(!b.coded.is_empty(), "block {i} has empty coded bytes");
            assert_eq!(
                b.coded.len() as u32,
                b.len_cup + b.len_ref,
                "block {i} coded length must equal len_cup + len_ref"
            );
            assert!(b.num_passes >= 1, "block {i} must have >= 1 coding pass");
            assert!(b.width > 0 && b.height > 0, "block {i} has degenerate size");
            assert!(b.geom.comp < 2, "block {i} component index out of range");
        }
    }
}
