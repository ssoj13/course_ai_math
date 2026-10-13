//! Port target:
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/codestream/ojph_codestream_local.h`
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/codestream/ojph_codestream_local.cpp`

use crate::codestream::ojph_params_local::{
    Jp2kMarker, OJPH_PN_STRING_BROADCAST, OJPH_PN_STRING_IMF, OJPH_TILEPART_COMPONENTS,
    OJPH_TILEPART_NO_DIVISIONS, OJPH_TILEPART_RESOLUTIONS, OjphProfileNum, ParamAtk, ParamCap,
    ParamCod, ParamDfs, ParamNlt, ParamPoc, ParamQcd, ParamSiz, ParamSot, ParamTlm,
    ProgressionOrder, TtlmPtlmPair, read_rgn,
};
use crate::codestream::ojph_tile::Tile;
use crate::openjph::ojph_arch::swap_byte_u16;
use crate::openjph::ojph_base::{Rect, Size};
use crate::openjph::ojph_defs::{Ui8, Ui16, Ui32, ojph_div_ceil, ojph_max, ojph_min};
use crate::openjph::ojph_file::{InfileBase, InfileSeek, OutfileBase};
use crate::openjph::ojph_mem::{LineBuf, mem_elastic_allocator, mem_fixed_allocator};
use crate::openjph::ojph_message::{OjphMsgLevel, OjphResult};
use crate::openjph::ojph_params::CommentExchange;
use crate::openjph::ojph_version::{
    OPENJPH_VERSION_MAJOR, OPENJPH_VERSION_MINOR, OPENJPH_VERSION_PATCH,
};
use crate::transform::ojph_colour::init_colour_transform_functions;
use crate::transform::ojph_transform::init_wavelet_transform_functions;
use core::{mem, ptr};
use std::slice;

#[allow(non_camel_case_types)]
pub struct LocalCodestream {
    precinct_scratch_needed_bytes: Ui32,
    precinct_scratch: *mut Ui8,
    cur_line: Ui32,
    cur_comp: Ui32,
    cur_tile_row: Ui32,
    resilient: bool,
    /// Truncating (T.800 dead-zone) forward quantization on every SIMD tier; see
    /// [`crate::codestream::ojph_codeblock_fun::CodeblockFun::init`].
    trunc_quant: bool,
    skipped_res_for_read: Ui32,
    skipped_res_for_recon: Ui32,
    num_tiles: Size,
    tiles: *mut Tile,
    lines: *mut LineBuf,
    num_comps: Ui32,
    comp_size: *mut Size,
    recon_comp_size: *mut Size,
    employ_color_transform: bool,
    planar: i32,
    profile: i32,
    tilepart_div: Ui32,
    need_tlm: bool,
    siz: ParamSiz,
    cod: ParamCod,
    cap: ParamCap,
    qcd: ParamQcd,
    tlm: ParamTlm,
    nlt: ParamNlt,
    dfs: ParamDfs,
    atk: ParamAtk,
    /// Main-header POC marker (`0xFF5F`), if present; applies to every tile (an in-tile POC
    /// overrides it for that tile). Empty when no POC marker was read.
    poc: ParamPoc,
    /// Per-component ROI max-shift from main-header RGN markers (`0xFF5E`), 0 = no shift. Sized to
    /// the component count once SIZ is read. Copied into every tile at allocation; an in-tile RGN
    /// overrides the tile-local copy (see [`crate::codestream::ojph_tile::Tile`]).
    roishift: Vec<u8>,
    /// Packed packet headers from main-header PPM markers (`0xFF60`): `(Zppm, Ippm)` segments,
    /// assembled (sorted by Zppm) into one stream of `[Nppm (u32 BE)][headers]` blocks, one block
    /// per tile-part in codestream order. `ppm_pos` is the global byte cursor consumed as
    /// successive tile-parts pull their header block. Empty when no PPM marker was read.
    ppm_segments: Vec<(u8, Vec<u8>)>,
    ppm_buf: Vec<u8>,
    ppm_pos: usize,
    allocator: Box<mem_fixed_allocator>,
    elastic_alloc: Box<mem_elastic_allocator>,
    outfile: Option<*mut dyn OutfileBase>,
    infile: Option<*mut dyn InfileBase>,
    /// Buffered packet-body bytes for multi-layer (num_layers>1) tiles whose packets span several
    /// non-empty tile-parts. Key = tile index; value = (concatenated payload bytes, tile-parts seen
    /// so far, the tile's POC captured from its first part). `read()` appends each tile-part payload
    /// as it arrives; once all declared parts are in (or at EOC, for an unknown TNsot count) the
    /// concatenation is swept once via a `MemInfile`. Lives here (this struct is Drop-managed)
    /// rather than on the arena-allocated `Tile`, which never drops.
    ml_tp_buf: std::collections::HashMap<u32, TpBuf>,
}

/// Buffered state of one tile whose packets span several tile-parts: payload bytes, packed packet
/// header bytes (PPM/PPT, `None` for inline headers), tile-parts seen so far, and the tile's POC
/// captured from its first part.
struct TpBuf {
    body: Vec<u8>,
    headers: Option<Vec<u8>>,
    parts: u8,
    poc: ParamPoc,
}

impl Default for LocalCodestream {
    fn default() -> Self {
        init_colour_transform_functions();
        init_wavelet_transform_functions();

        let mut value = Self {
            precinct_scratch_needed_bytes: 0,
            precinct_scratch: ptr::null_mut(),
            cur_line: 0,
            cur_comp: 0,
            cur_tile_row: 0,
            resilient: false,
            trunc_quant: false,
            skipped_res_for_read: 0,
            skipped_res_for_recon: 0,
            num_tiles: Size::default(),
            tiles: ptr::null_mut(),
            lines: ptr::null_mut(),
            num_comps: 0,
            comp_size: ptr::null_mut(),
            recon_comp_size: ptr::null_mut(),
            employ_color_transform: false,
            planar: -1,
            profile: OjphProfileNum::OjphPnUndefined as i32,
            tilepart_div: OJPH_TILEPART_NO_DIVISIONS,
            need_tlm: false,
            siz: ParamSiz::default(),
            cod: ParamCod::default(),
            cap: ParamCap::default(),
            qcd: ParamQcd::default(),
            tlm: ParamTlm::default(),
            nlt: ParamNlt::default(),
            dfs: ParamDfs::default(),
            atk: ParamAtk::default(),
            poc: ParamPoc::default(),
            roishift: Vec::new(),
            ppm_segments: Vec::new(),
            ppm_buf: Vec::new(),
            ppm_pos: 0,
            allocator: Box::new(mem_fixed_allocator::default()),
            elastic_alloc: Box::new(mem_elastic_allocator::new(1_048_576)),
            outfile: None,
            infile: None,
            ml_tp_buf: std::collections::HashMap::new(),
        };
        value.restart();
        value
    }
}

impl LocalCodestream {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create directly in a Box to avoid self-referential pointer invalidation: the parameter
    /// structs wired to each other by raw pointers (`cod`, `dfs`, ...) break if the codestream is
    /// moved after `restart()`. (`ParamSiz` no longer holds a pointer into itself; it owns a `Vec`.)
    /// This method ensures restart() runs at the final heap location.
    pub fn new_boxed() -> Box<Self> {
        init_colour_transform_functions();
        init_wavelet_transform_functions();

        let mut boxed = Box::new(Self {
            precinct_scratch_needed_bytes: 0,
            precinct_scratch: ptr::null_mut(),
            cur_line: 0,
            cur_comp: 0,
            cur_tile_row: 0,
            resilient: false,
            trunc_quant: false,
            skipped_res_for_read: 0,
            skipped_res_for_recon: 0,
            num_tiles: Size::default(),
            tiles: ptr::null_mut(),
            lines: ptr::null_mut(),
            num_comps: 0,
            comp_size: ptr::null_mut(),
            recon_comp_size: ptr::null_mut(),
            employ_color_transform: false,
            planar: -1,
            profile: OjphProfileNum::OjphPnUndefined as i32,
            tilepart_div: OJPH_TILEPART_NO_DIVISIONS,
            need_tlm: false,
            siz: ParamSiz::new_uninit(),
            cod: ParamCod::new_uninit(),
            cap: ParamCap::default(),
            qcd: ParamQcd::default(),
            tlm: ParamTlm::default(),
            nlt: ParamNlt::default(),
            dfs: ParamDfs::default(),
            atk: ParamAtk::default(),
            poc: ParamPoc::default(),
            roishift: Vec::new(),
            ppm_segments: Vec::new(),
            ppm_buf: Vec::new(),
            ppm_pos: 0,
            allocator: Box::new(mem_fixed_allocator::default()),
            elastic_alloc: Box::new(mem_elastic_allocator::new(1_048_576)),
            outfile: None,
            infile: None,
            ml_tp_buf: std::collections::HashMap::new(),
        });
        boxed.restart();
        boxed
    }

    pub fn restart(&mut self) {
        self.tiles = ptr::null_mut();
        self.lines = ptr::null_mut();
        self.comp_size = ptr::null_mut();
        self.recon_comp_size = ptr::null_mut();
        self.outfile = None;
        self.infile = None;

        self.num_tiles = Size::default();
        self.num_comps = 0;
        self.roishift.clear();
        self.ml_tp_buf.clear();
        self.ppm_segments.clear();
        self.ppm_buf.clear();
        self.ppm_pos = 0;
        self.employ_color_transform = false;
        self.planar = -1;
        self.profile = OjphProfileNum::OjphPnUndefined as i32;
        self.tilepart_div = OJPH_TILEPART_NO_DIVISIONS;
        self.need_tlm = false;

        self.cur_comp = 0;
        self.cur_line = 0;
        self.cur_tile_row = 0;
        self.resilient = false;
        self.trunc_quant = false;
        self.skipped_res_for_read = 0;
        self.skipped_res_for_recon = 0;

        self.precinct_scratch_needed_bytes = 0;
        self.precinct_scratch = ptr::null_mut();

        self.siz.init();
        self.cod.restart();
        self.qcd.restart();
        self.nlt.restart();
        self.dfs.restart();
        self.atk.restart();
        self.poc.restart();

        self.allocator.restart();
        self.elastic_alloc.restart();
    }

    #[inline]
    pub fn access_siz(&mut self) -> &mut ParamSiz {
        &mut self.siz
    }

    #[inline]
    pub const fn get_siz(&self) -> &ParamSiz {
        &self.siz
    }

    #[inline]
    pub fn access_cod(&mut self) -> &mut ParamCod {
        &mut self.cod
    }

    /// `Ccap^15` (CAP marker) — the HTJ2K capabilities word, looked up by part number (it is not
    /// always word 0: Part-2 streams carry `Ccap^2` first). Bits 14–15 distinguish HT-only (pure
    /// HT, 0) from HT-mixed; the multi-layer packet parser needs it to set the `HT_MIXED`
    /// code-block flag (ITU-T T.814 / T.800 B.10).
    #[inline]
    pub fn get_ccap15(&self) -> Ui16 {
        self.cap.get_ccap_part(15).unwrap_or(0)
    }

    /// ROI max-shift for component `comp_num` from main-header RGN markers (0 = none). Defensive:
    /// returns 0 for any component beyond the parsed table.
    #[inline]
    pub fn get_roishift(&self, comp_num: Ui32) -> u8 {
        self.roishift.get(comp_num as usize).copied().unwrap_or(0)
    }

    /// Record a main-header RGN max-shift for `comp_num`, growing the per-component table as needed
    /// (it is sized from SIZ, but stay robust to marker ordering).
    pub fn set_roishift(&mut self, comp_num: Ui32, shift: u8) {
        if self.roishift.len() <= comp_num as usize {
            self.roishift.resize(comp_num as usize + 1, 0);
        }
        self.roishift[comp_num as usize] = shift;
    }

    /// Read one main-header PPM marker (`0xFF60`) body (cursor just past `FF60`): `Lppm`, `Zppm`,
    /// then the `Ippm` payload, stashed by `Zppm` for later in-order assembly.
    fn read_ppm(&mut self, file: &mut dyn InfileBase) -> OjphResult<()> {
        let mut lppm = [0u8; 2];
        if file.read(&mut lppm) != 2 {
            return Err(crate::ojph_error!(0x00030081, "error reading PPM marker"));
        }
        let lppm = u16::from_be_bytes(lppm) as usize;
        if lppm < 3 {
            return Err(crate::ojph_error!(
                0x00030082,
                "wrong Lppm value {} in PPM marker",
                lppm
            ));
        }
        let mut zppm = [0u8; 1];
        if file.read(&mut zppm) != 1 {
            return Err(crate::ojph_error!(0x00030081, "error reading PPM marker"));
        }
        let mut data = vec![0u8; lppm - 3];
        if file.read(&mut data) != data.len() {
            return Err(crate::ojph_error!(
                0x00030081,
                "error reading PPM marker payload"
            ));
        }
        self.ppm_segments.push((zppm[0], data));
        Ok(())
    }

    /// Whether any main-header PPM marker was parsed.
    pub fn has_ppm(&self) -> bool {
        !self.ppm_segments.is_empty()
    }

    /// Read one in-tile PPT marker (`0xFF61`) body (cursor just past `FF61`): `Lppt`, `Zppt`, then
    /// the `Ippt` payload (packet headers for this tile). Returns `(Zppt, Ippt)`.
    fn read_ppt(&self, file: &mut dyn InfileBase) -> OjphResult<(u8, Vec<u8>)> {
        let mut lppt = [0u8; 2];
        if file.read(&mut lppt) != 2 {
            return Err(crate::ojph_error!(0x00030084, "error reading PPT marker"));
        }
        let lppt = u16::from_be_bytes(lppt) as usize;
        if lppt < 3 {
            return Err(crate::ojph_error!(
                0x00030085,
                "wrong Lppt value {} in PPT marker",
                lppt
            ));
        }
        let mut zppt = [0u8; 1];
        if file.read(&mut zppt) != 1 {
            return Err(crate::ojph_error!(0x00030084, "error reading PPT marker"));
        }
        let mut data = vec![0u8; lppt - 3];
        if file.read(&mut data) != data.len() {
            return Err(crate::ojph_error!(
                0x00030084,
                "error reading PPT marker payload"
            ));
        }
        Ok((zppt[0], data))
    }

    /// Pull the next tile-part's packet-header block from the assembled PPM stream: read the `Nppm`
    /// (u32 BE) length prefix at the global cursor and return the following `Nppm` header bytes,
    /// advancing the cursor. Returns an empty vec when the stream is exhausted (a tile-part with no
    /// packets). The segments are assembled (sorted by `Zppm`) on first use.
    pub fn take_ppm_tilepart_headers(&mut self) -> OjphResult<Vec<u8>> {
        if self.ppm_buf.is_empty() && !self.ppm_segments.is_empty() {
            self.ppm_segments.sort_by_key(|(z, _)| *z);
            for (_, d) in &self.ppm_segments {
                self.ppm_buf.extend_from_slice(d);
            }
        }
        if self.ppm_pos + 4 > self.ppm_buf.len() {
            return Ok(Vec::new());
        }
        let nppm = u32::from_be_bytes([
            self.ppm_buf[self.ppm_pos],
            self.ppm_buf[self.ppm_pos + 1],
            self.ppm_buf[self.ppm_pos + 2],
            self.ppm_buf[self.ppm_pos + 3],
        ]) as usize;
        self.ppm_pos += 4;
        // `nppm` is attacker-controlled (up to 4 GiB); compare without adding to `ppm_pos` so the
        // bounds check cannot wrap `usize` on a 32-bit target.
        if nppm > self.ppm_buf.len() - self.ppm_pos {
            return Err(crate::ojph_error!(
                0x00030083,
                "PPM stream claims a {}-byte tile-part header block but only {} bytes remain",
                nppm,
                self.ppm_buf.len() - self.ppm_pos
            ));
        }
        let block = self.ppm_buf[self.ppm_pos..self.ppm_pos + nppm].to_vec();
        self.ppm_pos += nppm;
        Ok(block)
    }

    #[inline]
    pub const fn get_cod(&self) -> &ParamCod {
        &self.cod
    }

    #[inline]
    pub fn get_coc(&self, comp_num: Ui32) -> *mut ParamCod {
        self.cod.get_coc(comp_num)
    }

    #[inline]
    pub const fn access_qcd(&self) -> &ParamQcd {
        &self.qcd
    }

    pub fn access_qcd_mut(&mut self) -> &mut ParamQcd {
        &mut self.qcd
    }

    #[inline]
    pub fn access_dfs(&self) -> Option<&ParamDfs> {
        if self.dfs.exists() {
            Some(&self.dfs)
        } else {
            None
        }
    }

    #[inline]
    pub const fn access_dfs_ptr(&self) -> *const ParamDfs {
        if self.dfs.exists() {
            &self.dfs
        } else {
            ptr::null()
        }
    }

    #[inline]
    pub const fn get_nlt(&self) -> &ParamNlt {
        &self.nlt
    }

    #[inline]
    pub fn get_nlt_mut(&mut self) -> &mut ParamNlt {
        &mut self.nlt
    }

    #[inline]
    pub fn get_allocator(&mut self) -> *mut mem_fixed_allocator {
        &mut *self.allocator
    }

    #[inline]
    pub fn get_elastic_alloc(&mut self) -> *mut mem_elastic_allocator {
        &mut *self.elastic_alloc
    }

    #[inline]
    pub fn get_file(&mut self) -> OjphResult<*mut dyn OutfileBase> {
        let outfile = self.outfile;
        debug_assert!(outfile.is_some(), "outfile is null");
        match outfile {
            Some(outfile) => Ok(outfile),
            None => Err(crate::ojph_error!(0x000300A2, "outfile is null")),
        }
    }

    #[inline]
    pub fn get_precinct_scratch(&mut self) -> *mut Ui8 {
        self.precinct_scratch
    }

    #[inline]
    pub const fn get_skipped_res_for_read(&self) -> Ui32 {
        self.skipped_res_for_read
    }

    #[inline]
    pub const fn get_skipped_res_for_recon(&self) -> Ui32 {
        self.skipped_res_for_recon
    }

    fn pre_alloc(&mut self) -> OjphResult<()> {
        self.num_tiles.w = self.siz.get_image_extent().x - self.siz.get_tile_offset().x;
        self.num_tiles.w = ojph_div_ceil(self.num_tiles.w, self.siz.get_tile_size().w);
        self.num_tiles.h = self.siz.get_image_extent().y - self.siz.get_tile_offset().y;
        self.num_tiles.h = ojph_div_ceil(self.num_tiles.h, self.siz.get_tile_size().h);
        if self.num_tiles.area() > 65_535 {
            return Err(crate::ojph_error!(0x00030011, "the number of tiles cannot exceed 65535"));
        }
        if self.num_tiles.area() == 0 {
            return Err(crate::ojph_error!(0x00030012, "the number of tiles cannot be 0"));
        }

        self.allocator
            .pre_alloc_obj::<Tile>(self.num_tiles.area() as usize);

        let mut tile_rect = Rect::default();
        let mut recon_tile_rect = Rect::default();
        let ds = 1u32 << self.skipped_res_for_recon;
        for y in 0..self.num_tiles.h {
            let y0 = self.siz.get_tile_offset().y + y * self.siz.get_tile_size().h;
            let y1 = y0 + self.siz.get_tile_size().h;
            tile_rect.org.y = ojph_max(y0, self.siz.get_image_offset().y);
            tile_rect.siz.h = ojph_min(y1, self.siz.get_image_extent().y) - tile_rect.org.y;
            recon_tile_rect.org.y = ojph_max(
                ojph_div_ceil(y0, ds),
                ojph_div_ceil(self.siz.get_image_offset().y, ds),
            );
            recon_tile_rect.siz.h = ojph_min(
                ojph_div_ceil(y1, ds),
                ojph_div_ceil(self.siz.get_image_extent().y, ds),
            ) - recon_tile_rect.org.y;

            for x in 0..self.num_tiles.w {
                let x0 = self.siz.get_tile_offset().x + x * self.siz.get_tile_size().w;
                let x1 = x0 + self.siz.get_tile_size().w;
                tile_rect.org.x = ojph_max(x0, self.siz.get_image_offset().x);
                tile_rect.siz.w = ojph_min(x1, self.siz.get_image_extent().x) - tile_rect.org.x;
                recon_tile_rect.org.x = ojph_max(
                    ojph_div_ceil(x0, ds),
                    ojph_div_ceil(self.siz.get_image_offset().x, ds),
                );
                recon_tile_rect.siz.w = ojph_min(
                    ojph_div_ceil(x1, ds),
                    ojph_div_ceil(self.siz.get_image_extent().x, ds),
                ) - recon_tile_rect.org.x;

                let mut tps = 0;
                Tile::pre_alloc(self, tile_rect, recon_tile_rect, &mut tps)?;
            }
        }

        let num_comps = self.siz.get_num_components() as usize;
        self.allocator.pre_alloc_obj::<LineBuf>(num_comps);
        self.allocator.pre_alloc_obj::<Size>(num_comps);
        self.allocator.pre_alloc_obj::<Size>(num_comps);
        for i in 0..num_comps {
            self.allocator
                .pre_alloc_data::<i32>(self.siz.get_recon_width(i as Ui32) as usize, 0);
        }

        // OpenJPH v0.27+ fix: the precinct scratch buffer is shared by all components, but
        // each component may override its codeblock/precinct geometry via a COC marker. The
        // per-component tag-tree storage (ojph_resolution.rs) is derived from that component's
        // effective params, so size the shared buffer from the largest ratio across every
        // component (the main COD and all COC overrides). Sizing from the COD alone (as
        // OpenJPH v0.26.3 did) under-reserves the buffer for any component whose COC declares
        // a smaller codeblock than the COD.
        let mut ratio = Size::default();
        for c in 0..num_comps as Ui32 {
            let cdp = unsafe { &*self.get_coc(c) };
            let num_decomps = cdp.get_num_decompositions() as Ui32;
            let log_cb = cdp.get_log_block_dims();
            for r in 0..=num_decomps {
                let log_pp = cdp.get_log_precinct_size(r);
                ratio.w = ojph_max(ratio.w, log_pp.w - ojph_min(log_cb.w, log_pp.w));
                ratio.h = ojph_max(ratio.h, log_pp.h - ojph_min(log_cb.h, log_pp.h));
            }
        }
        let mut max_ratio = ojph_max(ratio.w, ratio.h);
        max_ratio = 1 << max_ratio;
        self.precinct_scratch_needed_bytes = 4 * ((max_ratio * max_ratio * 4 + 2) / 3);
        self.allocator
            .pre_alloc_obj::<Ui8>(self.precinct_scratch_needed_bytes as usize);
        Ok(())
    }

    fn finalize_alloc(&mut self) -> OjphResult<()> {
        self.allocator.alloc();

        self.precinct_scratch = self
            .allocator
            .post_alloc_obj::<Ui8>(self.precinct_scratch_needed_bytes as usize);

        self.tiles = self
            .allocator
            .post_alloc_obj::<Tile>(self.num_tiles.area() as usize);
        let mut tile_rect = Rect::default();
        let mut num_tileparts: Ui32 = 0; // across all tiles; sizes the TLM marker
        for y in 0..self.num_tiles.h {
            let y0 = self.siz.get_tile_offset().y + y * self.siz.get_tile_size().h;
            let y1 = y0 + self.siz.get_tile_size().h;
            tile_rect.org.y = ojph_max(y0, self.siz.get_image_offset().y);
            tile_rect.siz.h = ojph_min(y1, self.siz.get_image_extent().y) - tile_rect.org.y;
            let mut offset = 0;
            for x in 0..self.num_tiles.w {
                let x0 = self.siz.get_tile_offset().x + x * self.siz.get_tile_size().w;
                let x1 = x0 + self.siz.get_tile_size().w;
                tile_rect.org.x = ojph_max(x0, self.siz.get_image_offset().x);
                tile_rect.siz.w = ojph_min(x1, self.siz.get_image_extent().x) - tile_rect.org.x;
                let idx = (y * self.num_tiles.w + x) as usize;
                let mut tps = 0;
                unsafe {
                    ptr::write(self.tiles.add(idx), Tile::default());
                    (*self.tiles.add(idx)).finalize_alloc(
                        self,
                        tile_rect,
                        y * self.num_tiles.w + x,
                        &mut offset,
                        &mut tps,
                    )?;
                }
                num_tileparts += tps;
            }
        }

        self.num_comps = self.siz.get_num_components() as Ui32;
        self.lines = self
            .allocator
            .post_alloc_obj::<LineBuf>(self.num_comps as usize);
        self.comp_size = self
            .allocator
            .post_alloc_obj::<Size>(self.num_comps as usize);
        self.recon_comp_size = self
            .allocator
            .post_alloc_obj::<Size>(self.num_comps as usize);
        self.employ_color_transform = self.cod.is_employing_color_transform();

        for i in 0..self.num_comps as usize {
            let idx = i as Ui32;
            let cw = self.siz.get_recon_width(idx) as usize;
            unsafe {
                ptr::write(
                    self.comp_size.add(i),
                    Size::new(self.siz.get_width(idx), self.siz.get_height(idx)),
                );
                ptr::write(
                    self.recon_comp_size.add(i),
                    Size::new(
                        self.siz.get_recon_width(idx),
                        self.siz.get_recon_height(idx),
                    ),
                );
                ptr::write(self.lines.add(i), LineBuf::default());
                (*self.lines.add(i)).wrap_i32(self.allocator.post_alloc_data::<i32>(cw, 0), cw, 0);
            }
        }

        self.cur_comp = 0;
        self.cur_line = 0;

        if self.outfile.is_some() && self.need_tlm {
            // One (tile, length) pair per tile-part, exactly as the C++ sizes it; `Tile::fill_tlm`
            // fills them in `flush`. (Sizing it to 0 made every TLM request index out of bounds.)
            self.tlm.init(num_tileparts, Vec::<TtlmPtlmPair>::new())?;
        }
        Ok(())
    }

    fn check_imf_validity(&mut self) -> OjphResult<()> {
        let reversible = self.cod.is_reversible();
        let mut imf2k = !reversible;
        let mut imf4k = !reversible;
        let mut imf8k = !reversible;
        let mut imf2kls = reversible;
        let mut imf4kls = reversible;
        let mut imf8kls = reversible;

        if reversible {
            let ext = self.siz.get_image_extent();
            if ext.x <= 2048 && ext.y <= 1556 {
                imf2kls &= true;
            }
            if ext.x <= 4096 && ext.y <= 3112 {
                imf4kls &= true;
            }
            if ext.x <= 8192 && ext.y <= 6224 {
                imf8kls &= true;
            }
            if !imf2kls && !imf4kls && !imf8kls {
                return Err(crate::ojph_error!(
                    0x000300C1,
                    "Image dimensions do not meet any of the lossless IMF profiles"
                ));
            }
        } else {
            let ext = self.siz.get_image_extent();
            if ext.x <= 2048 && ext.y <= 1556 {
                imf2k &= true;
            }
            if ext.x <= 4096 && ext.y <= 3112 {
                imf4k &= true;
            }
            if ext.x <= 8192 && ext.y <= 6224 {
                imf8k &= true;
            }
            if !imf2k && !imf4k && !imf8k {
                return Err(crate::ojph_error!(
                    0x000300C2,
                    "Image dimensions do not meet any of the lossy IMF profiles"
                ));
            }
        }

        if self.siz.get_image_offset().x != 0 || self.siz.get_image_offset().y != 0 {
            return Err(crate::ojph_error!(
                0x000300C3,
                "For IMF profile, image offset (XOsiz, YOsiz) has to be 0."
            ));
        }
        if self.siz.get_tile_offset().x != 0 || self.siz.get_tile_offset().y != 0 {
            return Err(crate::ojph_error!(
                0x000300C4,
                "For IMF profile, tile offset (XTOsiz, YTOsiz) has to be 0."
            ));
        }
        if self.siz.get_num_components() > 3 {
            return Err(crate::ojph_error!(
                0x000300C5,
                "For IMF profile, the number of components has to be less  or equal to 3"
            ));
        }

        let mut test_ds1 = true;
        let mut test_ds2 = true;
        for i in 0..self.siz.get_num_components() as Ui32 {
            let downsampling = self.siz.get_downsampling(i);
            test_ds1 &= downsampling.y == 1;
            test_ds2 &= downsampling.y == 1;
            test_ds1 &= downsampling.x == 1;
            if i == 1 || i == 2 {
                test_ds2 &= downsampling.x == 2;
            } else {
                test_ds2 &= downsampling.x == 1;
            }
        }
        if !test_ds1 && !test_ds2 {
            return Err(crate::ojph_error!(
                0x000300C6,
                "For IMF profile, either no component downsampling is used, or the x-dimension of the 2nd and 3rd components is downsampled by 2."
            ));
        }

        let mut test_bd = true;
        for i in 0..self.siz.get_num_components() as Ui32 {
            let bit_depth = self.siz.get_bit_depth(i);
            let is_signed = self.siz.is_signed(i);
            test_bd &= bit_depth >= 8 && bit_depth <= 16 && !is_signed;
        }
        if !test_bd {
            return Err(crate::ojph_error!(
                0x000300C7,
                "For IMF profile, compnent bit_depth has to be between 8 and 16 bits inclusively, and the samples must be unsigned"
            ));
        }

        if self.cod.get_log_block_dims().w != 5 || self.cod.get_log_block_dims().h != 5 {
            return Err(crate::ojph_error!(
                0x000300C8,
                "For IMF profile, codeblock dimensions are restricted. Use \"-block_size {{32,32}}\" at the commandline"
            ));
        }

        let num_decomps = self.cod.get_num_decompositions() as Ui32;
        let mut test_pz =
            self.cod.get_log_precinct_size(0).w == 7 && self.cod.get_log_precinct_size(0).h == 7;
        for i in 1..=num_decomps {
            // &= accumulates: all decomp levels must satisfy the size constraint.
            test_pz &= self.cod.get_log_precinct_size(i).w == 8
                && self.cod.get_log_precinct_size(i).h == 8;
        }
        if !test_pz {
            return Err(crate::ojph_error!(
                0x000300C9,
                "For IMF profile, precinct sizes are restricted. Use \"-precincts {{128,128}},{{256,256}}\" at the commandline"
            ));
        }

        if self.cod.get_progression_order() != ProgressionOrder::OjphPoCprl as Ui8 {
            return Err(crate::ojph_error!(
                0x000300CA,
                "For IMF profile, the CPRL progression order must be used. Use \"-prog_order CPRL\"."
            ));
        }

        imf2k &= num_decomps <= 5;
        imf2kls &= num_decomps <= 5;
        imf4k &= num_decomps <= 6;
        imf4kls &= num_decomps <= 6;
        imf8k &= num_decomps <= 7;
        imf8kls &= num_decomps <= 7;

        if num_decomps == 0 || (!imf2k && !imf4k && !imf8k && !imf2kls && !imf4kls && !imf8kls) {
            return Err(crate::ojph_error!(
                0x000300CB,
                "Number of decompositions does not match the IMF profile dictated by wavelet reversibility and image dimensions."
            ));
        }

        let mut tiles_w = self.siz.get_image_extent().x;
        tiles_w = ojph_div_ceil(tiles_w, self.siz.get_tile_size().w);
        let mut tiles_h = self.siz.get_image_extent().y;
        tiles_h = ojph_div_ceil(tiles_h, self.siz.get_tile_size().h);
        let total_tiles = tiles_w * tiles_h;

        if total_tiles > 1 {
            if !reversible {
                return Err(crate::ojph_error!(0x000300CC, "Lossy IMF profile must have one tile."));
            }

            let tt = self.siz.get_tile_size();
            imf2kls &= tt.w == 1024 && tt.h == 1024;
            imf2kls &= (tt.w >= 1024 && num_decomps <= 4) || (tt.w >= 2048 && num_decomps <= 5);
            imf4kls &= (tt.w == 1024 && tt.h == 1024) || (tt.w == 2048 && tt.h == 2048);
            imf4kls &= (tt.w >= 1024 && num_decomps <= 4)
                || (tt.w >= 2048 && num_decomps <= 5)
                || (tt.w >= 4096 && num_decomps <= 6);
            imf8kls &= (tt.w == 1024 && tt.h == 1024)
                || (tt.w == 2048 && tt.h == 2048)
                || (tt.w == 4096 && tt.h == 4096);
            imf8kls &= (tt.w >= 1024 && num_decomps <= 4)
                || (tt.w >= 2048 && num_decomps <= 5)
                || (tt.w >= 4096 && num_decomps <= 6)
                || (tt.w >= 8192 && num_decomps <= 7);
            if !imf2kls && !imf4kls && !imf8kls {
                return Err(crate::ojph_error!(
                    0x000300CD,
                    "Number of decompositions does not match the IMF profile dictated by wavelet reversibility and image dimensions and tiles."
                ));
            }
        }

        self.need_tlm = true;
        self.tilepart_div |= OJPH_TILEPART_COMPONENTS;
        if self.tilepart_div != OJPH_TILEPART_COMPONENTS {
            self.tilepart_div = OJPH_TILEPART_COMPONENTS;
            crate::ojph_warn!(
                0x000300C1,
                "In IMF profile, tile part divisions at the component level must be employed, while at the resolution level is not allowed. This has been corrected."
            );
        }
        Ok(())
    }

    fn check_broadcast_validity(&mut self) -> OjphResult<()> {
        if self.siz.get_image_offset().x != 0 || self.siz.get_image_offset().y != 0 {
            return Err(crate::ojph_error!(
                0x000300B1,
                "For broadcast profile, image offset (XOsiz, YOsiz) has to be 0."
            ));
        }
        if self.siz.get_tile_offset().x != 0 || self.siz.get_tile_offset().y != 0 {
            return Err(crate::ojph_error!(
                0x000300B2,
                "For broadcast profile, tile offset (XTOsiz, YTOsiz) has to be 0."
            ));
        }
        if self.siz.get_num_components() > 4 {
            return Err(crate::ojph_error!(
                0x000300B3,
                "For broadcast profile, the number of components has to be less  or equal to 4"
            ));
        }

        let mut test_ds1 = true;
        let mut test_ds2 = true;
        for i in 0..self.siz.get_num_components() as Ui32 {
            let downsampling = self.siz.get_downsampling(i);
            test_ds1 &= downsampling.y == 1;
            test_ds2 &= downsampling.y == 1;
            test_ds1 &= downsampling.x == 1;
            if i == 1 || i == 2 {
                test_ds2 &= downsampling.x == 2;
            } else {
                test_ds2 &= downsampling.x == 1;
            }
        }
        if !test_ds1 && !test_ds2 {
            return Err(crate::ojph_error!(
                0x000300B4,
                "For broadcast profile, either no component downsampling is used, or the x-dimension of the 2nd and 3rd components is downsampled by 2."
            ));
        }

        let mut test_bd = true;
        for i in 0..self.siz.get_num_components() as Ui32 {
            let bit_depth = self.siz.get_bit_depth(i);
            let is_signed = self.siz.is_signed(i);
            test_bd &= bit_depth >= 8 && bit_depth <= 12 && !is_signed;
        }
        if !test_bd {
            return Err(crate::ojph_error!(
                0x000300B5,
                "For broadcast profile, compnent bit_depth has to be between 8 and 12 bits inclusively, and the samples must be unsigned"
            ));
        }

        let num_decomps = self.cod.get_num_decompositions() as Ui32;
        if num_decomps == 0 || num_decomps > 5 {
            return Err(crate::ojph_error!(
                0x000300B6,
                "For broadcast profile, number of decompositions has to be between1 and 5 inclusively."
            ));
        }

        if self.cod.get_log_block_dims().w < 5 || self.cod.get_log_block_dims().w > 7 {
            return Err(crate::ojph_error!(
                0x000300B7,
                "For broadcast profile, codeblock dimensions are restricted such that codeblock width has to be either 32, 64, or 128."
            ));
        }
        if self.cod.get_log_block_dims().h < 5 || self.cod.get_log_block_dims().h > 7 {
            return Err(crate::ojph_error!(
                0x000300B8,
                "For broadcast profile, codeblock dimensions are restricted such that codeblock height has to be either 32, 64, or 128."
            ));
        }

        let mut test_pz =
            self.cod.get_log_precinct_size(0).w == 7 && self.cod.get_log_precinct_size(0).h == 7;
        for i in 1..=num_decomps {
            // &= accumulates: all decomp levels must satisfy the size constraint.
            test_pz &= self.cod.get_log_precinct_size(i).w == 8
                && self.cod.get_log_precinct_size(i).h == 8;
        }
        if !test_pz {
            return Err(crate::ojph_error!(
                0x000300B9,
                "For broadcast profile, precinct sizes are restricted. Use \"-precincts {{128,128}},{{256,256}}\" at the commandline"
            ));
        }

        if self.cod.get_progression_order() != ProgressionOrder::OjphPoCprl as Ui8 {
            return Err(crate::ojph_error!(
                0x000300BA,
                "For broadcast profile, the CPRL progression order must be used. Use \"-prog_order CPRL\"."
            ));
        }

        let mut tiles_w = self.siz.get_image_extent().x;
        tiles_w = ojph_div_ceil(tiles_w, self.siz.get_tile_size().w);
        let mut tiles_h = self.siz.get_image_extent().y;
        tiles_h = ojph_div_ceil(tiles_h, self.siz.get_tile_size().h);
        let total_tiles = tiles_w * tiles_h;

        if total_tiles != 1 && total_tiles != 4 {
            return Err(crate::ojph_error!(
                0x000300BB,
                "The broadcast profile can only have 1 or 4 tiles"
            ));
        }

        self.need_tlm = true;
        self.tilepart_div |= OJPH_TILEPART_COMPONENTS;
        if self.tilepart_div != OJPH_TILEPART_COMPONENTS {
            self.tilepart_div = OJPH_TILEPART_COMPONENTS;
            crate::ojph_warn!(
                0x000300B1,
                "In BROADCAST profile, tile part divisions at the component level must be employed, while at the resolution level is not allowed. This has been corrected."
            );
        }
        Ok(())
    }

    pub fn write_headers(
        &mut self,
        file: &mut dyn OutfileBase,
        comments: Option<&[CommentExchange]>,
    ) -> OjphResult<()> {
        self.siz.set_cod(&self.cod);
        let tile_size = self.siz.get_tile_size();
        if tile_size.h == 0 && tile_size.w == 0 {
            let img_offset = self.siz.get_image_offset();
            let img_extent = self.siz.get_image_extent();
            self.siz.set_tile_size(Size::new(
                img_extent.x + img_offset.x,
                img_extent.y + img_offset.y,
            ));
        }
        self.siz.check_validity()?;
        self.cod.check_validity(&self.siz)?;
        self.cod.update_atk(&mut self.atk)?;
        self.qcd.check_validity(&self.siz, &self.cod)?;
        self.cap.check_validity(&self.cod, &self.qcd);
        self.nlt.check_validity(&mut self.siz);
        // Irreversible coding into 64-bit code-block buffers (> 32 magnitude bits, i.e. an extreme
        // step size) has a decoder conversion but no encoder one (OpenJPH leaves it NULL too).
        // Refuse here instead of silently encoding with the reversible converter.
        for c in 0..u32::from(self.siz.get_num_components()) {
            let (coc, qp) = unsafe { (&*self.get_coc(c), &*self.qcd.get_qcc(c)) };
            if !coc.is_reversible() && qp.propose_precision(coc as *const _) > 32 {
                return Err(crate::ojph_error!(
                    0x000300ED,
                    "component {c}: irreversible encoding needs more than 32 magnitude bits; use a larger quantization step"
                ));
            }
        }
        if self.profile == OjphProfileNum::OjphPnImf as i32 {
            self.check_imf_validity()?;
        } else if self.profile == OjphProfileNum::OjphPnBroadcast as i32 {
            self.check_broadcast_validity()?;
        }

        let po = self.cod.get_progression_order();
        if (po == ProgressionOrder::OjphPoLrcp as Ui8 || po == ProgressionOrder::OjphPoRlcp as Ui8)
            && self.tilepart_div == OJPH_TILEPART_COMPONENTS
        {
            self.tilepart_div |= OJPH_TILEPART_RESOLUTIONS;
            crate::ojph_info!(
                0x00030021,
                "For LRCP and RLCP progression orders, tilepart divisions at the component level, means that we have a tilepart for every resolution and component.\n"
            );
        }
        if po == ProgressionOrder::OjphPoRpcl as Ui8
            && (self.tilepart_div & OJPH_TILEPART_COMPONENTS) != 0
        {
            self.tilepart_div &= !OJPH_TILEPART_COMPONENTS;
            crate::ojph_warn!(
                0x00030021,
                "For RPCL progression, having tilepart divisions at the component level means a tilepart for every precinct, which does not make sense, since we can have no more than 255 tile parts. This has been corrected by removing tilepart divisions at the component level."
            );
        }
        if po == ProgressionOrder::OjphPoPcrl as Ui8 && self.tilepart_div != 0 {
            self.tilepart_div = 0;
            crate::ojph_warn!(
                0x00030022,
                "For PCRL progression, having tilepart divisions at the component level or the resolution level means a tile part for every precinct, which does not make sense, since we can have no more than 255 tile parts.  This has been corrected by removing tilepart divisions; use another progression if you want tileparts."
            );
        }
        if po == ProgressionOrder::OjphPoCprl as Ui8
            && (self.tilepart_div & OJPH_TILEPART_RESOLUTIONS) != 0
        {
            self.tilepart_div &= !OJPH_TILEPART_RESOLUTIONS;
            crate::ojph_warn!(
                0x00030023,
                "For CPRL progression, having tilepart divisions at the resolution level means a tile part for every precinct, which does not make sense, since we can have no more than 255 tile parts. This has been corrected by removing tilepart divisions at the resolution level."
            );
        }

        if self.planar == -1 {
            // Deliberate deviation from the 1:1 port here (documented, not silent): OpenJPH
            // 0.31.0's own auto-select resolves colour-transform-on to `planar = 1`
            // (`ojph_codestream_local.cpp:624`, `planar = is_employing_color_transform() ? 1 : 0`)
            // — but `planar == 1` combined with the colour transform is a confirmed *upstream*
            // bug, not just a jph-rs porting issue: a real C++ OpenJPH 0.31.0 build (built from
            // `_ref/OpenJPH`'s 0.31.0 tag) reproduces byte-for-byte identical corruption when fed
            // through that exact path (every component but the last one written comes back as its
            // last row repeated for every row; see `jph-rs/BUG1.md`, "Round 5"). Rather than
            // silently walking into a known-corrupting combination just because the untouched
            // default resolves there, error out the same way the explicit-request branch below
            // already does. This never fires for any caller that explicitly requests the correct
            // scheduling (`set_planar(false)` for colour-transform encodes, as `mem::encode` and
            // the `ojph` CLI both do) — only for a caller that leaves `planar` untouched with
            // colour transform on, which upstream's own auto-select cannot serve correctly either.
            if self.cod.is_employing_color_transform() {
                return Err(crate::ojph_error!(
                    0x00030024,
                    "the planar interface option cannot be used when colour transform is employed; this codestream left `planar` unset and colour transform on, which even upstream OpenJPH 0.31.0's auto-select cannot serve correctly (confirmed corrupting) — call set_planar(false) explicitly"
                ));
            }
            self.planar = 0;
        } else if self.planar == 0 {
        } else if self.planar == 1 {
            if self.cod.is_employing_color_transform() {
                return Err(crate::ojph_error!(
                    0x00030021,
                    "the planar interface option cannot be used when colour transform is employed"
                ));
            }
        } else {
            debug_assert!(false);
        }

        debug_assert!(self.outfile.is_none());
        // OpenJPH stores the active file handle as a raw pointer owned by the caller.
        // This mirrors that model and keeps lifetime management manual, as in C++.
        self.outfile = Some(unsafe {
            mem::transmute::<*mut dyn OutfileBase, *mut dyn OutfileBase>(
                file as *mut dyn OutfileBase,
            )
        });
        self.pre_alloc()?;
        self.finalize_alloc()?;

        let t = swap_byte_u16(Jp2kMarker::Soc as Ui16);
        if file.write(&t.to_ne_bytes())? != 2 {
            return Err(crate::ojph_error!(0x00030022, "Error writing to file"));
        }
        if !self.siz.write(file)? {
            return Err(crate::ojph_error!(0x00030023, "Error writing to file"));
        }
        if !self.cap.write(file)? {
            return Err(crate::ojph_error!(0x00030024, "Error writing to file"));
        }
        if !self.cod.write(file)? {
            return Err(crate::ojph_error!(0x00030025, "Error writing to file"));
        }
        if !self.cod.write_coc(file, self.num_comps)? {
            return Err(crate::ojph_error!(0x0003002E, "Error writing to file"));
        }
        if !self.qcd.write(file)? {
            return Err(crate::ojph_error!(0x00030026, "Error writing to file"));
        }
        if !self.qcd.write_qcc(file, self.num_comps)? {
            return Err(crate::ojph_error!(0x0003002D, "Error writing to file"));
        }
        if !self.nlt.write(file)? {
            return Err(crate::ojph_error!(0x00030027, "Error writing to file"));
        }
        if !write_builtin_version_comment(file)? {
            return Err(crate::ojph_error!(0x00030028, "Error writing to file"));
        }

        if let Some(comments) = comments {
            for comment in comments {
                let t = swap_byte_u16(Jp2kMarker::Com as Ui16);
                if file.write(&t.to_ne_bytes())? != 2 {
                    return Err(crate::ojph_error!(0x00030029, "Error writing to file"));
                }
                let t = swap_byte_u16(comment.len + 4);
                if file.write(&t.to_ne_bytes())? != 2 {
                    return Err(crate::ojph_error!(0x0003002A, "Error writing to file"));
                }
                let t = swap_byte_u16(comment.rcom);
                if file.write(&t.to_ne_bytes())? != 2 {
                    return Err(crate::ojph_error!(0x0003002B, "Error writing to file"));
                }
                let data = unsafe { slice::from_raw_parts(comment.data, comment.len as usize) };
                if file.write(data)? != comment.len as usize {
                    return Err(crate::ojph_error!(0x0003002C, "Error writing to file"));
                }
            }
        }
        Ok(())
    }

    pub fn read_headers(&mut self, file: &mut dyn InfileBase) -> OjphResult<()> {
        let marker_list = [
            Jp2kMarker::Soc as Ui16,
            Jp2kMarker::Siz as Ui16,
            Jp2kMarker::Cap as Ui16,
            Jp2kMarker::Prf as Ui16,
            Jp2kMarker::Cpf as Ui16,
            Jp2kMarker::Cod as Ui16,
            Jp2kMarker::Coc as Ui16,
            Jp2kMarker::Qcd as Ui16,
            Jp2kMarker::Qcc as Ui16,
            Jp2kMarker::Rgn as Ui16,
            Jp2kMarker::Poc as Ui16,
            Jp2kMarker::Ppm as Ui16,
            Jp2kMarker::Tlm as Ui16,
            Jp2kMarker::Plm as Ui16,
            Jp2kMarker::Crg as Ui16,
            Jp2kMarker::Com as Ui16,
            Jp2kMarker::Dfs as Ui16,
            Jp2kMarker::Atk as Ui16,
            Jp2kMarker::Nlt as Ui16,
            Jp2kMarker::Sot as Ui16,
        ];

        find_marker(file, &marker_list[0..1]);
        find_marker(file, &marker_list[1..2]);
        self.siz.read(file)?;
        // Per-component ROI max-shift table; filled by any main-header RGN markers below.
        self.roishift = vec![0u8; self.siz.get_num_components() as usize];

        let mut received_markers = 0i32;
        loop {
            let marker_idx = find_marker(file, &marker_list[2..20]);
            if marker_idx == 0 {
                self.cap.read(file)?;
            } else if marker_idx == 1 {
                skip_marker(file, "PRF", None, OjphMsgLevel::OjphMsgNoMsg, false)?;
            } else if marker_idx == 2 {
                skip_marker(file, "CPF", None, OjphMsgLevel::OjphMsgNoMsg, false)?;
            } else if marker_idx == 3 {
                self.cod.read(file)?;
                received_markers |= 1;
                // Multi-layer (num_layers > 1) is supported for all five progression orders via the
                // multi-layer packet parser (crate::codestream::ojph_packet_ml), validated bit-exact
                // against Kakadu HT codestreams — no reject needed here (cf. OpenJPH, which rejects
                // anything but a single quality layer). num_layers is consumed downstream by the
                // tile parser via ParamCod::get_num_layers().
            } else if marker_idx == 4 {
                let p = self.cod.add_coc_object(ParamCod::OJPH_COD_UNKNOWN as Ui32);
                unsafe {
                    (*p).read_coc(file, self.siz.get_num_components() as Ui32, &mut self.cod)?;
                    if (*p).get_comp_idx() >= self.siz.get_num_components() as Ui32 {
                        crate::ojph_info!(
                            0x00030056,
                            "The codestream carries a COC marker segment for a component indexed by {}, which is more than the allowed index number, since the codestream has {} components",
                            (*p).get_comp_idx(),
                            self.num_comps
                        );
                    }
                    let q = self.cod.get_coc((*p).get_comp_idx());
                    if p != q && (*p).get_comp_idx() == (*q).get_comp_idx() {
                        return Err(crate::ojph_error!(
                            0x00030057,
                            "The codestream has two COC marker segments for one component of index {}",
                            (*p).get_comp_idx()
                        ));
                    }
                }
            } else if marker_idx == 5 {
                self.qcd.read(file)?;
                received_markers |= 2;
            } else if marker_idx == 6 {
                let p = self.qcd.add_qcc_object(ParamQcd::OJPH_QCD_UNKNOWN as Ui32);
                unsafe {
                    (*p).read_qcc(file, self.siz.get_num_components() as Ui32)?;
                    if (*p).get_comp_idx() >= self.siz.get_num_components() {
                        return Err(crate::ojph_error!(
                            0x00030054,
                            "The codestream carries a QCC marker segment for a component indexed by {}, which is more than the allowed index number, since the codestream has {} components",
                            (*p).get_comp_idx(),
                            self.num_comps
                        ));
                    }
                    let q = self.qcd.get_qcc((*p).get_comp_idx() as Ui32);
                    if p != q && (*p).get_comp_idx() == (*q).get_comp_idx() {
                        return Err(crate::ojph_error!(
                            0x00030055,
                            "The codestream has two QCC marker segments for one component of index {}",
                            (*p).get_comp_idx()
                        ));
                    }
                }
            } else if marker_idx == 7 {
                // RGN (0xFF5E): native ROI max-shift. The main-header marker applies to every tile
                // (an in-tile RGN overrides the tile-local copy). OpenJPH skips RGN; this parses it.
                let csiz = self.siz.get_num_components() as Ui32;
                let mut comp: Ui32 = 0;
                let mut shift: Ui8 = 0;
                read_rgn(file, csiz, false, &mut comp, &mut shift)?;
                self.set_roishift(comp, shift);
            } else if marker_idx == 8 {
                // POC (0xFF5F): native progression-order-change support. The marker applies to all
                // tiles unless a tile-part header carries its own POC. Csiz selects 1- vs 2-byte
                // component fields.
                let csiz = self.siz.get_num_components() as Ui32;
                self.poc.read(file, csiz, false)?;
            } else if marker_idx == 9 {
                // PPM (0xFF60): packed packet headers for the whole codestream, stashed for the
                // per-tile-part header source used during packet parsing (see take_ppm_*).
                self.read_ppm(file)?;
            } else if marker_idx == 10 {
                skip_marker(file, "TLM", None, OjphMsgLevel::OjphMsgNoMsg, false)?;
            } else if marker_idx == 11 {
                skip_marker(file, "PLM", None, OjphMsgLevel::OjphMsgNoMsg, false)?;
            } else if marker_idx == 12 {
                skip_marker(
                    file,
                    "CRG",
                    Some(
                        "CRG has been ignored; CRG is related to where the Cb and Cr colour components are co-sited or located with respect to the Y' luma component. Perhaps, it is better to get the individual components and assemble the samples according to your needs",
                    ),
                    OjphMsgLevel::OjphMsgInfo,
                    false,
                )?;
            } else if marker_idx == 13 {
                skip_marker(file, "COM", None, OjphMsgLevel::OjphMsgNoMsg, false)?;
            } else if marker_idx == 14 {
                self.dfs.read(file)?;
            } else if marker_idx == 15 {
                self.atk.read(file)?;
            } else if marker_idx == 16 {
                self.nlt.read(file)?;
            } else if marker_idx == 17 {
                break;
            } else {
                return Err(crate::ojph_error!(0x00030051, "File ended before finding a tile segment"));
            }
        }

        // Same cross-marker rules the encoder enforces (colour transform vs components, RPCL/PCRL
        // downsampling): a hostile stream must not slip past just because it came from a file.
        self.cod.check_validity(&self.siz)?;
        self.cod.update_atk(&mut self.atk)?;
        self.siz.link_cod(&self.cod);
        if self.dfs.exists() {
            self.siz.link_dfs(&self.dfs);
        }

        if received_markers != 3 {
            return Err(crate::ojph_error!(0x00030052, "markers error, COD and QCD are required"));
        }

        // OpenJPH stores the active file handle as a raw pointer owned by the caller.
        // This mirrors that model and keeps lifetime management manual, as in C++.
        self.infile = Some(unsafe {
            mem::transmute::<*mut dyn InfileBase, *mut dyn InfileBase>(file as *mut dyn InfileBase)
        });
        self.planar = if self.cod.is_employing_color_transform() {
            0
        } else {
            1
        };
        Ok(())
    }

    pub fn enable_resilience(&mut self) -> OjphResult<()> {
        if self.infile.is_some() {
            return Err(crate::ojph_error!(
                0x000300A3,
                "Codestream resilience must be enabled before reading file headers.\n"
            ));
        }
        self.resilient = true;
        Ok(())
    }

    /// Parse-only enumeration (additive front-end for `crate::parse::enumerate`): walk every
    /// tile of the fully-parsed tile tree, emitting one [`crate::parse::CodeblockPlan`] per
    /// non-empty code-block, **without** entropy decode or DWT. Implemented here (rather than
    /// on `Codestream`) so it can read the private `num_tiles`/`tiles` fields. Iterates all
    /// tiles in flat row-major order; `Size::area` is the tile count.
    ///
    /// # Safety
    /// Must be called after `read_headers` + `read` (alias `create`): the tile tree must be
    /// allocated and every `coded_cbs` populated. No line has been pulled, so nothing is
    /// entropy-decoded — this is a read-only walk of already-parsed state.
    pub(crate) unsafe fn enumerate_blocks(
        &self,
        sink: &mut dyn FnMut(crate::parse::CodeblockPlan),
    ) {
        let n = self.num_tiles.area() as usize;
        for t in 0..n {
            (*self.tiles.add(t)).enumerate_blocks(t as u32, sink);
        }
    }

    pub fn read(&mut self) -> OjphResult<()> {
        self.pre_alloc()?;
        self.finalize_alloc()?;

        while let Some(infile_ptr) = self.infile {
            let mut sot = ParamSot::default();
            let sot_found = unsafe { sot.read(&mut *infile_ptr, self.resilient)? };
            if sot_found {
                let tile_start_location = unsafe { (&mut *infile_ptr).tell() as u64 };
                let mut skip_tile = false;

                if sot.get_tile_index() as u64 >= self.num_tiles.area() {
                    if self.resilient {
                        crate::ojph_info!(0x00030061, "wrong tile index");
                        skip_tile = true;
                    } else {
                        return Err(crate::ojph_error!(0x00030061, "wrong tile index"));
                    }
                }

                if !skip_tile {
                    if sot.get_tile_part_index() != 0 {
                        if sot.get_num_tile_parts() != 0
                            && sot.get_tile_part_index() >= sot.get_num_tile_parts()
                        {
                            if self.resilient {
                                crate::ojph_info!(
                                    0x00030062,
                                    "error in tile part number, should be smaller than total number of tile parts"
                                );
                            } else {
                                return Err(crate::ojph_error!(
                                    0x00030062,
                                    "error in tile part number, should be smaller than total number of tile parts"
                                ));
                            }
                        }

                        let mut sod_found = false;
                        // Tile-part-local POC; overrides the main-header POC for this tile when present.
                        let mut tp_poc = ParamPoc::default();
                        // In-tile PPT segments for this (subsequent) tile-part, collected by Zppt so
                        // packed headers route through the same guards as the first tile-part.
                        let mut ppt_segs: Vec<(u8, Vec<u8>)> = Vec::new();
                        let other_tile_part_markers = [
                            Jp2kMarker::Sot as Ui16,
                            Jp2kMarker::Poc as Ui16,
                            Jp2kMarker::Ppt as Ui16,
                            Jp2kMarker::Plt as Ui16,
                            Jp2kMarker::Com as Ui16,
                            Jp2kMarker::Nlt as Ui16,
                            Jp2kMarker::Sod as Ui16,
                        ];

                        loop {
                            let marker_idx = unsafe {
                                find_marker(&mut *infile_ptr, &other_tile_part_markers[1..])
                            };
                            let mut result = 0;
                            if marker_idx == 0 {
                                let csiz = self.siz.get_num_components() as Ui32;
                                result =
                                    unsafe { tp_poc.read(&mut *infile_ptr, csiz, self.resilient)? };
                            } else if marker_idx == 1 {
                                let seg = unsafe { self.read_ppt(&mut *infile_ptr)? };
                                ppt_segs.push(seg);
                            } else if marker_idx == 2 {
                                result = unsafe {
                                    skip_marker(
                                        &mut *infile_ptr,
                                        "PLT",
                                        None,
                                        OjphMsgLevel::OjphMsgNoMsg,
                                        self.resilient,
                                    )?
                                };
                            } else if marker_idx == 3 {
                                result = unsafe {
                                    skip_marker(
                                        &mut *infile_ptr,
                                        "COM",
                                        None,
                                        OjphMsgLevel::OjphMsgNoMsg,
                                        self.resilient,
                                    )?
                                };
                            } else if marker_idx == 4 {
                                result = unsafe {
                                    skip_tile_marker(
                                        &mut *infile_ptr,
                                        "NLT",
                                        self.resilient,
                                    )?
                                };
                            } else if marker_idx == 5 {
                                sod_found = true;
                                break;
                            }

                            if marker_idx == -1 {
                                if self.resilient {
                                    crate::ojph_info!(
                                        0x00030063,
                                        "File terminated early before start of data is found for tile indexed {} and tile part {}",
                                        sot.get_tile_index(),
                                        sot.get_tile_part_index()
                                    );
                                } else {
                                    return Err(crate::ojph_error!(
                                        0x00030063,
                                        "File terminated early before start of data is found for tile indexed {} and tile part {}",
                                        sot.get_tile_index(),
                                        sot.get_tile_part_index()
                                    ));
                                }
                                break;
                            }
                            if result == -1 {
                                if self.resilient {
                                    crate::ojph_info!(
                                        0x00030064,
                                        "File terminated during marker segment skipping"
                                    );
                                } else {
                                    return Err(crate::ojph_error!(
                                        0x00030064,
                                        "File terminated during marker segment skipping"
                                    ));
                                }
                                break;
                            }
                        }

                        if sod_found {
                            // Computed before `active_poc` borrows `self.poc`. PPT (this tile) wins
                            // over a PPM tile-part block; both present is non-conformant.
                            if !ppt_segs.is_empty() && self.has_ppm() {
                                return Err(crate::ojph_error!(
                                    0x000300C6,
                                    "codestream carries both PPM and PPT packed packet headers"
                                ));
                            }
                            let pkt_headers: Option<Vec<u8>> = if !ppt_segs.is_empty() {
                                ppt_segs.sort_by_key(|(z, _)| *z);
                                let mut b = Vec::new();
                                for (_, d) in &ppt_segs {
                                    b.extend_from_slice(d);
                                }
                                Some(b)
                            } else if self.has_ppm() {
                                Some(self.take_ppm_tilepart_headers()?)
                            } else {
                                None
                            };
                            let active_poc = if tp_poc.exists() { &tp_poc } else { &self.poc };
                            let buffered = unsafe {
                                (*self.tiles.add(sot.get_tile_index() as usize)).parse_tile_header(
                                    &sot,
                                    &mut *infile_ptr,
                                    &tile_start_location,
                                    active_poc,
                                    pkt_headers,
                                )
                            };
                            if let Some((bytes, headers)) = buffered? {
                                // Clone the tile's active POC (releasing the `active_poc` borrow of
                                // `self`) before the &mut self buffer call.
                                let poc_owned = active_poc.clone();
                                self.buffer_or_sweep_ml_tilepart(&sot, poc_owned, bytes, headers)?;
                            }
                        }
                    } else {
                        let mut sod_found = false;
                        // Tile-part-local POC; overrides the main-header POC for this tile when present.
                        let mut tp_poc = ParamPoc::default();
                        // In-tile PPT packet-header segments for this tile-part, collected by Zppt.
                        let mut ppt_segs: Vec<(u8, Vec<u8>)> = Vec::new();
                        // In-tile QCD/QCC override for this tile (None until such a marker is seen),
                        // seeded from the main QCD and applied to the tile's subbands below.
                        let mut tile_qcd: Option<Box<ParamQcd>> = None;
                        let first_tile_part_markers = [
                            Jp2kMarker::Sot as Ui16,
                            Jp2kMarker::Cod as Ui16,
                            Jp2kMarker::Coc as Ui16,
                            Jp2kMarker::Qcd as Ui16,
                            Jp2kMarker::Qcc as Ui16,
                            Jp2kMarker::Rgn as Ui16,
                            Jp2kMarker::Poc as Ui16,
                            Jp2kMarker::Ppt as Ui16,
                            Jp2kMarker::Plt as Ui16,
                            Jp2kMarker::Com as Ui16,
                            Jp2kMarker::Nlt as Ui16,
                            Jp2kMarker::Sod as Ui16,
                        ];

                        loop {
                            let marker_idx = unsafe {
                                find_marker(&mut *infile_ptr, &first_tile_part_markers[1..])
                            };
                            let mut result = 0;
                            if marker_idx == 0 {
                                result = unsafe {
                                    skip_tile_marker(
                                        &mut *infile_ptr,
                                        "COD",
                                        self.resilient,
                                    )?
                                };
                            } else if marker_idx == 1 {
                                result = unsafe {
                                    skip_tile_marker(
                                        &mut *infile_ptr,
                                        "COC",
                                        self.resilient,
                                    )?
                                };
                            } else if marker_idx == 2 {
                                // In-tile QCD (0xFF5C): per-tile quantization override. Parse it into
                                // a tile-scoped QCD (seeded from the main QCD so components without an
                                // in-tile entry keep the main values) and requantize this tile's
                                // subbands after the header (below) — mirroring how an in-tile RGN
                                // overrides the tile's ROI shift. A parse error surfaces via the
                                // shared `ojph_error!` path inside `ParamQcd::read`.
                                if tile_qcd.is_none() {
                                    let mut q = Box::new(ParamQcd::default());
                                    q.copy_quant_from(self.access_qcd());
                                    tile_qcd = Some(q);
                                }
                                if let Some(q) = tile_qcd.as_mut() {
                                    unsafe {
                                        q.read(&mut *infile_ptr)?;
                                    }
                                }
                            } else if marker_idx == 3 {
                                // In-tile QCC (0xFF5D): per-tile, per-component quantization override.
                                // Same tile-scoped handling as the in-tile QCD above, targeting one
                                // component's entry in the tile QCD chain.
                                if tile_qcd.is_none() {
                                    let mut q = Box::new(ParamQcd::default());
                                    q.copy_quant_from(self.access_qcd());
                                    tile_qcd = Some(q);
                                }
                                let csiz = self.siz.get_num_components() as Ui32;
                                if let Some(q) = tile_qcd.as_mut() {
                                    let p = q.add_qcc_object(ParamQcd::OJPH_QCD_UNKNOWN as Ui32);
                                    unsafe {
                                        (*p).read_qcc(&mut *infile_ptr, csiz)?;
                                    }
                                    q.commit_qcc(p);
                                }
                            } else if marker_idx == 4 {
                                // In-tile RGN (0xFF5E): ROI max-shift for this tile, overriding the
                                // main-header per-component copy made at tile allocation.
                                let csiz = self.siz.get_num_components() as Ui32;
                                let mut comp: Ui32 = 0;
                                let mut shift: Ui8 = 0;
                                result = unsafe {
                                    read_rgn(
                                        &mut *infile_ptr,
                                        csiz,
                                        self.resilient,
                                        &mut comp,
                                        &mut shift,
                                    )?
                                };
                                if result == 0 {
                                    unsafe {
                                        (*self.tiles.add(sot.get_tile_index() as usize))
                                            .set_roishift(comp, shift);
                                    }
                                }
                            } else if marker_idx == 5 {
                                let csiz = self.siz.get_num_components() as Ui32;
                                result =
                                    unsafe { tp_poc.read(&mut *infile_ptr, csiz, self.resilient)? };
                            } else if marker_idx == 6 {
                                // PPT (0xFF61): packed packet headers for this tile, collected for
                                // the header source used during packet parsing.
                                let seg = unsafe { self.read_ppt(&mut *infile_ptr)? };
                                ppt_segs.push(seg);
                            } else if marker_idx == 7 {
                                result = unsafe {
                                    skip_marker(
                                        &mut *infile_ptr,
                                        "PLT",
                                        None,
                                        OjphMsgLevel::OjphMsgNoMsg,
                                        self.resilient,
                                    )?
                                };
                            } else if marker_idx == 8 {
                                result = unsafe {
                                    skip_marker(
                                        &mut *infile_ptr,
                                        "COM",
                                        None,
                                        OjphMsgLevel::OjphMsgNoMsg,
                                        self.resilient,
                                    )?
                                };
                            } else if marker_idx == 9 {
                                result = unsafe {
                                    skip_tile_marker(
                                        &mut *infile_ptr,
                                        "NLT",
                                        self.resilient,
                                    )?
                                };
                            } else if marker_idx == 10 {
                                sod_found = true;
                                break;
                            }

                            if marker_idx == -1 {
                                if self.resilient {
                                    crate::ojph_info!(
                                        0x00030065,
                                        "File terminated early before start of data is found for tile indexed {} and tile part {}",
                                        sot.get_tile_index(),
                                        sot.get_tile_part_index()
                                    );
                                } else {
                                    return Err(crate::ojph_error!(
                                        0x00030065,
                                        "File terminated early before start of data is found for tile indexed {} and tile part {}",
                                        sot.get_tile_index(),
                                        sot.get_tile_part_index()
                                    ));
                                }
                                break;
                            }
                            if result == -1 {
                                if self.resilient {
                                    crate::ojph_info!(
                                        0x00030066,
                                        "File terminated during marker segment skipping"
                                    );
                                } else {
                                    return Err(crate::ojph_error!(
                                        0x00030066,
                                        "File terminated during marker segment skipping"
                                    ));
                                }
                                break;
                            }
                        }

                        if sod_found {
                            // PPM and PPT are mutually exclusive per T.800; a stream carrying both is
                            // non-conformant and would desync the global PPM cursor, so reject loudly
                            // rather than silently mis-decode.
                            if !ppt_segs.is_empty() && self.has_ppm() {
                                return Err(crate::ojph_error!(
                                    0x000300C6,
                                    "codestream carries both PPM and PPT packed packet headers"
                                ));
                            }
                            // Apply any in-tile QCD/QCC override to this tile's subbands before its
                            // packets are parsed/decoded; each tile owns its subbands, so subsequent
                            // tiles keep using the main QCD (no restore needed).
                            if let Some(tq) = tile_qcd.as_deref() {
                                unsafe {
                                    (*self.tiles.add(sot.get_tile_index() as usize))
                                        .requantize(self, tq)?;
                                }
                            }
                            // Packed packet headers for this tile-part (PPT takes precedence over a
                            // PPM tile-part block). Computed before `active_poc` borrows `self.poc`.
                            let pkt_headers: Option<Vec<u8>> = if !ppt_segs.is_empty() {
                                ppt_segs.sort_by_key(|(z, _)| *z);
                                let mut b = Vec::new();
                                for (_, d) in &ppt_segs {
                                    b.extend_from_slice(d);
                                }
                                Some(b)
                            } else if self.has_ppm() {
                                Some(self.take_ppm_tilepart_headers()?)
                            } else {
                                None
                            };
                            let active_poc = if tp_poc.exists() { &tp_poc } else { &self.poc };
                            let buffered = unsafe {
                                (*self.tiles.add(sot.get_tile_index() as usize)).parse_tile_header(
                                    &sot,
                                    &mut *infile_ptr,
                                    &tile_start_location,
                                    active_poc,
                                    pkt_headers,
                                )
                            };
                            if let Some((bytes, headers)) = buffered? {
                                // Clone the tile's active POC (releasing the `active_poc` borrow of
                                // `self`) before the &mut self buffer call.
                                let poc_owned = active_poc.clone();
                                self.buffer_or_sweep_ml_tilepart(&sot, poc_owned, bytes, headers)?;
                            }
                        }
                    }
                }
            }

            let next_markers = [Jp2kMarker::Sot as Ui16, Jp2kMarker::Eoc as Ui16];
            let marker_idx = unsafe { find_marker(&mut *infile_ptr, &next_markers) };
            if marker_idx == -1 {
                crate::ojph_info!(0x00030067, "File terminated early");
                break;
            } else if marker_idx == 0 {
            } else if marker_idx == 1 {
                break;
            }
        }

        // End-of-codestream flush: sweep any multi-layer tiles still buffered because their declared
        // tile-part count was unknown (TNsot == 0) or never reached. The concatenated payload is
        // byte-identical to a single-tile-part packet stream.
        let pending: Vec<(u32, TpBuf)> = self.ml_tp_buf.drain().collect();
        for (tile_idx, tp) in pending {
            self.sweep_ml_buffer(tile_idx, tp)?;
        }
        Ok(())
    }

    /// Accept one buffered multi-layer tile-part payload returned by [`Tile::parse_tile_header`].
    /// Appends it to the per-tile buffer (recording the tile's POC from the first part); once the
    /// SOT-declared tile-part count (`TNsot`) is reached the assembled payload is swept once.
    fn buffer_or_sweep_ml_tilepart(
        &mut self,
        sot: &ParamSot,
        poc: ParamPoc,
        bytes: Vec<u8>,
        headers: Option<Vec<u8>>,
    ) -> OjphResult<()> {
        let tile_idx = sot.get_tile_index() as u32;
        let tnsot = sot.get_num_tile_parts();
        let entry = self
            .ml_tp_buf
            .entry(tile_idx)
            .or_insert_with(|| TpBuf { body: Vec::new(), headers: None, parts: 0, poc });
        entry.body.extend_from_slice(&bytes);
        if let Some(h) = headers {
            entry.headers.get_or_insert_with(Vec::new).extend_from_slice(&h);
        }
        entry.parts = entry.parts.saturating_add(1);
        // A known tile-part count (TNsot != 0) that is now complete: sweep the assembled payload.
        // An unknown count (TNsot == 0) defers to the end-of-codestream flush in `read()`.
        if tnsot != 0 && entry.parts >= tnsot {
            if let Some(tp) = self.ml_tp_buf.remove(&tile_idx) {
                self.sweep_ml_buffer(tile_idx, tp)?;
            }
        }
        Ok(())
    }

    /// Run the packet sweep once over a fully-assembled multi-layer tile payload held in `buf`, via
    /// a `MemInfile`, carrying the tile's POC (`poc`, captured from the first buffered tile-part;
    /// empty when the tile uses no POC). The concatenated payload is byte-identical to a single
    /// tile-part packet stream, so this is the validated single-call sweep path.
    fn sweep_ml_buffer(&mut self, tile_idx: u32, tp: TpBuf) -> OjphResult<()> {
        let TpBuf { body: buf, headers, poc, .. } = tp;
        let phdr = headers.map(|buf| crate::codestream::ojph_packet_ml::PacketHeaderReader {
            buf,
            pos: 0,
            tmp: 0,
            avail_bits: 0,
            unstuff: false,
        });
        // `buf` owns the bytes; `mem` holds a raw pointer into it and is declared after `buf`, so it
        // is dropped first — `buf` stays valid for the whole sweep.
        let mut mem = crate::openjph::ojph_file::MemInfile::default();
        mem.open(&buf);
        unsafe {
            (*self.tiles.add(tile_idx as usize)).parse_packets(
                buf.len() as Ui32,
                &mut mem,
                &poc,
                phdr,
            )?;
        }
        Ok(())
    }

    #[inline]
    pub const fn is_trunc_quant(&self) -> bool {
        self.trunc_quant
    }

    /// Choose the lossy forward quantizer rounding (see the `trunc_quant` field); call before
    /// `write_headers`/`create`.
    pub fn set_trunc_quant(&mut self, trunc: bool) {
        self.trunc_quant = trunc;
    }

    #[inline]
    pub const fn is_resilient(&self) -> bool {
        self.resilient
    }

    #[inline]
    pub fn set_planar(&mut self, planar: i32) {
        self.planar = planar;
    }

    pub fn set_profile(&mut self, s: &str) -> OjphResult<()> {
        let len = s.len();
        if len == 9 && &s[..9] == OJPH_PN_STRING_BROADCAST {
            self.profile = OjphProfileNum::OjphPnBroadcast as i32;
        } else if len == 3 && &s[..3] == OJPH_PN_STRING_IMF {
            self.profile = OjphProfileNum::OjphPnImf as i32;
        } else {
            return Err(crate::ojph_error!(0x000300A1, "unkownn or unsupported profile"));
        }
        Ok(())
    }

    #[inline]
    pub fn set_tilepart_divisions(&mut self, value: Ui32) {
        self.tilepart_div = value;
    }

    #[inline]
    pub fn request_tlm_marker(&mut self, needed: bool) {
        self.need_tlm = needed;
    }

    pub fn flush(&mut self) -> OjphResult<()> {
        let repeat = self.num_tiles.area() as usize;
        for i in 0..repeat {
            unsafe {
                (*self.tiles.add(i)).prepare_for_flush();
            }
        }

        if self.need_tlm {
            for i in 0..repeat {
                unsafe {
                    (*self.tiles.add(i)).fill_tlm(&mut self.tlm);
                }
            }

            if let Some(outfile_ptr) = self.outfile {
                unsafe {
                    self.tlm.write(&mut *outfile_ptr)?;
                }
            }
        }

        if let Some(outfile_ptr) = self.outfile {
            for i in 0..repeat {
                unsafe {
                    (*self.tiles.add(i)).flush(&mut *outfile_ptr)?;
                }
            }

            let t = swap_byte_u16(Jp2kMarker::Eoc as Ui16);
            let written = unsafe { (&mut *outfile_ptr).write(&t.to_ne_bytes())? };
            if written != 2 {
                return Err(crate::ojph_error!(0x00030071, "Error writing to file"));
            }
        }
        Ok(())
    }

    pub fn close(&mut self) {
        if let Some(infile) = self.infile {
            unsafe {
                (&mut *infile).close();
            }
        }
        if let Some(outfile) = self.outfile {
            unsafe {
                (&mut *outfile).close();
            }
        }
    }

    pub fn exchange(
        &mut self,
        line: *mut LineBuf,
        next_component: &mut Ui32,
    ) -> OjphResult<*mut LineBuf> {
        if !line.is_null() {
            let mut success = false;
            while !success {
                success = true;
                for i in 0..self.num_tiles.w {
                    let idx = i + self.cur_tile_row * self.num_tiles.w;
                    unsafe {
                        debug_assert!(!self.tiles.is_null(), "tiles is null");
                        debug_assert!(
                            (idx as usize) < (self.num_tiles.w * self.num_tiles.h) as usize,
                            "tile index out of bounds"
                        );
                        success &= (*self.tiles.add(idx as usize)).push(line, self.cur_comp)?;
                        if !success {
                            break;
                        }
                    }
                }
                self.cur_tile_row += if success { 0 } else { 1 };
                if self.cur_tile_row >= self.num_tiles.h {
                    self.cur_tile_row = 0;
                }
            }

            if self.planar != 0 {
                unsafe {
                    if {
                        self.cur_line += 1;
                        self.cur_line
                    } >= (*self.comp_size.add(self.cur_comp as usize)).h
                    {
                        self.cur_line = 0;
                        self.cur_tile_row = 0;
                        self.cur_comp += 1;
                        if self.cur_comp >= self.num_comps {
                            *next_component = 0;
                            return Ok(ptr::null_mut());
                        }
                    }
                }
            } else {
                self.cur_comp += 1;
                if self.cur_comp >= self.num_comps {
                    self.cur_comp = 0;
                    unsafe {
                        if {
                            self.cur_line += 1;
                            self.cur_line
                        } >= (*self.comp_size.add(self.cur_comp as usize)).h
                        {
                            *next_component = 0;
                            return Ok(ptr::null_mut());
                        }
                    }
                }
            }
        }

        *next_component = self.cur_comp;
        let result = unsafe { self.lines.add(self.cur_comp as usize) };
        debug_assert!(!result.is_null(), "exchange returning null line");
        Ok(result)
    }

    pub fn pull(&mut self, comp_num: &mut Ui32) -> OjphResult<*mut LineBuf> {
        let mut success = false;
        // A tile row that cannot supply a line advances `cur_tile_row`; once EVERY row has failed
        // in a row nothing can ever supply one (e.g. a sub-sampled component whose line count is
        // exhausted before the others'), so the old unbounded loop spun forever on such streams.
        let mut failed_rows: Ui32 = 0;
        while !success {
            success = true;
            for i in 0..self.num_tiles.w {
                let idx = i + self.cur_tile_row * self.num_tiles.w;
                unsafe {
                    success &= (*self.tiles.add(idx as usize))
                        .pull(self.lines.add(self.cur_comp as usize), self.cur_comp)?;
                    if !success {
                        break;
                    }
                }
            }
            if !success {
                failed_rows += 1;
                if failed_rows > self.num_tiles.h {
                    return Err(crate::ojph_error!(
                        0x000300A9,
                        "no tile row can supply a line for component {} (corrupt or sub-sampled codestream)
",
                        self.cur_comp
                    ));
                }
            }
            self.cur_tile_row += if success { 0 } else { 1 };
            if self.cur_tile_row >= self.num_tiles.h {
                self.cur_tile_row = 0;
            }
        }
        *comp_num = self.cur_comp;

        if self.planar != 0 {
            unsafe {
                if {
                    self.cur_line += 1;
                    self.cur_line
                } >= (*self.recon_comp_size.add(self.cur_comp as usize)).h
                {
                    self.cur_line = 0;
                    self.cur_tile_row = 0;
                    let old_comp = self.cur_comp;
                    self.cur_comp += 1;
                    if old_comp >= self.num_comps {
                        *comp_num = 0;
                        return Ok(ptr::null_mut());
                    }
                }
            }
        } else {
            self.cur_comp += 1;
            if self.cur_comp >= self.num_comps {
                self.cur_comp = 0;
                unsafe {
                    let old_line = self.cur_line;
                    self.cur_line += 1;
                    if old_line >= (*self.recon_comp_size.add(self.cur_comp as usize)).h {
                        *comp_num = 0;
                        return Ok(ptr::null_mut());
                    }
                }
            }
        }

        Ok(unsafe { self.lines.add(*comp_num as usize) })
    }

    pub fn restrict_input_resolution(
        &mut self,
        skipped_res_for_read: Ui32,
        skipped_res_for_recon: Ui32,
    ) -> OjphResult<()> {
        if skipped_res_for_read < skipped_res_for_recon {
            return Err(crate::ojph_error!(
                0x000300A1,
                "skipped_resolution for data {} must be equal or smaller than  skipped_resolution for reconstruction {}\n",
                skipped_res_for_read,
                skipped_res_for_recon
            ));
        }
        // Every component must still have a resolution left: a COC may give a component fewer
        // decomposition levels than the main COD (and `1 << skip` must stay below 32).
        let min_decomps = (0..u32::from(self.siz.get_num_components()))
            .map(|c| unsafe { (*self.get_coc(c)).get_num_decompositions() as Ui32 })
            .min()
            .unwrap_or(self.cod.get_num_decompositions() as Ui32)
            .min(31);
        if skipped_res_for_read > min_decomps {
            return Err(crate::ojph_error!(
                0x000300A2,
                "skipped_resolution for data {} must be smaller than  the number of decomposition levels {}\n",
                skipped_res_for_read,
                self.cod.get_num_decompositions()
            ));
        }

        self.skipped_res_for_read = skipped_res_for_read;
        self.skipped_res_for_recon = skipped_res_for_recon;
        self.siz.set_skipped_resolutions(skipped_res_for_recon);
        Ok(())
    }

    #[inline]
    pub const fn is_planar(&self) -> bool {
        self.planar != 0
    }

    #[inline]
    pub const fn get_profile(&self) -> i32 {
        self.profile
    }

    #[inline]
    pub const fn get_tilepart_div(&self) -> Ui32 {
        self.tilepart_div
    }

    #[inline]
    pub const fn is_tlm_needed(&self) -> bool {
        self.need_tlm
    }
}

fn write_builtin_version_comment(file: &mut dyn OutfileBase) -> OjphResult<bool> {
    let mut buf = format!(
        "      OpenJPH Ver {}.{}.{}.",
        OPENJPH_VERSION_MAJOR, OPENJPH_VERSION_MINOR, OPENJPH_VERSION_PATCH
    )
    .into_bytes();
    let len = buf.len();
    let marker = swap_byte_u16(Jp2kMarker::Com as Ui16).to_ne_bytes();
    let lcom = swap_byte_u16((len - 2) as Ui16).to_ne_bytes();
    let rcom = swap_byte_u16(1).to_ne_bytes();
    buf[0..2].copy_from_slice(&marker);
    buf[2..4].copy_from_slice(&lcom);
    buf[4..6].copy_from_slice(&rcom);
    Ok(file.write(&buf)? == len)
}

fn find_marker(file: &mut dyn InfileBase, char_list: &[Ui16]) -> i32 {
    while !file.eof() {
        let mut new_char = [0u8; 1];
        if file.read(&mut new_char) != 1 {
            return -1;
        }
        if new_char[0] == 0xFF {
            if file.read(&mut new_char) != 1 {
                return -1;
            }
            for (i, marker) in char_list.iter().enumerate() {
                if new_char[0] == (marker & 0xFF) as u8 {
                    return i as i32;
                }
            }
        }
    }
    -1
}

/// Tile-header marker (COD/COC/NLT) that would change how this tile decodes but is not applied by
/// this decoder. Skipping it silently would misdecode, so strict mode refuses the stream; resilient
/// mode keeps the OpenJPH behaviour (warn, skip, decode with the main-header values).
fn skip_tile_marker(file: &mut dyn InfileBase, marker: &str, resilient: bool) -> OjphResult<i32> {
    if !resilient {
        return Err(crate::ojph_error!(
            0x000300EB,
            "{marker} marker segment in a tile is not supported (decoding it with the main-header values would be wrong); use resilient mode to ignore it"
        ));
    }
    let msg = format!("{marker} marker segment in a tile is not supported yet");
    skip_marker(file, marker, Some(&msg), OjphMsgLevel::OjphMsgWarn, true)
}

fn skip_marker(
    file: &mut dyn InfileBase,
    marker: &str,
    msg: Option<&str>,
    msg_level: OjphMsgLevel,
    resilient: bool,
) -> OjphResult<i32> {
    let mut com_len = [0u8; 2];
    if file.read(&mut com_len) != 2 {
        if resilient {
            return Ok(-1);
        } else {
            return Err(crate::ojph_error!(0x00030041, "error reading marker"));
        }
    }
    let com_len = swap_byte_u16(Ui16::from_ne_bytes(com_len));
    // Lxxx counts its own two bytes, so anything below 2 is corrupt (and `com_len - 2` would wrap
    // into a ~64 KiB forward skip). A seek that cannot be honoured (segment runs past the end of
    // the data) is equally corrupt: stay loud instead of resuming marker search mid-payload.
    if com_len < 2 {
        if resilient {
            return Ok(-1);
        }
        return Err(crate::ojph_error!(0x000300E3, "{marker} marker segment has an invalid length {com_len}"));
    }
    if file.seek((com_len - 2) as i64, InfileSeek::OjphSeekCur) != 0 {
        if resilient {
            return Ok(-1);
        }
        return Err(crate::ojph_error!(0x000300E4, "{marker} marker segment extends past the end of the data"));
    }

    if let Some(msg) = msg {
        if msg_level != OjphMsgLevel::OjphMsgNoMsg {
            if msg_level == OjphMsgLevel::OjphMsgInfo {
                crate::ojph_info!(0x00030001, "{}", msg);
            } else if msg_level == OjphMsgLevel::OjphMsgWarn {
                crate::ojph_warn!(0x00030001, "{}", msg);
            } else if msg_level == OjphMsgLevel::OjphMsgError {
                return Err(crate::ojph_error!(0x00030001, "{}", msg));
            } else {
                debug_assert!(false);
            }
        }
    }

    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::{LocalCodestream, find_marker, skip_marker, write_builtin_version_comment};
    use crate::codestream::ojph_params_local::OjphProfileNum;
    use crate::openjph::ojph_arch::swap_byte_u16;
    use crate::openjph::ojph_file::{FailingOutfile, InfileBase, MemInfile, MemOutfile, OutfileBase};
    use crate::openjph::ojph_message::OjphMsgLevel;
    use crate::openjph::ojph_version::{
        OPENJPH_VERSION_MAJOR, OPENJPH_VERSION_MINOR, OPENJPH_VERSION_PATCH,
    };

    /// Confirmed-upstream-bug hardening (`BUG1.md`, "Round 5"): OpenJPH 0.31.0's own auto-select
    /// (`planar == -1`, untouched) resolves colour-transform-on to `planar = 1`, and a real C++
    /// OpenJPH 0.31.0 build reproducibly corrupts the codestream through that exact path. Leaving
    /// `planar` untouched with colour transform employed must now error the same way an explicit
    /// `set_planar(true)` + colour transform already does, instead of silently producing a bad
    /// codestream.
    #[test]
    #[should_panic]
    fn write_headers_rejects_untouched_planar_with_colour_transform() {
        let mut cs = LocalCodestream::default();
        {
            let siz = cs.access_siz();
            siz.set_image_extent(crate::openjph::ojph_base::Point::new(32, 32));
            siz.set_tile_size(crate::openjph::ojph_base::Size::new(32, 32));
            siz.set_num_components(3);
            for c in 0..3 {
                siz.set_comp_info(c, crate::openjph::ojph_base::Point::new(1, 1), 8, false);
            }
        }
        {
            let cod = cs.access_cod();
            cod.set_reversible(true);
            cod.employ_color_transform(1);
        }
        // Deliberately do NOT call cs.set_planar() here.
        let mut outfile = MemOutfile::default();
        outfile.open(4096, false).expect("open");
        cs.write_headers(&mut outfile, None).expect("must error (test asserts #[should_panic])");
    }

    #[test]
    fn write_headers_propagates_first_write_failure() {
        let mut cs = LocalCodestream::default();
        {
            let siz = cs.access_siz();
            siz.set_image_extent(crate::openjph::ojph_base::Point::new(16, 16));
            siz.set_tile_size(crate::openjph::ojph_base::Size::new(16, 16));
            siz.set_num_components(1);
            siz.set_comp_info(0, crate::openjph::ojph_base::Point::new(1, 1), 8, false);
        }
        cs.access_cod().set_reversible(true);
        cs.set_planar(0);
        // Otherwise-valid single-component codestream; the very first write (the SOC
        // marker) fails, and write_headers must propagate that Err rather than
        // continuing (or panicking) past a broken sink.
        let mut sink = FailingOutfile::new(0);
        assert!(cs.write_headers(&mut sink, None).is_err());
    }

    #[test]
    fn read_ppm_rejects_undersized_lppm() {
        let mut cs = LocalCodestream::default();
        // Lppm must be >= 3 (it counts itself + Zppm); 2 is malformed.
        let data = [0x00u8, 0x02, 0x00];
        let mut file = MemInfile::default();
        file.open(&data);
        assert!(cs.read_ppm(&mut file).is_err());
    }

    #[test]
    fn read_ppt_rejects_truncated_marker() {
        let cs = LocalCodestream::default();
        // Claims a 2-byte Lppt field but the stream has nothing after it.
        let data = [0x00u8, 0x02];
        let mut file = MemInfile::default();
        file.open(&data);
        assert!(cs.read_ppt(&mut file).is_err());
    }

    #[test]
    fn take_ppm_tilepart_headers_rejects_overclaimed_block_size() {
        let mut cs = LocalCodestream::default();
        // A single PPM segment (Zppm=0) whose payload claims (via the leading
        // Nppm length prefix) more tile-part-header bytes than actually follow.
        let mut payload = vec![0u8; 4];
        payload[0..4].copy_from_slice(&1_000_000u32.to_be_bytes());
        cs.ppm_segments.push((0, payload));
        assert!(cs.take_ppm_tilepart_headers().is_err());
    }

    #[test]
    fn write_headers_rejects_broadcast_profile_component_count_violation() {
        let mut cs = LocalCodestream::default();
        {
            let siz = cs.access_siz();
            siz.set_image_extent(crate::openjph::ojph_base::Point::new(32, 32));
            siz.set_tile_size(crate::openjph::ojph_base::Size::new(32, 32));
            // BROADCAST allows at most 4 components (error 0x000300B3).
            siz.set_num_components(5);
            for c in 0..5 {
                siz.set_comp_info(c, crate::openjph::ojph_base::Point::new(1, 1), 8, false);
            }
        }
        cs.access_cod().set_reversible(true);
        cs.set_planar(1);
        cs.set_profile("BROADCAST").expect("set_profile");
        let mut outfile = MemOutfile::default();
        outfile.open(4096, false).expect("open");
        assert!(cs.write_headers(&mut outfile, None).is_err());
    }

    #[test]
    fn write_headers_rejects_imf_profile_component_count_violation() {
        let mut cs = LocalCodestream::default();
        {
            let siz = cs.access_siz();
            siz.set_image_extent(crate::openjph::ojph_base::Point::new(32, 32));
            siz.set_tile_size(crate::openjph::ojph_base::Size::new(32, 32));
            // IMF allows at most 3 components (error 0x000300C5).
            siz.set_num_components(4);
            for c in 0..4 {
                siz.set_comp_info(c, crate::openjph::ojph_base::Point::new(1, 1), 8, false);
            }
        }
        cs.access_cod().set_reversible(true);
        cs.set_planar(1);
        cs.set_profile("IMF").expect("set_profile");
        let mut outfile = MemOutfile::default();
        outfile.open(4096, false).expect("open");
        assert!(cs.write_headers(&mut outfile, None).is_err());
    }

    #[test]
    fn set_profile_matches_reference_string_dispatch() {
        let mut codestream = LocalCodestream::default();
        codestream.set_profile("BROADCAST").expect("set_profile");
        assert_eq!(
            codestream.get_profile(),
            OjphProfileNum::OjphPnBroadcast as i32
        );
        codestream.set_profile("IMF").expect("set_profile");
        assert_eq!(codestream.get_profile(), OjphProfileNum::OjphPnImf as i32);
    }

    #[test]
    fn builtin_version_comment_matches_reference_layout() {
        let mut file = MemOutfile::default();
        file.open(128, false).expect("open");
        assert!(write_builtin_version_comment(&mut file).expect("write_builtin_version_comment"));
        file.close();

        let mut expected = format!(
            "      OpenJPH Ver {}.{}.{}.",
            OPENJPH_VERSION_MAJOR, OPENJPH_VERSION_MINOR, OPENJPH_VERSION_PATCH
        )
        .into_bytes();
        let expected_len = expected.len();
        expected[0..2].copy_from_slice(&swap_byte_u16(0xFF64).to_ne_bytes());
        expected[2..4].copy_from_slice(&swap_byte_u16((expected_len - 2) as u16).to_ne_bytes());
        expected[4..6].copy_from_slice(&swap_byte_u16(1).to_ne_bytes());

        assert_eq!(file.get_data(), expected.as_slice());
    }

    #[test]
    fn find_marker_matches_reference_byte_scan_shape() {
        let data = [0x01u8, 0xFF, 0x51, 0xAA];
        let mut file = MemInfile::default();
        file.open(&data);

        assert_eq!(find_marker(&mut file, &[0xFF51]), 0);
    }

    #[test]
    fn skip_marker_advances_over_segment_payload() {
        let data = [0x00u8, 0x06, 0xAA, 0xBB, 0xCC, 0xDD];
        let mut file = MemInfile::default();
        file.open(&data);

        assert_eq!(
            skip_marker(&mut file, "COM", None, OjphMsgLevel::OjphMsgNoMsg, false).expect("skip_marker"),
            0
        );
        assert!(file.eof());
    }

    /// A segment length below 2 (it counts its own bytes) or one that runs past the data is a
    /// corrupt header: strict mode errors, resilient mode reports `-1`. Previously `len - 2`
    /// wrapped to a ~64 KiB skip whose failure was ignored.
    #[test]
    fn skip_marker_rejects_bad_lengths() {
        for data in [&[0x00u8, 0x00, 0xAA][..], &[0x00, 0x01, 0xAA], &[0x00, 0x09, 0xAA, 0xBB]] {
            let mut file = MemInfile::default();
            file.open(&data);
            assert!(skip_marker(&mut file, "COM", None, OjphMsgLevel::OjphMsgNoMsg, false).is_err());

            let mut file = MemInfile::default();
            file.open(&data);
            assert_eq!(
                skip_marker(&mut file, "COM", None, OjphMsgLevel::OjphMsgNoMsg, true).expect("resilient"),
                -1
            );
        }
    }
}
