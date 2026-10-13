//! Multi-quality-layer packet parsing — a JPEG2000 feature **absent from OpenJPH** (which jph-rs
//! ports 1:1 and which errors on `num_layers != 1`). This module adds it without touching the
//! bit-exact single-layer path, validated against the OpenHTJ2K reference decoder.
//!
//! Port reference: OpenHTJ2K `j2k_precinct_subband::parse_packet_header` (ITU-T T.800 Annex B.10).
//! The single-layer parse in [`super::ojph_precinct`] stays the authority for `num_layers == 1`.
//!
//! This file is safe, idiomatic Rust (no raw pointers): the multi-layer state lives in owned
//! [`MlTree`] buffers, so the manual-memory single-layer code is unaffected.
//!
//! Build-up in progress: the tag-tree, per-precinct state, inclusion/ZBP walks, the HT
//! segment-length parser, the per-layer body accumulator, and the contiguous-buffer finalizer are
//! in place; only the tile-level progression wiring (a layer dimension on every progression order)
//! and the OpenHTJ2K-oracle validation remain. Items not yet wired are allowed to be unused.
//!
//! Body model (critical): the HT block decoder reads each code-block from ONE contiguous buffer
//! (`CodedCbHeader::next_coded.buf + PREFIX`, length `pass_length[0]+[1]`) and never walks the
//! `next_list` chain. So a block's per-layer body contributions are concatenated into a single
//! [`MlBlock::coded`] accumulator and materialised once, after the last layer, by [`finalize_band`].

/// Source of packed packet headers (PPM `0xFF60` main-header / PPT `0xFF61` tile) for one
/// tile-part. When present, the packet **header** bits are read from `buf` (advancing `pos`) while
/// the packet **bodies** stay in the tile stream. `None` everywhere is the default JPEG 2000 layout
/// (headers inline before each body) and leaves the bit-exact path untouched.
pub struct PacketHeaderReader {
    pub buf: Vec<u8>,
    pub pos: usize,
    /// Persistent bit-reader state across packets: the headers in a PPM/PPT buffer form ONE
    /// continuous bit stream (the bit accumulator / 0xFF-unstuff carry survives packet boundaries,
    /// exactly like OpenHTJ2K's `buf_chain` which only resets on `activate()`). Re-initialising a
    /// fresh reader per packet would drop the `unstuff` carry and desync when no EPH marker forces
    /// a clean byte boundary.
    pub tmp: u32,
    pub avail_bits: i32,
    pub unstuff: bool,
}

/// `ceil(log2(x))` for the tag-tree depth (matches `ojph_precinct::log2ceil`).
fn log2ceil(x: u32) -> u32 {
    if x <= 1 {
        return 0;
    }
    let t = 31 - x.leading_zeros();
    t + if x & (x - 1) != 0 { 1 } else { 0 }
}

/// A stateful tag tree over a `w × h` code-block grid that **persists across quality layers**
/// (T.800 B.10). Unlike the single-layer binary tree, each node carries a running `current` value
/// (the threshold counter), a finalized `value`, and a `state` flag (resolved or not). Level 0 is
/// the leaves; the top level is the single 1×1 root.
pub struct MlTree {
    /// Number of levels (0..n_levels-1); the top level is 1×1 (the root).
    pub n_levels: usize,
    wl: Vec<usize>,
    off: Vec<usize>,
    /// Finalized value once `state` is set.
    pub value: Vec<u16>,
    /// Running threshold counter during decode.
    pub current: Vec<u16>,
    /// Whether the node's value is finalized.
    pub state: Vec<bool>,
}

impl MlTree {
    /// Build a tag tree for a `w × h` code-block grid (both ≥ 1).
    pub fn new(w: usize, h: usize) -> Self {
        let top = log2ceil(w as u32).max(log2ceil(h as u32)) as usize;
        let n_levels = top + 1;
        let (mut wl, mut off) = (Vec::new(), Vec::new());
        let mut total = 0usize;
        for lev in 0..n_levels {
            let lw = (w + (1 << lev) - 1) >> lev;
            let lh = (h + (1 << lev) - 1) >> lev;
            wl.push(lw);
            off.push(total);
            total += lw * lh;
        }
        MlTree {
            n_levels,
            wl,
            off,
            value: vec![0; total],
            current: vec![0; total],
            state: vec![false; total],
        }
    }

    /// Flat index of node `(x, y)` at `lev` (coordinates already shifted into that level).
    #[inline]
    pub fn idx(&self, x: usize, y: usize, lev: usize) -> usize {
        self.off[lev] + y * self.wl[lev] + x
    }

    /// The top (root) level index = `n_levels - 1` (a 1×1 grid).
    #[inline]
    pub fn top_level(&self) -> usize {
        self.n_levels - 1
    }
}

// HT code-block coding-mode flags (OpenHTJ2K open_htj2k_typedef.hpp).
/// Max coding passes guard (OpenHTJ2K kMaxCodingPasses).
const K_MAX_PASSES: u32 = 128;

/// Per-code-block multi-layer parse state not held by `CodedCbHeader`: the running coding mode
/// and the HT fast-skip placeholder count (cleanup/refinement byte totals and `num_passes`
/// accumulate directly on the header).
pub struct MlBlock {
    pub cmodes: u16,
    /// Zero-bit-plane count from the inclusion ZBP tag tree (`num_ZBP`), captured when the block is
    /// first included; folded into `missing_msbs` at [`finalize_band`] together with the placeholder
    /// passes (T.814: `S_blk = P0 + num_ZBP`).
    pub num_zbp: u32,
    /// Per-coding-pass byte lengths in decode order, accumulated across layers (OpenHTJ2K
    /// `pass_length[]`). One non-zero entry per coding-pass segment, at the segment's last pass
    /// index; leading zero entries are placeholder passes. The HT decode reduces these to the
    /// cleanup/refinement segment lengths (see [`finalize_band`]).
    pub pass_lengths: Vec<u32>,
    /// Bytes this code-block contributes in the current layer's packet body.
    pub this_layer_bytes: u32,
    /// Accumulated cleanup‖refinement bytes across all quality layers, in stream order. Becomes the
    /// single contiguous `next_coded` buffer the HT block decoder consumes (see [`finalize_band`]).
    pub coded: Vec<u8>,
    /// HT vs Part-1 (legacy MQ/EBCOT) coding for this block's component. Part-1 blocks skip the HT
    /// placeholder/segment machinery: a single MQ segment spans all passes (see [`finalize_block`]).
    pub is_ht: bool,
    /// Part-1 segmenting styles (`SEG_MODES`: BYPASS/RESTART) of this block's component. When
    /// non-zero the per-pass `pass_lengths` become the decoder's `seg_lengths` array (segment
    /// boundaries per [`seg_end`]) instead of collapsing to one combined length. Zero for default
    /// Part-1 and HT blocks.
    pub style: u16,
}

impl Default for MlBlock {
    fn default() -> Self {
        MlBlock {
            cmodes: HT,
            num_zbp: 0,
            pass_lengths: Vec::new(),
            this_layer_bytes: 0,
            coded: Vec::new(),
            is_ht: true,
            style: 0,
        }
    }
}

/// One subband's persistent multi-layer state: inclusion + zero-bit-plane tag trees and per-block
/// state, surviving across the precinct's per-layer packet parses.
pub struct MlBand {
    pub inc: MlTree,
    pub zbp: MlTree,
    pub blocks: Vec<MlBlock>,
}

impl MlBand {
    pub fn new(gw: usize, gh: usize) -> Self {
        MlBand {
            inc: MlTree::new(gw, gh),
            zbp: MlTree::new(gw, gh),
            blocks: (0..gw * gh).map(|_| MlBlock::default()).collect(),
        }
    }
}

/// One precinct's persistent multi-layer state across ALL quality layers of a tile. A subband's
/// [`MlBand`] is created lazily the first time that subband is seen non-empty (T.800 B.10). This
/// state MUST outlive the per-layer packet parses: for LRCP/RLCP the same precinct is revisited
/// once per layer, non-consecutively, over the whole tile parse, so it cannot be a per-packet
/// local. Subband slot order is the usual LL/HL/LH/HH (the resolution-0 case uses slot 0 only).
#[derive(Default)]
pub struct MlPrecinct {
    pub bands: [Option<MlBand>; 4],
}

use crate::codestream::ojph_bitbuffer_read::{BitReadBuf, bb_read_bit};
use crate::openjph::ojph_message::OjphResult;

/// Read `n` bits MSB-first from the packet-header bit-reader (OpenHTJ2K `get_N_bits`). A length
/// field wider than 32 bits is impossible in a well-formed codestream (it would also silently lose
/// the high bits of `v`); reject loudly rather than return a corrupt value or spin a huge loop.
fn read_n_bits(bb: &mut BitReadBuf, n: u32) -> OjphResult<u32> {
    if n > 32 {
        return Err(crate::ojph_error!(
            0x000300B0,
            "multi-layer packet: a {n}-bit length field exceeds 32 bits (corrupt codestream)"
        ));
    }
    let mut v = 0u32;
    for _ in 0..n {
        v = (v << 1) | read_bit(bb)?;
    }
    Ok(v)
}

/// Read one header bit. `bb_read_bit` reports `Ok(false)` (and yields a 0 bit) once the packet
/// data is exhausted; treating that as data would let tag-tree walks spin forever on a truncated
/// tile-part, so it is an error exactly as in the single-layer parser (`Precinct::parse`).
fn read_bit(bb: &mut BitReadBuf) -> OjphResult<u32> {
    let mut bit = 0u32;
    if !bb_read_bit(bb, &mut bit)? {
        return Err(crate::ojph_error!(
            0x000300E2,
            "multi-layer packet header is truncated (ran out of data)"
        ));
    }
    Ok(bit)
}

/// Largest value a tag-tree leaf may take (zero bit-planes / first-inclusion layer). A longer run of
/// 0 bits is a corrupt stream; bounding it keeps the `u16` counters from wrapping.
const TT_MAX: u16 = 1 << 12;

/// Tag-tree decode of leaf `(x, y)` (T.800 B.10.2; OpenJPEG `tgt_decode`). Walks root→leaf and, at
/// every unresolved node, reads bits while `current <= threshold` (a `1` resolves the node to
/// `current`, a `0` raises it). Returns whether the leaf is resolved with value `<= threshold`.
///
/// The per-node read loop is what makes the walk independent of how often the leaf was queried
/// before: after an EMPTY packet (no queries) the node lags behind the layer and must consume one
/// bit per skipped threshold. Inclusion uses `threshold = layer`; the zero-bit-plane tree uses
/// `u16::MAX` ("until resolved").
fn tt_walk(bb: &mut BitReadBuf, t: &mut MlTree, x: usize, y: usize, threshold: u16) -> OjphResult<bool> {
    let top = t.top_level();
    for lev in (0..=top).rev() {
        let ni = t.idx(x >> lev, y >> lev, lev);
        if t.state[ni] {
            continue;
        }
        if lev < top {
            let pi = t.idx((x >> lev) >> 1, (y >> lev) >> 1, lev + 1);
            t.current[ni] = t.current[ni].max(t.current[pi]);
        }
        while !t.state[ni] && t.current[ni] <= threshold {
            if read_bit(bb)? == 1 {
                t.value[ni] = t.current[ni];
                t.state[ni] = true;
            } else if t.current[ni] >= TT_MAX {
                return Err(crate::ojph_error!(
                    0x000300E9,
                    "tag-tree value exceeds {TT_MAX} (corrupt codestream)"
                ));
            } else {
                t.current[ni] += 1;
            }
        }
    }
    let leaf = t.idx(x, y, 0);
    Ok(t.state[leaf] && t.value[leaf] <= threshold)
}

/// Zero-bit-plane count of leaf `(x, y)`: walk the tree until the leaf resolves.
fn zbp_decode(bb: &mut BitReadBuf, t: &mut MlTree, x: usize, y: usize) -> OjphResult<u16> {
    tt_walk(bb, t, x, y, u16::MAX)?;
    Ok(t.value[t.idx(x, y, 0)])
}

use crate::codestream::ojph_codeblock::CodedCbHeader;
use crate::coding::ojph_cmodes::{HT, HT_MIXED, HT_PHLD, seg_end};
use crate::openjph::ojph_file::{InfileBase, InfileSeek};
use crate::openjph::ojph_mem::{CodedLists, mem_elastic_allocator};

/// Decode the "number of new coding passes in this layer" code (T.800 Table B.4).
fn decode_num_passes(bb: &mut BitReadBuf) -> OjphResult<u32> {
    let mut n = 1u32;
    n += read_bit(bb)?;
    if n >= 2 {
        n += read_bit(bb)?;
        if n >= 3 {
            n += read_n_bits(bb, 2)?;
            if n >= 6 {
                n += read_n_bits(bb, 5)?;
                if n >= 37 {
                    n += read_n_bits(bb, 7)?;
                }
            }
        }
    }
    Ok(n)
}

/// Parse one quality layer's contribution to the code-blocks of a single subband within a precinct
/// packet. Faithful port of the HT path of OpenHTJ2K `j2k_precinct_subband::parse_packet_header`,
/// accumulating per-block `cleanup`/`refinement` byte totals (→ `pass_length[0]`/`[1]`) and the
/// bytes this layer adds to the body (`MlBlock::this_layer_bytes`). HT blocks have ≤3 effective
/// passes; placeholder passes are folded into `missing_msbs` (as in the single-layer path).
///
/// # Safety
/// `coded_cbs` must point to the subband's `CodedCbHeader` array with `band_width` stride; the
/// `(org_x, org_y, gw, gh)` window must lie within it.
#[allow(clippy::too_many_arguments)]
pub unsafe fn parse_subband_ml(
    bb: &mut BitReadBuf,
    coded_cbs: *mut CodedCbHeader,
    band_width: u32,
    org_x: u32,
    org_y: u32,
    gw: usize,
    gh: usize,
    band: &mut MlBand,
    layer_idx: u16,
    ccap15: u16,
    is_ht: bool,
    style: u16,
) -> OjphResult<()> {
    // The per-block grid is fixed per precinct/subband; it must match the band allocated on the
    // first layer that saw this subband (guards against an inconsistent re-entry).
    debug_assert_eq!(band.blocks.len(), gw * gh, "MlBand grid mismatch across layers");
    for idx in 0..gw * gh {
        let (x, y) = (idx % gw, idx / gw);
        let cp = unsafe { &mut *coded_cbs.add((org_x + x as u32 + (org_y + y as u32) * band_width) as usize) };
        let mlb = &mut band.blocks[idx];
        mlb.this_layer_bytes = 0;
        mlb.is_ht = is_ht;
        // HT blocks never use the Part-1 segmenting styles (HT block styles forbid them).
        mlb.style = if is_ht { 0 } else { style };

        // ---- inclusion ----
        let included;
        if !cp.already_included {
            if is_ht {
                mlb.cmodes |= HT_PHLD;
                if ccap15 & 0xC000 != 0 {
                    mlb.cmodes |= HT_MIXED;
                }
            } else {
                // Part-1: plain block coding, no HT placeholder/MIXED machinery. The non-HT
                // first-segment and continuation branches below then size each layer's contribution
                // as a single `Lblock + floor(log2(passes))` segment.
                mlb.cmodes = 0;
            }
            included = tt_walk(bb, &mut band.inc, x, y, layer_idx)?;
            if included {
                cp.already_included = true;
                let zbp = zbp_decode(bb, &mut band.zbp, x, y)?;
                mlb.num_zbp = zbp as u32;
                cp.lblock = 3;
            }
        } else {
            included = read_bit(bb)? == 1;
        }
        if !included {
            continue;
        }

        // ---- this layer's coding passes (faithful port of OpenHTJ2K parse_packet_header) ----
        let new_passes = decode_num_passes(bb)?;
        while read_bit(bb)? == 1 {
            cp.lblock += 1;
            // Lblock is small for any real stream; an unbounded run of 1-bits is corrupt and would
            // push `bits_to_read` past 32 (and overflow `lblock`). Reject loudly.
            if cp.lblock > 32 {
                return Err(crate::ojph_error!(
                    0x000300B2,
                    "multi-layer Lblock grew beyond 32 bits (corrupt codestream)"
                ));
            }
        }

        // First segment of this layer. `pass_index` is the running pass total before this layer.
        let pass_index = cp.num_passes;
        let mut segment_bytes;
        let mut segment_passes: u32;
        let mut next_segment_passes = 0u32;
        let mut bits_to_read;

        if mlb.cmodes & HT_PHLD != 0 {
            // Block has not yet produced its HT Cleanup pass; placeholder passes may precede it.
            let href = (pass_index + new_passes - 1) % 3;
            segment_passes = new_passes.saturating_sub(href);
            let mut pass_bound = 2u32;
            bits_to_read = cp.lblock;
            if segment_passes < 1 {
                // No HT Cleanup here: placeholder passes (or a raw J2K segment in MIXED mode).
                segment_passes = new_passes;
                while pass_bound <= segment_passes {
                    bits_to_read += 1;
                    pass_bound += pass_bound;
                }
                segment_bytes = read_n_bits(bb, bits_to_read)?;
                if segment_bytes != 0 && mlb.cmodes & HT_MIXED != 0 {
                    mlb.cmodes &= !(HT_PHLD | HT);
                }
            } else {
                while pass_bound <= segment_passes {
                    bits_to_read += 1;
                    pass_bound += pass_bound;
                }
                segment_bytes = read_n_bits(bb, bits_to_read)?;
                if segment_bytes != 0 {
                    if mlb.cmodes & HT_MIXED == 0 {
                        // First HT Cleanup pass.
                        next_segment_passes = 2;
                        mlb.cmodes &= !HT_PHLD;
                    } else if cp.lblock > 3
                        && segment_bytes > 1
                        && (segment_bytes >> (bits_to_read - 1)) == 0
                    {
                        // First HT Cleanup pass (length MSB is 0).
                        next_segment_passes = 2;
                        mlb.cmodes &= !HT_PHLD;
                    } else {
                        // Original (non-HT) block coding pass.
                        mlb.cmodes &= !(HT_PHLD | HT);
                        segment_passes = new_passes;
                        // segment_bytes is built bit-by-bit here, so `bits_to_read` is no longer
                        // consulted (the loop bound is `pass_bound`); don't track it further.
                        while pass_bound <= segment_passes {
                            pass_bound += pass_bound;
                            segment_bytes = (segment_bytes << 1) + read_bit(bb)?;
                        }
                    }
                } else {
                    // Placeholder passes: read an extra length bit to confirm.
                    segment_passes = new_passes;
                    if pass_bound <= segment_passes {
                        loop {
                            pass_bound += pass_bound;
                            segment_bytes = (segment_bytes << 1) + read_bit(bb)?;
                            if pass_bound > segment_passes {
                                break;
                            }
                        }
                        if segment_bytes != 0 && mlb.cmodes & HT_MIXED != 0 {
                            mlb.cmodes &= !(HT_PHLD | HT);
                        }
                    }
                }
            }
        } else if mlb.cmodes & HT != 0 {
            // Layer commences with a non-initial HT coding pass.
            let sp = cp.num_passes % 3;
            if sp == 0 {
                segment_passes = 1;
                next_segment_passes = 2;
                bits_to_read = cp.lblock;
            } else {
                segment_passes = if new_passes > 1 { 3 - sp } else { 1 };
                next_segment_passes = 1;
                bits_to_read = cp.lblock + segment_passes - 1;
            }
            segment_bytes = read_n_bits(bb, bits_to_read)?;
        } else {
            // Non-HT block (Part-1 component, or MIXED-mode original J2K): this layer's passes
            // continue the open codeword segment (if any) and then open new ones; each contribution
            // is `Lblock + floor(log2(passes))` bits (T.800 B.10.7.2), cut at `seg_end` boundaries.
            let mut left = new_passes;
            while left > 0 {
                let take = (seg_end(mlb.style, cp.num_passes) - cp.num_passes).min(left);
                let bytes = read_n_bits(bb, cp.lblock + int_log2(take))?;
                push_pass(mlb, cp, take, bytes)?;
                left -= take;
            }
            continue;
        }

        push_pass(mlb, cp, segment_passes, segment_bytes)?;
        let mut remaining = new_passes - segment_passes;

        // Remaining passes of this layer. The per-pass lengths are all we persist — OpenHTJ2K's
        // primary/secondary/fast_skip_bytes bookkeeping is recomputed at decode time from the
        // leading zero-length passes, so it is not stored here.
        if (mlb.cmodes & (HT | HT_PHLD)) == HT {
            while remaining > 0 {
                let sp = if remaining > 1 { next_segment_passes } else { 1 };
                next_segment_passes = 3 - next_segment_passes;
                let bits = cp.lblock + sp - 1;
                let seg = read_n_bits(bb, bits)?;
                remaining -= sp;
                push_pass(mlb, cp, sp, seg)?;
            }
        } else {
            // A non-HT block takes the dedicated segment loop above, and the placeholder branch
            // consumes every pass of a non-HT contribution, so nothing can remain here.
            debug_assert_eq!(remaining, 0);
        }
    }
    Ok(())
}

/// Floor-`log2` for the non-HT length-bit count (OpenHTJ2K `int_log2`).
fn int_log2(x: u32) -> u32 {
    if x <= 1 { 0 } else { 31 - x.leading_zeros() }
}

/// Append one coding-pass segment to a block's running per-pass length array (OpenHTJ2K stores the
/// whole segment's bytes at its LAST pass index, zero-filling the segment's earlier passes), grow
/// `num_passes`, and add the bytes to this layer's body total.
fn push_pass(
    mlb: &mut MlBlock,
    cp: &mut CodedCbHeader,
    segment_passes: u32,
    segment_bytes: u32,
) -> OjphResult<()> {
    cp.num_passes += segment_passes;
    // OpenHTJ2K caps coding passes at kMaxCodingPasses; a larger count is corrupt and would let
    // `pass_lengths` grow unbounded — reject loudly (matches the reference `throw`).
    if cp.num_passes > K_MAX_PASSES {
        return Err(crate::ojph_error!(
            0x000300B1,
            "multi-layer code-block exceeds {K_MAX_PASSES} coding passes (corrupt codestream)"
        ));
    }
    while (mlb.pass_lengths.len() as u32) < cp.num_passes {
        mlb.pass_lengths.push(0);
    }
    if cp.num_passes > 0 {
        let idx = (cp.num_passes - 1) as usize;
        mlb.pass_lengths[idx] = segment_bytes;
    }
    mlb.this_layer_bytes = mlb.this_layer_bytes.saturating_add(segment_bytes);
    Ok(())
}

/// Reduce a block's per-pass lengths to the HT Cleanup/Refinement segment lengths the block decoder
/// expects, and fold placeholder passes into `missing_msbs` (faithful port of OpenHTJ2K
/// `htj2k_decode` length setup, ITU-T T.814):
/// * `P0` = (number of leading zero-length passes) / 3 = placeholder HT Sets;
/// * `missing_msbs = num_ZBP + P0`; effective `num_passes = num_passes − 3·P0`;
/// * `pass_length[0]` (cleanup) = first non-empty pass; `pass_length[1]` (refinement) = sum of the
///   rest. The coded buffer is the non-empty passes concatenated in order (cleanup‖refinement).
///
/// Returns `true` when the block contributes a decodable cleanup segment.
fn finalize_block(cp: &mut CodedCbHeader, mlb: &MlBlock) -> bool {
    let num_passes = cp.num_passes;
    if num_passes == 0 {
        cp.pass_length = [0, 0];
        return false;
    }
    let total: u64 = mlb.pass_lengths.iter().map(|&l| l as u64).sum();
    // The parser clears `HT` from a block's modes when its first segment proves it is an original
    // (non-HT) block in a MIXED component, so the effective coder is decided per block here.
    let ht = mlb.is_ht && mlb.cmodes & HT != 0;
    cp.orig_j2k = mlb.is_ht && !ht;
    if !ht {
        // Part-1: the whole block is one MQ codeword segment spanning every pass; no placeholder
        // folding (missing_msbs comes solely from the ZBP tag tree). Store the total byte count in
        // pass_length[0] (what the Part-1 decoder reads) and leave the refinement slot empty.
        if total == 0 || total > u32::MAX as u64 {
            cp.pass_length = [0, 0];
            cp.num_passes = 0;
            return false;
        }
        cp.pass_length = [total as u32, 0];
        cp.missing_msbs = mlb.num_zbp;
        return true;
    }
    let p0 = if num_passes > 3 {
        let lead = mlb.pass_lengths.iter().take_while(|&&l| l == 0).count() as u32;
        lead / 3
    } else if total == 0 {
        1
    } else {
        0
    };
    let empty_passes = p0 * 3;
    if num_passes < empty_passes {
        cp.pass_length = [0, 0];
        cp.num_passes = 0;
        return false;
    }
    let num_ht_passes = num_passes - empty_passes;
    let mut segs = mlb.pass_lengths.iter().copied().filter(|&l| l != 0);
    let Some(len_cup) = segs.next() else {
        cp.pass_length = [0, 0];
        cp.num_passes = 0;
        return false;
    };
    if num_ht_passes == 0 {
        cp.pass_length = [0, 0];
        cp.num_passes = 0;
        return false;
    }
    // `total` is bounded by the tile-part size (read_subband_body_ml rejects over-claims), so the
    // refinement sum fits in u32; guard the cast regardless.
    let len_ref: u64 = segs.map(|l| l as u64).sum();
    if len_ref > u32::MAX as u64 {
        cp.pass_length = [0, 0];
        cp.num_passes = 0;
        return false;
    }
    cp.pass_length = [len_cup, len_ref as u32];
    cp.missing_msbs = mlb.num_zbp + p0;
    cp.num_passes = num_ht_passes;
    true
}

/// Read one quality layer's body contribution for a subband: each included block's
/// `this_layer_bytes` raw bytes, appended to its accumulator ([`MlBlock::coded`]). Mirrors the
/// single-layer body loop in [`super::ojph_precinct`], but per-layer and accumulating. The layer's
/// bytes are zero-padded to the expected length on a truncated tile-part (matching `bb_read_chunk`,
/// which zero-fills short reads) so the accumulator length stays consistent with `pass_length`.
/// Block order is raster within the subband — the same order [`parse_subband_ml`] used for headers.
/// With `skip` (resolution dropped by `restrict_input_resolution`) the bodies are only stepped over:
/// the headers had to be parsed to find where the next packet starts, but nothing is accumulated.
pub fn read_subband_body_ml(
    file: &mut dyn InfileBase,
    bytes_left: &mut u32,
    band: &mut MlBand,
    gw: usize,
    gh: usize,
    skip: bool,
) -> OjphResult<()> {
    for idx in 0..gw * gh {
        let n = band.blocks[idx].this_layer_bytes as usize;
        if n == 0 {
            continue;
        }
        // A segment claiming more bytes than remain in the tile-part is corrupt/truncated. Reject
        // loudly instead of allocating the (attacker-controlled) claimed size — for a well-formed
        // stream `n` never exceeds the bytes still owed to this and later blocks.
        if n as u64 > *bytes_left as u64 {
            return Err(crate::ojph_error!(
                0x000300B4,
                "multi-layer code-block claims {n} body bytes but only {} remain (corrupt or truncated codestream)",
                *bytes_left
            ));
        }
        if skip {
            if file.seek(n as i64, InfileSeek::OjphSeekCur) != 0 {
                return Err(crate::ojph_error!(0x000300EA, "cannot skip a multi-layer code-block body"));
            }
            *bytes_left -= n as u32;
            continue;
        }
        let start = band.blocks[idx].coded.len();
        band.blocks[idx].coded.resize(start + n, 0);
        let read = file.read(&mut band.blocks[idx].coded[start..start + n]);
        *bytes_left -= read as u32;
        // A short (I/O-truncated) read leaves the tail (already resized to `n`) zeroed.
    }
    Ok(())
}

/// Build a single contiguous [`CodedLists`] node holding `coded` (the block's full cleanup‖
/// refinement bytes, accumulated across layers), laid out exactly as `bb_read_chunk` does: payload
/// at `PREFIX_BUF_SIZE`, then `SUFFIX_BUF_SIZE` trailing zero-pad. The HT block decoder reads from
/// this one buffer and never follows `next_list`, so per-layer contributions are concatenated here
/// rather than chained.
///
/// # Safety
/// `elastic` must point to a live allocator; the returned node is owned by it.
unsafe fn build_coded_lists(
    coded: &[u8],
    elastic: *mut mem_elastic_allocator,
) -> OjphResult<*mut CodedLists> {
    let prefix = CodedCbHeader::PREFIX_BUF_SIZE as usize;
    let suffix = CodedCbHeader::SUFFIX_BUF_SIZE as usize;
    let total = coded.len() + prefix + suffix;
    // `get_buffer` takes a u32 size; a larger request would truncate and under-allocate, turning the
    // copy below into a heap overflow. Reject loudly (unreachable for a valid ≤u32-byte code-block).
    if total > u32::MAX as usize {
        return Err(crate::ojph_error!(
            0x000300B3,
            "multi-layer code-block of {total} bytes exceeds the u32 buffer limit (corrupt codestream)"
        ));
    }
    let mut p: *mut CodedLists = core::ptr::null_mut();
    unsafe {
        (*elastic).get_buffer(total as u32, &mut p);
        let dst = (*p).buf.add(prefix);
        core::ptr::copy_nonoverlapping(coded.as_ptr(), dst, coded.len());
        core::ptr::write_bytes(dst.add(coded.len()), 0, suffix);
    }
    Ok(p)
}

/// After the final quality layer, fold placeholder passes ([`finalize_block`]) and materialise each
/// block's accumulated bytes into a single contiguous `next_coded` node for the HT block decoder.
///
/// # Safety
/// The `(coded_cbs, band_width, org_x, org_y, gw, gh)` window must address the subband's
/// `CodedCbHeader` array (as in [`parse_subband_ml`]); `elastic` must be live.
#[allow(clippy::too_many_arguments)]
pub unsafe fn finalize_band(
    coded_cbs: *mut CodedCbHeader,
    band_width: u32,
    org_x: u32,
    org_y: u32,
    gw: usize,
    gh: usize,
    band: &mut MlBand,
    elastic: *mut mem_elastic_allocator,
) -> OjphResult<()> {
    for idx in 0..gw * gh {
        let (x, y) = (idx % gw, idx / gw);
        let cp = unsafe {
            &mut *coded_cbs.add((org_x + x as u32 + (org_y + y as u32) * band_width) as usize)
        };
        let mlb = &band.blocks[idx];
        if finalize_block(cp, mlb) && !mlb.coded.is_empty() {
            cp.next_coded = unsafe { build_coded_lists(&mlb.coded, elastic)? };
            // Part-1 RESTART: hand the decoder the per-pass byte lengths (each pass is its own MQ
            // segment). `mlb.pass_lengths` already holds one entry per pass; copy them into an
            // elastic-store array and point `seg_lengths` at it (same layout as the single-layer
            // parser). Non-RESTART blocks leave `seg_count == 0` and decode as a single segment.
            if mlb.style != 0 && cp.num_passes > 0 {
                let n = cp.num_passes;
                let mut seg_cl: *mut CodedLists = core::ptr::null_mut();
                unsafe { (*elastic).get_buffer(n.saturating_mul(4), &mut seg_cl) };
                let seg_ptr = unsafe { (*seg_cl).buf } as *mut u32;
                for z in 0..n as usize {
                    unsafe { *seg_ptr.add(z) = mlb.pass_lengths[z] };
                }
                cp.seg_lengths = seg_ptr;
                cp.seg_count = n as u16;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry() {
        // 3×2 grid: levels 0 (3×2), 1 (2×1), 2 (1×1 root).
        let t = MlTree::new(3, 2);
        assert_eq!(t.n_levels, 2 + 1 - 1 + 1, "n_levels for 3x2"); // top=max(log2ceil3,log2ceil2)=2
        assert_eq!(t.n_levels, 3);
        assert_eq!(t.top_level(), 2);
        // leaf count = 6, level1 = ceil(3/2)*ceil(2/2)=2*1=2, root = 1.
        assert_eq!(t.value.len(), 6 + 2 + 1);
        // index sanity: leaf (2,1) and root (0,0).
        assert_eq!(t.idx(2, 1, 0), 0 + 1 * 3 + 2);
        assert_eq!(t.idx(0, 0, 2), 6 + 2);
    }

    #[test]
    fn single_node() {
        let t = MlTree::new(1, 1);
        assert_eq!(t.n_levels, 1);
        assert_eq!(t.top_level(), 0);
        assert_eq!(t.value.len(), 1);
    }

    /// Test-only tag-tree ENCODER (T.800 B.10.2; OpenJPEG `tgt_encode`) over an [`MlTree`] geometry.
    struct TtEnc {
        geo: MlTree,
        val: Vec<u16>,
        low: Vec<u16>,
        known: Vec<bool>,
    }

    impl TtEnc {
        /// Internal node values are the minimum of their children (leaf values given row-major).
        fn new(w: usize, h: usize, leaves: &[u16]) -> Self {
            let geo = MlTree::new(w, h);
            let mut val = vec![u16::MAX; geo.value.len()];
            for lev in 0..geo.n_levels {
                for y in 0..(h + (1 << lev) - 1) >> lev {
                    for x in 0..(w + (1 << lev) - 1) >> lev {
                        let i = geo.idx(x, y, lev);
                        val[i] = if lev == 0 {
                            leaves[y * w + x]
                        } else {
                            let mut m = u16::MAX;
                            for (cx, cy) in [(2 * x, 2 * y), (2 * x + 1, 2 * y), (2 * x, 2 * y + 1), (2 * x + 1, 2 * y + 1)] {
                                if cx < (w + (1 << (lev - 1)) - 1) >> (lev - 1) && cy < (h + (1 << (lev - 1)) - 1) >> (lev - 1) {
                                    m = m.min(val[geo.idx(cx, cy, lev - 1)]);
                                }
                            }
                            m
                        };
                    }
                }
            }
            let n = val.len();
            TtEnc { geo, val, low: vec![0; n], known: vec![false; n] }
        }

        /// Emit the bits that tell a decoder whether leaf `(x, y)` has value `< threshold`.
        fn encode(&mut self, bits: &mut Vec<bool>, x: usize, y: usize, threshold: u16) {
            let mut low = 0u16;
            for lev in (0..self.geo.n_levels).rev() {
                let ni = self.geo.idx(x >> lev, y >> lev, lev);
                if low > self.low[ni] {
                    self.low[ni] = low;
                } else {
                    low = self.low[ni];
                }
                while low < threshold {
                    if low >= self.val[ni] {
                        if !self.known[ni] {
                            bits.push(true);
                            self.known[ni] = true;
                        }
                        break;
                    }
                    bits.push(false);
                    low += 1;
                }
                self.low[ni] = low;
            }
        }
    }

    /// H5: a precinct whose packet is EMPTY in some layer skips its tag-tree queries, so the decoder's
    /// nodes lag behind the layer and must consume one bit per skipped threshold. Round-trips a
    /// 3x2 inclusion tree through an independent encoder with layer 1 empty (first-inclusion layers
    /// 2,0,3,2,0,4 — none equal to the empty layer, as a real encoder could not include there).
    #[test]
    fn inclusion_tree_survives_empty_layers() {
        use crate::openjph::ojph_file::MemInfile;
        let (w, h) = (3usize, 2usize);
        let first = [2u16, 0, 3, 2, 0, 4];
        let (layers, empty) = (5u16, 1u16);

        let mut enc = TtEnc::new(w, h, &first);
        let mut bits = Vec::new();
        let mut done = [false; 6];
        for l in (0..layers).filter(|&l| l != empty) {
            for i in 0..w * h {
                if done[i] {
                    continue;
                }
                enc.encode(&mut bits, i % w, i / w, l + 1);
                done[i] = first[i] <= l;
            }
        }
        // Byte-pack MSB-first; the stream is sparse in 1s so no 0xFF byte (bit-stuffing) occurs.
        let mut data: Vec<u8> = bits.chunks(8).map(|c| c.iter().enumerate().fold(0u8, |a, (k, &b)| a | (u8::from(b) << (7 - k)))).collect();
        data.extend([0; 4]);
        assert!(!data.contains(&0xFF));

        let mut infile = MemInfile::default();
        infile.open(&data);
        let mut bb = crate::codestream::ojph_bitbuffer_read::bb_init(data.len() as u32, &mut infile);
        let mut tree = MlTree::new(w, h);
        let mut got = [u16::MAX; 6];
        for l in (0..layers).filter(|&l| l != empty) {
            for i in 0..w * h {
                if got[i] != u16::MAX {
                    continue;
                }
                if tt_walk(&mut bb, &mut tree, i % w, i / w, l).expect("walk") {
                    got[i] = l;
                }
            }
        }
        assert_eq!(got, first);
    }

    /// An all-zero header that ends before the ZBP tag-tree resolves must be an error. Previously
    /// `read_bit` ignored end-of-data and `zbp_decode` looped forever on the synthetic zero bits.
    #[test]
    fn zbp_walk_on_exhausted_header_errors_instead_of_hanging() {
        use crate::openjph::ojph_file::MemInfile;
        let data = [0u8; 1];
        let mut infile = MemInfile::default();
        infile.open(&data);
        let mut bb = crate::codestream::ojph_bitbuffer_read::bb_init(data.len() as u32, &mut infile);
        let mut tree = MlTree::new(1, 1);
        assert!(zbp_decode(&mut bb, &mut tree, 0, 0).is_err());
    }
}
