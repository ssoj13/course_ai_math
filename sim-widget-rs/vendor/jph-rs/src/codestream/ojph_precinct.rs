//! Port target:
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/codestream/ojph_precinct.h`
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/codestream/ojph_precinct.cpp`

use crate::codestream::ojph_bitbuffer_read as bb_read;
use crate::codestream::ojph_bitbuffer_write as bb_write;
use crate::codestream::ojph_subband::Subband;
use crate::coding::ojph_cmodes::{SEG_MODES, seg_end};
use crate::openjph::ojph_base::{Point, Rect, Size};
use crate::openjph::ojph_defs::{Ui8, Ui32, ojph_max, ojph_min};
use crate::openjph::ojph_file::{InfileBase, OutfileBase};
use crate::openjph::ojph_mem::{CodedLists, mem_elastic_allocator};
use core::ptr;

/// A JPEG 2000 precinct — the spatial subdivision of a resolution level
/// that groups codeblocks for packet-level encoding.
///
/// Each precinct covers up to 4 subbands (LL/HL/LH/HH) and owns:
/// - a scratch buffer for tag-tree construction,
/// - per-subband codeblock index rects (`cb_idxs`),
/// - a linked list of coded data (`coded`) written by [`prepare_precinct`].
#[repr(C)]
pub struct Precinct {
    /// Scratch memory shared by all four tag trees (inclusion, inclusion-flags,
    /// missing-msbs, missing-msbs-flags). Layout: 4 contiguous regions of
    /// `tag_tree_size` bytes each.
    pub(crate) scratch: *mut Ui8,
    /// Top-left corner of this precinct in canvas coordinates.
    pub(crate) img_point: Point,
    /// Codeblock index rectangles for each of the 4 subbands.
    /// `org` is the first codeblock index, `siz` is the count in each axis.
    pub(crate) cb_idxs: [Rect; 4],
    /// Pointer to the array of 4 Subband descriptors for this resolution level.
    pub(crate) bands: *mut Subband,
    /// Head of the linked list of coded packet data (header + codeblock bytes).
    /// `None` (null) means the precinct has not been prepared yet, or is empty.
    pub(crate) coded: *mut CodedLists,
    /// Whether SOP (Start of Packet) markers may be used.
    pub(crate) may_use_sop: bool,
    /// Whether EPH (End of Packet Header) markers are used.
    pub(crate) uses_eph: bool,
    /// HT (HTJ2K) vs Part-1 (legacy MQ/EBCOT) block coding for this resolution's component. Selects
    /// the per-codeblock packet-header length/pass model (HT folds placeholder passes into
    /// `missing_msbs` and codes two segment lengths; Part-1 codes a single segment length).
    pub(crate) is_ht: bool,
    /// Part-1 coding modes (SPcod/SPcoc low 6 bits) for this resolution's component. Only the
    /// segmenting styles (`SEG_MODES`: BYPASS/RESTART) are consulted here: they split a block into
    /// several codeword segments, so the packet header carries one length per segment (see
    /// `seg_end`) instead of a single combined length. Ignored for HT blocks.
    pub(crate) cmodes: u16,
}

impl Default for Precinct {
    fn default() -> Self {
        Self {
            scratch: ptr::null_mut(),
            img_point: Point::default(),
            cb_idxs: [
                Rect::default(),
                Rect::default(),
                Rect::default(),
                Rect::default(),
            ],
            bands: ptr::null_mut(),
            coded: ptr::null_mut(),
            may_use_sop: false,
            uses_eph: false,
            is_ht: true,
            cmodes: 0,
        }
    }
}

/// Quad-tree structure for JPEG 2000 tag-tree coding (ITU-T T.800 Annex B).
///
/// Stores hierarchical minimum values used to code inclusion and
/// missing-MSBs information in packet headers. Level 0 holds leaf
/// values (one per codeblock), each subsequent level holds the
/// min-of-4-children, up to a single root element.
#[repr(C)]
struct TagTree {
    /// Codeblock grid width at level 0.
    width: Ui32,
    /// Codeblock grid height at level 0.
    height: Ui32,
    /// Number of hierarchy levels (excluding the extra root sentinel).
    num_levels: Ui32,
    /// Pointers into the scratch buffer for each level's data.
    /// `levs[0]` = leaf level, `levs[num_levels]` = root sentinel (always 0).
    /// Unused entries above `num_levels` are set to `usize::MAX` to crash on
    /// accidental access (matches C++ `INT_MAX` sentinel).
    levs: [*mut Ui8; 16],
}

impl Default for TagTree {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            num_levels: 0,
            levs: [ptr::null_mut(); 16],
        }
    }
}

impl TagTree {
    /// Initialize tag tree from a scratch buffer region.
    ///
    /// `lev_idx` provides byte offsets into `buf` for each level.
    /// Leaf values are filled with `init_val`; the root sentinel is set to 0.
    unsafe fn init(
        &mut self,
        buf: *mut Ui8,
        lev_idx: *mut Ui32,
        num_levels: Ui32,
        s: Size,
        init_val: i32,
    ) {
        // Map each level to its region within the shared scratch buffer
        for i in 0..=num_levels as usize {
            self.levs[i] = buf.add(*lev_idx.add(i) as usize);
        }
        // Poison unused slots so any OOB access crashes immediately
        for i in num_levels as usize + 1..16 {
            self.levs[i] = usize::MAX as *mut Ui8;
        }
        self.width = s.w;
        self.height = s.h;
        // Fill each level with init_val; level size = 4^(num_levels-1-i)
        for i in 0..num_levels as usize {
            let size = 1u32 << ((num_levels as usize - 1 - i) << 1);
            ptr::write_bytes(self.levs[i], init_val as u8, size as usize);
        }
        // Root sentinel is always 0 (the "already sent" baseline)
        *self.levs[num_levels as usize] = 0;
        self.num_levels = num_levels;
    }

    /// Returns a mutable pointer to the tag-tree element at `(x, y)` in the
    /// given level. The row stride at each level is `ceil(width / 2^lev)`.
    unsafe fn get(&self, x: Ui32, y: Ui32, lev: Ui32) -> *mut Ui8 {
        self.levs[lev as usize].add((x + y * ((self.width + (1 << lev) - 1) >> lev)) as usize)
    }
}

/// Ceiling of log2 for a positive integer. Used to determine the number of
/// tag-tree levels needed for a given codeblock grid dimension.
#[inline]
fn log2ceil(x: Ui32) -> Ui32 {
    let t = 31 - x.leading_zeros();
    t + if x & (x - 1) != 0 { 1 } else { 0 }
}

impl Precinct {
    /// Encode this precinct's packet header and collect codeblock data sizes.
    ///
    /// Builds tag trees for codeblock inclusion and missing-MSBs, then writes
    /// the packet header bitstream (inclusion bits, missing-MSBs, pass counts,
    /// pass lengths) into the elastic allocator's coded-list chain.
    ///
    /// Returns the total byte count (packet header + codeblock data) that
    /// [`write`] will emit, or 1 for an empty packet.
    ///
    /// # Tag tree layout in scratch buffer
    ///
    /// The scratch buffer is divided into 4 regions of `tag_tree_size` bytes:
    /// - `[0 .. tag_tree_size)` — inclusion tag tree values
    /// - `[tag_tree_size .. 2*tag_tree_size)` — inclusion tag tree flags (sent/not)
    /// - `[2*tag_tree_size .. 3*tag_tree_size)` — missing-MSBs tag tree values
    /// - `[3*tag_tree_size .. 4*tag_tree_size)` — missing-MSBs tag tree flags
    pub fn prepare_precinct(
        &mut self,
        tag_tree_size: i32,
        lev_idx: *mut Ui32,
        elastic: *mut mem_elastic_allocator,
    ) -> Ui32 {
        let mut bb = bb_write::BitWriteBuf::default();
        let mut cur_coded_list = ptr::null_mut();
        let mut cb_bytes = 0u32;
        let mut ph_bytes = 0u32;
        let mut num_skipped_subbands = 0;
        // Iterate over up to 4 subbands: band 0 = LL (only at lowest res),
        // bands 1/2/3 = HL/LH/HH
        for s in 0..4usize {
            unsafe {
                if (*self.bands.add(s)).empty {
                    continue;
                }
                if self.cb_idxs[s].siz.w == 0 || self.cb_idxs[s].siz.h == 0 {
                    continue;
                }

                // Tag tree depth: enough levels to reduce the codeblock grid
                // to a single root element
                let num_levels = 1 + ojph_max(
                    log2ceil(self.cb_idxs[s].siz.w),
                    log2ceil(self.cb_idxs[s].siz.h),
                );

                // Build 4 tag trees from the scratch buffer regions
                let mut inc_tag = TagTree::default();
                let mut inc_tag_flags = TagTree::default();
                let mut mmsb_tag = TagTree::default();
                let mut mmsb_tag_flags = TagTree::default();
                inc_tag.init(self.scratch, lev_idx, num_levels, self.cb_idxs[s].siz, 255);
                inc_tag_flags.init(
                    self.scratch.add(tag_tree_size as usize),
                    lev_idx,
                    num_levels,
                    self.cb_idxs[s].siz,
                    0,
                );
                mmsb_tag.init(
                    self.scratch.add((tag_tree_size << 1) as usize),
                    lev_idx,
                    num_levels,
                    self.cb_idxs[s].siz,
                    255,
                );
                mmsb_tag_flags.init(
                    self.scratch
                        .add(((tag_tree_size << 1) + tag_tree_size) as usize),
                    lev_idx,
                    num_levels,
                    self.cb_idxs[s].siz,
                    0,
                );

                // Fill leaf level (level 0) from codeblock headers
                let band_width = (*self.bands.add(s)).num_blocks.w;
                let mut cp = (*self.bands.add(s))
                    .coded_cbs
                    .add((self.cb_idxs[s].org.x + self.cb_idxs[s].org.y * band_width) as usize);
                for y in 0..self.cb_idxs[s].siz.h {
                    for x in 0..self.cb_idxs[s].siz.w {
                        let p = cp.add(x as usize);
                        // inclusion: 1 if codeblock has no coded data (empty), 0 if included
                        *inc_tag.get(x, y, 0) = ((*p).next_coded.is_null()) as Ui8;
                        *mmsb_tag.get(x, y, 0) = (*p).missing_msbs as Ui8;
                    }
                    cp = cp.add(band_width as usize);
                }

                // Propagate min values up the tree (parent = min of 4 children)
                for lev in 1..num_levels {
                    let height = (self.cb_idxs[s].siz.h + (1 << lev) - 1) >> lev;
                    let width = (self.cb_idxs[s].siz.w + (1 << lev) - 1) >> lev;
                    for y in 0..height {
                        for x in 0..width {
                            let t1 = ojph_min(
                                *inc_tag.get(x << 1, y << 1, lev - 1),
                                *inc_tag.get((x << 1) + 1, y << 1, lev - 1),
                            );
                            let t2 = ojph_min(
                                *inc_tag.get(x << 1, (y << 1) + 1, lev - 1),
                                *inc_tag.get((x << 1) + 1, (y << 1) + 1, lev - 1),
                            );
                            *inc_tag.get(x, y, lev) = ojph_min(t1, t2);
                            *inc_tag_flags.get(x, y, lev) = 0;
                            let t1 = ojph_min(
                                *mmsb_tag.get(x << 1, y << 1, lev - 1),
                                *mmsb_tag.get((x << 1) + 1, y << 1, lev - 1),
                            );
                            let t2 = ojph_min(
                                *mmsb_tag.get(x << 1, (y << 1) + 1, lev - 1),
                                *mmsb_tag.get((x << 1) + 1, (y << 1) + 1, lev - 1),
                            );
                            *mmsb_tag.get(x, y, lev) = ojph_min(t1, t2);
                            *mmsb_tag_flags.get(x, y, lev) = 0;
                        }
                    }
                }

                // Root sentinel: always 0 (the "already coded" baseline)
                *inc_tag.get(0, 0, num_levels) = 0;
                *inc_tag_flags.get(0, 0, num_levels) = 0;
                *mmsb_tag.get(0, 0, num_levels) = 0;
                *mmsb_tag_flags.get(0, 0, num_levels) = 0;

                // If the top-level inclusion value is nonzero, the entire
                // subband is empty (no codeblocks have data)
                if *inc_tag.get(0, 0, num_levels - 1) != 0 {
                    if !self.coded.is_null() {
                        // Precinct already has data from a prior subband —
                        // write a 0 bit to signal this subband is empty
                        bb_write::bb_put_bits(
                            &mut bb,
                            0,
                            1,
                            elastic,
                            &mut cur_coded_list,
                            &mut ph_bytes,
                        );
                    } else {
                        num_skipped_subbands += 1;
                    }
                    continue;
                }

                // First non-empty subband: initialize the bitstream and write
                // the "non-empty packet" flag bit
                if self.coded.is_null() {
                    bb_write::bb_init(&mut bb, elastic, &mut cur_coded_list);
                    self.coded = cur_coded_list;
                    // Non-empty packet indicator (1 bit = 1)
                    bb_write::bb_put_bit(&mut bb, 1, elastic, &mut cur_coded_list, &mut ph_bytes);
                    // Write zero bits for any leading empty subbands.
                    // NOTE: value must be 0, not 0xFFFF_FFFF — we are writing
                    // zeros to signal empty subbands, not ones.
                    bb_write::bb_put_bits(
                        &mut bb,
                        0,
                        num_skipped_subbands,
                        elastic,
                        &mut cur_coded_list,
                        &mut ph_bytes,
                    );
                    num_skipped_subbands = 0;
                }

                // Per-codeblock encoding: walk all codeblocks in raster order
                let width = self.cb_idxs[s].siz.w;
                let height = self.cb_idxs[s].siz.h;
                for y in 0..height {
                    cp = (*self.bands.add(s)).coded_cbs.add(
                        (self.cb_idxs[s].org.x + (y + self.cb_idxs[s].org.y) * band_width) as usize,
                    );
                    for x in 0..width {
                        // Inclusion tag-tree coding: walk from root to leaf,
                        // writing 1 (included) or 0 (not yet) at each unsent node
                        for cur_lev in (1..=num_levels).rev() {
                            let levm1 = cur_lev - 1;
                            if *inc_tag_flags.get(x >> levm1, y >> levm1, levm1) == 0 {
                                let skipped = *inc_tag.get(x >> levm1, y >> levm1, levm1)
                                    - *inc_tag.get(x >> cur_lev, y >> cur_lev, cur_lev);
                                // For HTJ2K, skipped is always 0 or 1
                                debug_assert!(skipped <= 1);
                                bb_write::bb_put_bits(
                                    &mut bb,
                                    1 - skipped as Ui32,
                                    1,
                                    elastic,
                                    &mut cur_coded_list,
                                    &mut ph_bytes,
                                );
                                *inc_tag_flags.get(x >> levm1, y >> levm1, levm1) = 1;
                            }
                            if *inc_tag.get(x >> levm1, y >> levm1, levm1) > 0 {
                                break;
                            }
                        }

                        // Empty codeblock (no coded passes) — skip remaining fields
                        if (*cp).num_passes == 0 {
                            cp = cp.add(1);
                            continue;
                        }

                        // Missing-MSBs tag-tree coding: same root-to-leaf walk
                        for cur_lev in (1..=num_levels).rev() {
                            let levm1 = cur_lev - 1;
                            if *mmsb_tag_flags.get(x >> levm1, y >> levm1, levm1) == 0 {
                                let num_zeros = *mmsb_tag.get(x >> levm1, y >> levm1, levm1) as i32
                                    - *mmsb_tag.get(x >> cur_lev, y >> cur_lev, cur_lev) as i32;
                                bb_write::bb_put_zeros(
                                    &mut bb,
                                    num_zeros,
                                    elastic,
                                    &mut cur_coded_list,
                                    &mut ph_bytes,
                                );
                                bb_write::bb_put_bits(
                                    &mut bb,
                                    1,
                                    1,
                                    elastic,
                                    &mut cur_coded_list,
                                    &mut ph_bytes,
                                );
                                *mmsb_tag_flags.get(x >> levm1, y >> levm1, levm1) = 1;
                            }
                        }

                        // Number of coding passes: HTJ2K uses 1, 2, or 3
                        // Encoding: 1 pass = "0", 2 passes = "10", 3 passes = "1100"
                        match (*cp).num_passes {
                            3 => bb_write::bb_put_bits(
                                &mut bb,
                                12,
                                4,
                                elastic,
                                &mut cur_coded_list,
                                &mut ph_bytes,
                            ),
                            2 => bb_write::bb_put_bits(
                                &mut bb,
                                2,
                                2,
                                elastic,
                                &mut cur_coded_list,
                                &mut ph_bytes,
                            ),
                            1 => bb_write::bb_put_bits(
                                &mut bb,
                                0,
                                1,
                                elastic,
                                &mut cur_coded_list,
                                &mut ph_bytes,
                            ),
                            _ => debug_assert!(false, "unexpected HTJ2K pass count"),
                        }

                        // Pass lengths: compute the minimum bit-width needed to
                        // represent both pass_length[0] and pass_length[1],
                        // write a unary prefix (bits+1 bits of 1s then 0),
                        // then the actual lengths
                        let bits1 = 32 - (*cp).pass_length[0].leading_zeros() as i32;
                        let extra_bit = if (*cp).num_passes > 2 { 1 } else { 0 };
                        let mut bits2 = 0;
                        if (*cp).num_passes > 1 {
                            bits2 = 32 - (*cp).pass_length[1].leading_zeros() as i32;
                        }
                        let bits =
                            ojph_max(bits1 as Ui32, (bits2 - extra_bit).max(0) as Ui32) as i32 - 3;
                        let bits = bits.max(0);
                        // Unary length prefix: `bits` ones followed by a zero
                        bb_write::bb_put_bits(
                            &mut bb,
                            0xFFFF_FFFE,
                            bits + 1,
                            elastic,
                            &mut cur_coded_list,
                            &mut ph_bytes,
                        );
                        // First pass length (cleanup pass)
                        bb_write::bb_put_bits(
                            &mut bb,
                            (*cp).pass_length[0],
                            bits + 3,
                            elastic,
                            &mut cur_coded_list,
                            &mut ph_bytes,
                        );
                        // Second pass length (SigProp + MagRef combined), if present
                        if (*cp).num_passes > 1 {
                            bb_write::bb_put_bits(
                                &mut bb,
                                (*cp).pass_length[1],
                                bits + 3 + extra_bit,
                                elastic,
                                &mut cur_coded_list,
                                &mut ph_bytes,
                            );
                        }

                        cb_bytes += (*cp).pass_length[0] + (*cp).pass_length[1];
                        cp = cp.add(1);
                    }
                }
            }
        }

        // Finalize the bitstream and account for the last buffer's used bytes
        if !self.coded.is_null() {
            bb_write::bb_terminate(&mut bb);
            unsafe {
                ph_bytes += (*cur_coded_list).buf_size - (*cur_coded_list).avail_size;
            }
        }
        // Return total bytes (header + codeblock data), or 1 for empty packet
        if !self.coded.is_null() {
            cb_bytes + ph_bytes
        } else {
            1
        }
    }

    /// Write the precinct's packet to the output file.
    ///
    /// First emits the packet header (built by [`prepare_precinct`]), then
    /// walks each subband's codeblocks in raster order and writes their
    /// coded data. For an empty precinct, writes a single 0x00 byte.
    pub fn write(&mut self, file: &mut dyn OutfileBase) -> crate::openjph::ojph_message::OjphResult<()> {
        if !self.coded.is_null() {
            unsafe {
                // Write packet header buffers
                let mut ccl = self.coded;
                while !ccl.is_null() {
                    file.write(core::slice::from_raw_parts(
                        (*ccl).buf,
                        ((*ccl).buf_size - (*ccl).avail_size) as usize,
                    ))?;
                    ccl = (*ccl).next_list;
                }

                // Write codeblock data in the same subband/raster order
                // that prepare_precinct used for length encoding
                for s in 0..4usize {
                    if (*self.bands.add(s)).empty {
                        continue;
                    }
                    let band_width = (*self.bands.add(s)).num_blocks.w;
                    let width = self.cb_idxs[s].siz.w;
                    let height = self.cb_idxs[s].siz.h;
                    for y in 0..height {
                        let mut cp = (*self.bands.add(s)).coded_cbs.add(
                            (self.cb_idxs[s].org.x + (y + self.cb_idxs[s].org.y) * band_width)
                                as usize,
                        );
                        for _ in 0..width {
                            let mut lists = (*cp).next_coded;
                            while !lists.is_null() {
                                file.write(core::slice::from_raw_parts(
                                    (*lists).buf,
                                    ((*lists).buf_size - (*lists).avail_size) as usize,
                                ))?;
                                lists = (*lists).next_list;
                            }
                            cp = cp.add(1);
                        }
                    }
                }
            }
        } else {
            // Empty packet: single zero byte per ITU-T T.800
            file.write(&[0x00])?;
        }
        Ok(())
    }

    /// Parse a precinct's packet from the input bitstream.
    ///
    /// Reads the packet header (inclusion, missing-MSBs, pass counts,
    /// pass lengths) and stores codeblock data into the elastic allocator.
    /// Updates `data_left` with the remaining tile-part bytes.
    ///
    /// If `skipped` is true, this precinct's resolution level was skipped
    /// during decode (e.g. `-skip_res`) and we only consume header bytes.
    #[allow(clippy::too_many_arguments)]
    pub fn parse(
        &mut self,
        tag_tree_size: i32,
        lev_idx: *mut Ui32,
        elastic: *mut mem_elastic_allocator,
        data_left: &mut Ui32,
        file: &mut dyn InfileBase,
        skipped: bool,
        phdr: &mut Option<crate::codestream::ojph_packet_ml::PacketHeaderReader>,
    ) -> crate::openjph::ojph_message::OjphResult<()> {
        debug_assert!(*data_left > 0);
        let mut bb = bb_read::bb_init(*data_left, file);
        if self.may_use_sop {
            bb_read::bb_skip_sop(&mut bb)?;
        }

        // Packet HEADER source. Inline packet headers (the bit-exact default) read the header from
        // the tile stream `bb`; packed packet headers (PPM/PPT) read it from `phdr`'s buffer while
        // the packet BODY always stays in `bb`. The EPH terminator (T.800 B.9) is consumed from
        // whichever stream carries the header — `parse_header` calls `bb_terminate` on that stream.
        match phdr {
            None => self.parse_header(&mut bb, tag_tree_size, lev_idx, elastic, data_left)?,
            Some(p) => {
                use crate::openjph::ojph_file::MemInfile;
                // Re-open a MemInfile at the running byte cursor and restore the carried bit state,
                // mirroring `parse_ml_layer`: PPM/PPT headers form one continuous bit stream whose
                // bit accumulator / 0xFF-unstuff carry must survive packet boundaries.
                let avail = (p.buf.len() - p.pos) as Ui32;
                let base = p.buf.as_ptr();
                let mut hmem = MemInfile::default();
                unsafe { hmem.open_raw(base.add(p.pos), avail as usize) };
                let mut hbb = bb_read::BitReadBuf {
                    file: &mut hmem,
                    tmp: p.tmp,
                    avail_bits: p.avail_bits,
                    unstuff: p.unstuff,
                    bytes_left: avail,
                };
                self.parse_header(&mut hbb, tag_tree_size, lev_idx, elastic, data_left)?;
                p.pos += (avail - hbb.bytes_left) as usize;
                p.tmp = hbb.tmp;
                p.avail_bits = hbb.avail_bits;
                p.unstuff = hbb.unstuff;
            }
        }

        for s in 0..4usize {
            unsafe {
                if (*self.bands.add(s)).empty {
                    continue;
                }
                let band_width = (*self.bands.add(s)).num_blocks.w;
                let width = self.cb_idxs[s].siz.w;
                let height = self.cb_idxs[s].siz.h;
                for y in 0..height {
                    let mut cp = (*self.bands.add(s)).coded_cbs.add(
                        (self.cb_idxs[s].org.x + (y + self.cb_idxs[s].org.y) * band_width) as usize,
                    );
                    for _ in 0..width {
                        let num_bytes = (*cp).pass_length[0] + (*cp).pass_length[1];
                        if *data_left != 0 {
                            if num_bytes != 0 {
                                if skipped {
                                    let cur_loc = bb.file.tell();
                                    let t = ojph_min(num_bytes, bb.bytes_left);
                                    bb.file.seek(
                                        t as i64,
                                        crate::openjph::ojph_file::InfileSeek::OjphSeekCur,
                                    );
                                    let bytes_read = (bb.file.tell() - cur_loc) as Ui32;
                                    (*cp).pass_length[0] = 0;
                                    (*cp).pass_length[1] = 0;
                                    bb.bytes_left -= bytes_read;
                                    debug_assert!(bytes_read == t || bb.bytes_left == 0);
                                } else if !bb_read::bb_read_chunk(
                                    &mut bb,
                                    num_bytes,
                                    &mut (*cp).next_coded,
                                    elastic,
                                )? {
                                    (*cp).pass_length[0] = 0;
                                    (*cp).pass_length[1] = 0;
                                    *data_left = 0;
                                }
                            }
                        } else {
                            (*cp).pass_length[0] = 0;
                            (*cp).pass_length[1] = 0;
                        }
                        cp = cp.add(1);
                    }
                }
            }
        }
        *data_left = bb.bytes_left;
        Ok(())
    }

    /// Parse the single-layer packet **header** from the bit source `hbb` (empty-packet flag,
    /// inclusion + missing-MSBs tag trees, pass counts and codeword-segment lengths), writing
    /// `pass_length`/`num_passes`/`missing_msbs`/`seg_lengths` onto each subband's `CodedCbHeader`
    /// for the body loop in [`Self::parse`]. For inline packet headers `hbb` IS the tile stream
    /// (bit-exact default); for packed packet headers (PPM/PPT) it reads the packed buffer while the
    /// packet BODY stays in the tile stream. The EPH terminator (T.800 B.9) is consumed from `hbb`,
    /// i.e. from whichever stream carries the header.
    #[allow(clippy::too_many_arguments)]
    fn parse_header(
        &mut self,
        hbb: &mut bb_read::BitReadBuf<'_>,
        tag_tree_size: i32,
        lev_idx: *mut Ui32,
        elastic: *mut mem_elastic_allocator,
        data_left: &mut Ui32,
    ) -> crate::openjph::ojph_message::OjphResult<()> {
        let mut empty_packet = true;
        for s in 0..4usize {
            unsafe {
                if (*self.bands.add(s)).empty {
                    continue;
                }
                if self.cb_idxs[s].siz.w == 0 || self.cb_idxs[s].siz.h == 0 {
                    continue;
                }
                if empty_packet {
                    let mut bit = 0;
                    bb_read::bb_read_bit(hbb, &mut bit)?;
                    if bit == 0 {
                        bb_read::bb_terminate(hbb, self.uses_eph)?;
                        return Ok(());
                    }
                    empty_packet = false;
                }

                let num_levels = 1 + ojph_max(
                    log2ceil(self.cb_idxs[s].siz.w),
                    log2ceil(self.cb_idxs[s].siz.h),
                );
                let mut inc_tag = TagTree::default();
                let mut inc_tag_flags = TagTree::default();
                let mut mmsb_tag = TagTree::default();
                let mut mmsb_tag_flags = TagTree::default();
                inc_tag.init(self.scratch, lev_idx, num_levels, self.cb_idxs[s].siz, 0);
                *inc_tag.get(0, 0, num_levels) = 0;
                inc_tag_flags.init(
                    self.scratch.add(tag_tree_size as usize),
                    lev_idx,
                    num_levels,
                    self.cb_idxs[s].siz,
                    0,
                );
                *inc_tag_flags.get(0, 0, num_levels) = 0;
                mmsb_tag.init(
                    self.scratch.add((tag_tree_size << 1) as usize),
                    lev_idx,
                    num_levels,
                    self.cb_idxs[s].siz,
                    0,
                );
                *mmsb_tag.get(0, 0, num_levels) = 0;
                mmsb_tag_flags.init(
                    self.scratch
                        .add(((tag_tree_size << 1) + tag_tree_size) as usize),
                    lev_idx,
                    num_levels,
                    self.cb_idxs[s].siz,
                    0,
                );
                *mmsb_tag_flags.get(0, 0, num_levels) = 0;

                let band_width = (*self.bands.add(s)).num_blocks.w;
                let width = self.cb_idxs[s].siz.w;
                let height = self.cb_idxs[s].siz.h;
                for y in 0..height {
                    let mut cp = (*self.bands.add(s)).coded_cbs.add(
                        (self.cb_idxs[s].org.x + (y + self.cb_idxs[s].org.y) * band_width) as usize,
                    );
                    for x in 0..width {
                        let mut empty_cb = false;
                        for cl in (1..=num_levels).rev() {
                            let cur_lev = cl - 1;
                            empty_cb = *inc_tag.get(x >> cur_lev, y >> cur_lev, cur_lev) == 1;
                            if empty_cb {
                                break;
                            }
                            if *inc_tag_flags.get(x >> cur_lev, y >> cur_lev, cur_lev) == 0 {
                                let mut bit = 0;
                                if !bb_read::bb_read_bit(hbb, &mut bit)? {
                                    *data_left = 0;
                                    return Err(crate::ojph_error!(0x00030093, "error reading from file p1"));
                                }
                                empty_cb = bit == 0;
                                *inc_tag.get(x >> cur_lev, y >> cur_lev, cur_lev) =
                                    (1 - bit) as Ui8;
                                *inc_tag_flags.get(x >> cur_lev, y >> cur_lev, cur_lev) = 1;
                            }
                            if empty_cb {
                                break;
                            }
                        }
                        if empty_cb {
                            cp = cp.add(1);
                            continue;
                        }

                        let mut mmsbs = 0u32;
                        for levp1 in (1..=num_levels).rev() {
                            let cur_lev = levp1 - 1;
                            mmsbs = *mmsb_tag.get(x >> levp1, y >> levp1, levp1) as Ui32;
                            if *mmsb_tag_flags.get(x >> cur_lev, y >> cur_lev, cur_lev) == 0 {
                                let mut bit = 0;
                                while bit == 0 {
                                    if !bb_read::bb_read_bit(hbb, &mut bit)? {
                                        *data_left = 0;
                                        return Err(crate::ojph_error!(
                                            0x00030094,
                                            "error reading from file p2"
                                        ));
                                    }
                                    mmsbs += 1 - bit;
                                }
                                *mmsb_tag.get(x >> cur_lev, y >> cur_lev, cur_lev) = mmsbs as Ui8;
                                *mmsb_tag_flags.get(x >> cur_lev, y >> cur_lev, cur_lev) = 1;
                            }
                        }
                        if mmsbs > (*cp).kmax {
                            return Err(crate::ojph_error!(
                                0x00030095,
                                "error in parsing a tile header; missing msbs are larger or equal to kmax. The most likely cause is a corruption in the bitstream."
                            ));
                        }
                        (*cp).missing_msbs = mmsbs;

                        let mut bit = 0u32;
                        let mut num_passes = 1u32;
                        if !bb_read::bb_read_bit(hbb, &mut bit)? {
                            *data_left = 0;
                            return Err(crate::ojph_error!(0x00030096, "error reading from file p3"));
                        }
                        if bit != 0 {
                            num_passes = 2;
                            if !bb_read::bb_read_bit(hbb, &mut bit)? {
                                *data_left = 0;
                                return Err(crate::ojph_error!(0x00030097, "error reading from file p4"));
                            }
                            if bit != 0 {
                                if !bb_read::bb_read_bits(hbb, 2, &mut bit)? {
                                    *data_left = 0;
                                    return Err(crate::ojph_error!(0x00030098, "error reading from file p5"));
                                }
                                num_passes = 3 + bit;
                                if bit == 3 {
                                    if !bb_read::bb_read_bits(hbb, 5, &mut bit)? {
                                        *data_left = 0;
                                        return Err(crate::ojph_error!(
                                            0x00030099,
                                            "error reading from file p6"
                                        ));
                                    }
                                    num_passes = 6 + bit;
                                    if bit == 31 {
                                        if !bb_read::bb_read_bits(hbb, 7, &mut bit)? {
                                            *data_left = 0;
                                            return Err(crate::ojph_error!(
                                                0x0003009A,
                                                "error reading from file p7"
                                            ));
                                        }
                                        num_passes = 37 + bit;
                                    }
                                }
                            }
                        }
                        (*cp).num_passes = num_passes;

                        if self.is_ht {
                            let mut num_phld_passes = (num_passes - 1) / 3;
                            (*cp).missing_msbs += num_phld_passes;
                            num_phld_passes *= 3;
                            (*cp).num_passes = num_passes - num_phld_passes;
                            (*cp).pass_length[0] = 0;
                            (*cp).pass_length[1] = 0;

                            let mut lblock = 3i32;
                            bit = 1;
                            while bit != 0 {
                                if !bb_read::bb_read_bit(hbb, &mut bit)? {
                                    *data_left = 0;
                                    return Err(crate::ojph_error!(0x0003009B, "error reading from file p8"));
                                }
                                lblock += bit as i32;
                            }

                            let bits = lblock + 31 - (num_phld_passes + 1).leading_zeros() as i32;
                            if !bb_read::bb_read_bits(hbb, bits, &mut bit)? {
                                *data_left = 0;
                                return Err(crate::ojph_error!(0x0003009C, "error reading from file p9"));
                            }
                            if bit < 2 {
                                return Err(crate::ojph_error!(
                                    0x0003009D,
                                    "The cleanup segment of an HT codeblock cannot contain less than 2 bytes"
                                ));
                            }
                            if bit >= 65535 {
                                return Err(crate::ojph_error!(
                                    0x0003009E,
                                    "The cleanup segment of an HT codeblock must contain less than 65535 bytes"
                                ));
                            }
                            (*cp).pass_length[0] = bit;

                            if (*cp).num_passes > 1 {
                                let bits = lblock + if (*cp).num_passes > 2 { 1 } else { 0 };
                                if !bb_read::bb_read_bits(hbb, bits, &mut bit)? {
                                    *data_left = 0;
                                    return Err(crate::ojph_error!(0x0003009F, "error reading from file p10"));
                                }
                                if bit >= 2047 {
                                    return Err(crate::ojph_error!(
                                        0x000300A0,
                                        "The refinement segment (SigProp and MagRep passes) of an HT codeblock must contain less than 2047 bytes"
                                    ));
                                }
                                (*cp).pass_length[1] = bit;
                            }
                        } else {
                            // Part-1 (legacy MQ/EBCOT): no placeholder-pass folding (missing_msbs
                            // comes solely from the tag tree). The default block forms a SINGLE
                            // codeword segment over all passes (T.800 B.10.7.1) — one length of
                            // `Lblock + floor(log2(num_passes))` bits. RESTART/BYPASS split the block
                            // into several segments ([`seg_end`]): one length per segment, each
                            // `Lblock + floor(log2(passes in segment))` bits, kept per-pass for the
                            // decoder.
                            (*cp).pass_length[0] = 0;
                            (*cp).pass_length[1] = 0;

                            let mut lblock = 3i32;
                            bit = 1;
                            while bit != 0 {
                                if !bb_read::bb_read_bit(hbb, &mut bit)? {
                                    *data_left = 0;
                                    return Err(crate::ojph_error!(0x000300C7, "error reading Lblock (Part-1)"));
                                }
                                lblock += bit as i32;
                            }
                            if self.cmodes & SEG_MODES != 0 {
                                // Total is summed into `pass_length[0]` so the body chunk read
                                // stays a single contiguous copy, while the per-pass lengths drive
                                // `mq.init` in the decoder.
                                let mut seg_cl: *mut CodedLists = ptr::null_mut();
                                (*elastic)
                                    .get_buffer(num_passes.saturating_mul(4), &mut seg_cl);
                                let seg_ptr = (*seg_cl).buf as *mut Ui32;
                                core::ptr::write_bytes(seg_ptr, 0, num_passes as usize);
                                let mut total = 0u32;
                                let mut z = 0u32;
                                while z < num_passes {
                                    let take = (seg_end(self.cmodes, z) - z).min(num_passes - z);
                                    let bits = lblock + 31 - take.leading_zeros() as i32;
                                    if !bb_read::bb_read_bits(hbb, bits, &mut bit)? {
                                        *data_left = 0;
                                        return Err(crate::ojph_error!(
                                            0x000300C8,
                                            "error reading codeword segment length (Part-1)"
                                        ));
                                    }
                                    z += take;
                                    *seg_ptr.add(z as usize - 1) = bit;
                                    total = total.saturating_add(bit);
                                }
                                (*cp).seg_lengths = seg_ptr;
                                (*cp).seg_count = num_passes as u16;
                                (*cp).pass_length[0] = total;
                            } else {
                                let bits = lblock + 31 - num_passes.leading_zeros() as i32;
                                if !bb_read::bb_read_bits(hbb, bits, &mut bit)? {
                                    *data_left = 0;
                                    return Err(crate::ojph_error!(
                                        0x000300C8,
                                        "error reading codeword segment length (Part-1)"
                                    ));
                                }
                                (*cp).pass_length[0] = bit;
                            }
                        }
                        cp = cp.add(1);
                    }
                }
            }
        }

        if empty_packet {
            let mut bit = 0;
            bb_read::bb_read_bit(hbb, &mut bit)?;
        }
        bb_read::bb_terminate(hbb, self.uses_eph)?;
        Ok(())
    }

    /// Parse one quality layer's packet **header** from `hbb` into `mlp` (inclusion / ZBP / HT
    /// segment lengths per band). Returns `true` when the packet is non-empty (bodies follow). The
    /// header source `hbb` is the tile stream for inline headers, or a PPM/PPT buffer when packed
    /// packet headers are in use (see [`Self::parse_ml_layer`]).
    fn parse_ml_header(
        &mut self,
        mlp: &mut crate::codestream::ojph_packet_ml::MlPrecinct,
        layer_idx: u16,
        ccap15: u16,
        hbb: &mut bb_read::BitReadBuf<'_>,
    ) -> crate::openjph::ojph_message::OjphResult<bool> {
        use crate::codestream::ojph_packet_ml as ml;
        let mut empty_packet = true;
        for s in 0..4usize {
            unsafe {
                if (*self.bands.add(s)).empty {
                    continue;
                }
                if self.cb_idxs[s].siz.w == 0 || self.cb_idxs[s].siz.h == 0 {
                    continue;
                }
                if empty_packet {
                    let mut bit = 0;
                    bb_read::bb_read_bit(hbb, &mut bit)?;
                    if bit == 0 {
                        bb_read::bb_terminate(hbb, self.uses_eph)?;
                        return Ok(false);
                    }
                    empty_packet = false;
                }
                let gw = self.cb_idxs[s].siz.w as usize;
                let gh = self.cb_idxs[s].siz.h as usize;
                if mlp.bands[s].is_none() {
                    mlp.bands[s] = Some(ml::MlBand::new(gw, gh));
                }
                let band = mlp.bands[s].as_mut().unwrap();
                let band_width = (*self.bands.add(s)).num_blocks.w;
                ml::parse_subband_ml(
                    hbb,
                    (*self.bands.add(s)).coded_cbs,
                    band_width,
                    self.cb_idxs[s].org.x,
                    self.cb_idxs[s].org.y,
                    gw,
                    gh,
                    band,
                    layer_idx,
                    ccap15,
                    self.is_ht,
                    self.cmodes & SEG_MODES,
                )?;
            }
        }
        if empty_packet {
            let mut bit = 0;
            bb_read::bb_read_bit(hbb, &mut bit)?;
        }
        bb_read::bb_terminate(hbb, self.uses_eph)?;
        Ok(!empty_packet)
    }

    /// Parse ONE quality layer's packet for this precinct, accumulating into the persistent `mlp`
    /// (the multi-layer state that spans all layers of this precinct, T.800 B.10). Mirrors
    /// [`Self::parse`] structurally — SOP skip, empty-packet bit, four subbands, EPH terminate,
    /// then the body — but routes the header through [`ml::parse_subband_ml`] (which accumulates
    /// per-block `pass_length`/`num_passes` on the `CodedCbHeader` and the per-layer body length on
    /// the `MlBand`) and appends this layer's body bytes to each block's accumulator instead of
    /// reading one contiguous chunk. Call once per layer in stream order; after the last layer call
    /// [`Self::finalize_ml`] to fold placeholder passes and materialise `next_coded`.
    ///
    /// Per-block header state (`already_included`, `lblock`, totals) persists on the arena
    /// `CodedCbHeader` array; the tag-tree + body accumulators persist on `mlp`. Both rely on a
    /// fresh (zeroed) start at layer 0, which holds for a fresh decode.
    ///
    /// `skipped` marks a resolution dropped for reading: its headers are still parsed (they fix the
    /// offset of the next packet) but the bodies are stepped over and nothing is finalized.
    #[allow(clippy::too_many_arguments)]
    pub fn parse_ml_layer(
        &mut self,
        mlp: &mut crate::codestream::ojph_packet_ml::MlPrecinct,
        layer_idx: u16,
        ccap15: u16,
        data_left: &mut Ui32,
        file: &mut dyn InfileBase,
        skipped: bool,
        phdr: &mut Option<crate::codestream::ojph_packet_ml::PacketHeaderReader>,
    ) -> crate::openjph::ojph_message::OjphResult<()> {
        use crate::codestream::ojph_packet_ml as ml;
        use crate::openjph::ojph_file::MemInfile;
        debug_assert!(*data_left > 0);
        // Body source: the tile stream. SOP (if used) precedes each packet body here even when the
        // headers are packed away in PPM/PPT.
        let mut bb = bb_read::bb_init(*data_left, file);
        if self.may_use_sop {
            bb_read::bb_skip_sop(&mut bb)?;
        }

        // Header source: a separate bit-reader over the PPM/PPT buffer when present, otherwise the
        // body bit-reader itself (inline headers — the bit-exact default path).
        let non_empty = match phdr {
            None => self.parse_ml_header(mlp, layer_idx, ccap15, &mut bb)?,
            Some(p) => {
                // ONE persistent bit-reader over the tile-part's header buffer: the bit accumulator
                // (tmp/avail_bits) and 0xFF-unstuff carry survive across packets, since without an
                // EPH marker the packet headers are a continuous bit stream (mirrors OpenHTJ2K's
                // buf_chain). The MemInfile is re-opened at the byte cursor each packet, but the
                // carried bit state is restored rather than reset.
                let avail = (p.buf.len() - p.pos) as Ui32;
                let base = p.buf.as_ptr();
                let mut hmem = MemInfile::default();
                unsafe { hmem.open_raw(base.add(p.pos), avail as usize) };
                let mut hbb = bb_read::BitReadBuf {
                    file: &mut hmem,
                    tmp: p.tmp,
                    avail_bits: p.avail_bits,
                    unstuff: p.unstuff,
                    bytes_left: avail,
                };
                let ne = self.parse_ml_header(mlp, layer_idx, ccap15, &mut hbb)?;
                p.pos += (avail - hbb.bytes_left) as usize;
                p.tmp = hbb.tmp;
                p.avail_bits = hbb.avail_bits;
                p.unstuff = hbb.unstuff;
                ne
            }
        };

        if non_empty {
            for s in 0..4usize {
                unsafe {
                    if (*self.bands.add(s)).empty {
                        continue;
                    }
                    if self.cb_idxs[s].siz.w == 0 || self.cb_idxs[s].siz.h == 0 {
                        continue;
                    }
                    let gw = self.cb_idxs[s].siz.w as usize;
                    let gh = self.cb_idxs[s].siz.h as usize;
                    if let Some(band) = mlp.bands[s].as_mut() {
                        ml::read_subband_body_ml(&mut *bb.file, &mut bb.bytes_left, band, gw, gh, skipped)?;
                    }
                }
            }
        }
        *data_left = bb.bytes_left;
        Ok(())
    }

    /// After the final quality layer, fold placeholder passes and materialise each block's
    /// accumulated bytes into its single contiguous `next_coded` node (see [`ml::finalize_band`]).
    pub fn finalize_ml(
        &mut self,
        mlp: &mut crate::codestream::ojph_packet_ml::MlPrecinct,
        elastic: *mut mem_elastic_allocator,
    ) -> crate::openjph::ojph_message::OjphResult<()> {
        use crate::codestream::ojph_packet_ml as ml;
        for s in 0..4usize {
            unsafe {
                if (*self.bands.add(s)).empty {
                    continue;
                }
                if self.cb_idxs[s].siz.w == 0 || self.cb_idxs[s].siz.h == 0 {
                    continue;
                }
                let gw = self.cb_idxs[s].siz.w as usize;
                let gh = self.cb_idxs[s].siz.h as usize;
                let band_width = (*self.bands.add(s)).num_blocks.w;
                if let Some(band) = mlp.bands[s].as_mut() {
                    ml::finalize_band(
                        (*self.bands.add(s)).coded_cbs,
                        band_width,
                        self.cb_idxs[s].org.x,
                        self.cb_idxs[s].org.y,
                        gw,
                        gh,
                        band,
                        elastic,
                    )?;
                }
            }
        }
        Ok(())
    }
}
