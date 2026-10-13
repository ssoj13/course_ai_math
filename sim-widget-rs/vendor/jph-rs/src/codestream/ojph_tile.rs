//! Port target:
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/codestream/ojph_tile.h`
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/codestream/ojph_tile.cpp`

use crate::codestream::ojph_codestream_local::LocalCodestream;
use crate::codestream::ojph_params_local::{
    Jp2kMarker, OJPH_TILEPART_COMPONENTS, OJPH_TILEPART_MASK, OJPH_TILEPART_NO_DIVISIONS,
    OJPH_TILEPART_RESOLUTIONS, ParamPoc, ParamQcd, ParamSot, ParamTlm, PocEntry, ProgressionOrder,
};
use crate::codestream::ojph_tile_comp::TileComp;
use crate::openjph::ojph_arch::swap_byte_u16;
use crate::openjph::ojph_base::{Point, Rect, Size};
use crate::openjph::ojph_defs::{Si64, Ui8, Ui16, Ui32, ojph_div_ceil, ojph_max};
use crate::openjph::ojph_file::{InfileBase, InfileSeek, OutfileBase};
use crate::openjph::ojph_mem::LineBuf;
use crate::openjph::ojph_message::OjphResult;
use crate::openjph::ojph_params::Nonlinearity;
use crate::transform::ojph_colour::{
    ict_backward, ict_forward, irv_convert_to_float, irv_convert_to_float_nlt_type3,
    irv_convert_to_integer, irv_convert_to_integer_nlt_type3, rct_backward, rct_forward,
    rev_convert, rev_convert_nlt_type3,
};
use core::ptr;

/// Payload length of a tile-part whose SOT has `Psot == 0` (T.800 A.4.2: the last tile-part of the
/// codestream, extending to the EOC marker). Measured from `start` (just after the SOT segment) to
/// the end of the data, excluding a trailing EOC (`FFD9`) when present; the file position is
/// restored. Errors if the data cannot be measured or does not fit the `u32` the parser uses.
fn open_ended_payload_length(file: &mut dyn InfileBase, start: u64) -> OjphResult<Ui32> {
    let here = file.tell();
    let fail = || crate::ojph_error!(0x000300E5, "cannot determine the length of a tile-part with Psot == 0");
    if file.seek(0, InfileSeek::OjphSeekEnd) != 0 {
        return Err(fail());
    }
    let end = file.tell() as u64;
    let mut eoc = [0u8; 2];
    let has_eoc = end >= start + 2
        && file.seek(-2, InfileSeek::OjphSeekEnd) == 0
        && file.read(&mut eoc) == 2
        && eoc == [0xFF, 0xD9];
    if file.seek(here, InfileSeek::OjphSeekSet) != 0 {
        return Err(fail());
    }
    let len = end.saturating_sub(start).saturating_sub(if has_eoc { 2 } else { 0 });
    Ui32::try_from(len).map_err(|_| fail())
}

#[repr(C)]
pub struct Tile {
    tile_rect: Rect,
    num_comps: Ui32,
    comps: *mut TileComp,
    num_lines: Ui32,
    lines: *mut LineBuf,
    employ_color_transform: bool,
    resilient: bool,
    reversible: *mut bool,
    comp_rects: *mut Rect,
    recon_comp_rects: *mut Rect,
    line_offsets: *mut Ui32,
    skipped_res_for_read: Ui32,
    num_bits: *mut Ui32,
    is_signed: *mut bool,
    cur_line: *mut Ui32,
    nlt_type3: *mut Ui8,
    /// Per-component ROI max-shift (RGN, `0xFF5E`), 0 = none. Initialised from the codestream's
    /// main-header RGN table at allocation; an in-tile RGN overrides the entry for this tile.
    roishift: *mut Ui8,
    prog_order: i32,
    /// Number of quality layers (COD marker). 1 for the OpenJPH-faithful single-layer path; >1
    /// drives the multi-layer packet parser (layer-innermost progression orders only, for now).
    num_layers: Ui16,
    /// `Ccap^15` (CAP marker word 0), needed by the multi-layer parser to set the `HT_MIXED` flag.
    ccap15: Ui16,
    sot: ParamSot,
    next_tile_part: i32,
    /// This tile's packets span several tile-parts and are being buffered for ONE sweep (set by
    /// the first part; later parts may carry no POC of their own yet must follow the first).
    defer_packets: bool,
    profile: i32,
    tilepart_div: Ui32,
    need_tlm: bool,
    num_bytes: Ui32,
}

impl Default for Tile {
    fn default() -> Self {
        Self {
            tile_rect: Rect::default(),
            num_comps: 0,
            comps: ptr::null_mut(),
            num_lines: 0,
            lines: ptr::null_mut(),
            employ_color_transform: false,
            resilient: false,
            reversible: ptr::null_mut(),
            comp_rects: ptr::null_mut(),
            recon_comp_rects: ptr::null_mut(),
            line_offsets: ptr::null_mut(),
            skipped_res_for_read: 0,
            num_bits: ptr::null_mut(),
            is_signed: ptr::null_mut(),
            cur_line: ptr::null_mut(),
            nlt_type3: ptr::null_mut(),
            roishift: ptr::null_mut(),
            prog_order: 0,
            num_layers: 1,
            ccap15: 0,
            sot: ParamSot::default(),
            next_tile_part: 0,
            defer_packets: false,
            profile: 0,
            tilepart_div: OJPH_TILEPART_NO_DIVISIONS,
            need_tlm: false,
            num_bytes: 0,
        }
    }
}

/// One step of the position-driven packet walk (see [`walk_pos`]).
enum Walk {
    /// Start of an outer group: a resolution (RPCL) or a component (CPRL); the writer opens its
    /// tile-part here. Never emitted for PCRL/PRCL.
    Group(Ui32),
    /// The next packet-sequence element: component `.0`, resolution `.1`, precinct raster index `.2`
    /// (as reported by the cursor; `0` and unused for [`TileCursor`], whose owner tracks it).
    Packet(usize, Ui32, u64),
}

/// Where each `(component, resolution)` stands in its precinct raster. The one thing that differs
/// between the plain progression orders (cursor stored in the tile) and POC (every progression
/// entry restarts the walk over precincts that may already have some layers parsed).
trait PrecinctCursor {
    /// Top-left of the next unvisited precinct, `None` when the resolution is exhausted/absent.
    fn peek(&mut self, c: usize, r: Ui32) -> Option<Point>;
    /// Raster index of the precinct [`Self::peek`] reports.
    fn index(&mut self, c: usize, r: Ui32) -> u64;
    /// Mark the reported precinct visited.
    fn advance(&mut self, c: usize, r: Ui32);
}

/// Cursor kept inside each `TileComp` resolution; the visit callback advances it itself
/// (`write_one_precinct` / `parse_one_precinct`), so `advance` has nothing to do.
struct TileCursor(*mut TileComp);

impl PrecinctCursor for TileCursor {
    fn peek(&mut self, c: usize, r: Ui32) -> Option<Point> {
        let mut p = Point::default();
        unsafe { (*self.0.add(c)).get_top_left_precinct(r, &mut p) }.then_some(p)
    }
    fn index(&mut self, _c: usize, _r: Ui32) -> u64 {
        0
    }
    fn advance(&mut self, _c: usize, _r: Ui32) {}
}

/// Per-`(component, resolution)` raster cursors owned by the POC iterator.
struct PocCursor {
    comps: *mut TileComp,
    next: std::collections::HashMap<(usize, Ui32), u64>,
}

impl PrecinctCursor for PocCursor {
    fn peek(&mut self, c: usize, r: Ui32) -> Option<Point> {
        let res = unsafe { (*self.comps.add(c)).get_res(r) };
        if res.is_null() {
            return None;
        }
        let cur = self.index(c, r);
        unsafe { (cur < (*res).poc_num_precincts()).then(|| (*res).poc_precinct_point(cur)) }
    }
    fn index(&mut self, c: usize, r: Ui32) -> u64 {
        *self.next.get(&(c, r)).unwrap_or(&0)
    }
    fn advance(&mut self, c: usize, r: Ui32) {
        *self.next.entry((c, r)).or_insert(0) += 1;
    }
}

/// Visit candidates `(component, resolution)` in order and return the one whose next precinct has
/// the smallest top-left position (raster: y, then x). Only a strictly smaller position replaces
/// the pick, so ties go to the first visited candidate; `None` when no candidate has precincts left.
fn pick_pos(cur: &mut dyn PrecinctCursor, cands: &[(usize, Ui32)]) -> Option<(usize, Ui32)> {
    let mut best: Option<(usize, Ui32, Point)> = None;
    for &(c, r) in cands {
        let Some(pt) = cur.peek(c, r) else { continue };
        if best.is_none_or(|(_, _, b)| pt.y < b.y || (pt.y == b.y && pt.x < b.x)) {
            best = Some((c, r, pt));
        }
    }
    best.map(|(c, r, _)| (c, r))
}

/// The single packet iterator for the position-driven progression orders, shared by the encoder
/// (`Tile::flush`), the decoder (`Tile::parse_packets`) and the POC iterator (`Tile::poc_parse`),
/// so they cannot drift apart. Over components `comps` and resolutions `res`:
/// * RPCL: for each resolution, repeatedly the component/precinct with the smallest position;
/// * PCRL: smallest position first, ties broken component-major;
/// * PRCL (Part-2): as PCRL but ties broken resolution-major;
/// * CPRL: for each component, smallest position over its resolutions.
///
/// `nd(c)` is component `c`'s decomposition count (its resolutions are `0..=nd(c)`). `f` receives
/// [`Walk`] events; returning `Ok(false)` from a `Packet` ends the current group (used when the
/// payload is exhausted). After `f` returns, the walk advances the cursor.
fn walk_pos(
    cur: &mut dyn PrecinctCursor,
    nd: &dyn Fn(usize) -> Ui32,
    order: i32,
    comps: std::ops::Range<usize>,
    res: std::ops::Range<Ui32>,
    f: &mut dyn FnMut(Walk) -> OjphResult<bool>,
) -> OjphResult<()> {
    let drain = |cur: &mut dyn PrecinctCursor,
                 cands: &[(usize, Ui32)],
                 f: &mut dyn FnMut(Walk) -> OjphResult<bool>|
     -> OjphResult<()> {
        while let Some((c, r)) = pick_pos(cur, cands) {
            let idx = cur.index(c, r);
            let go = f(Walk::Packet(c, r, idx))?;
            cur.advance(c, r);
            if !go {
                break;
            }
        }
        Ok(())
    };
    if order == ProgressionOrder::OjphPoRpcl as i32 {
        for r in res {
            f(Walk::Group(r))?;
            let cands: Vec<_> = comps.clone().map(|c| (c, r)).collect();
            drain(cur, &cands, f)?;
        }
    } else if order == ProgressionOrder::OjphPoPcrl as i32 {
        let cands: Vec<_> = comps
            .flat_map(|c| (res.start..res.end.min(nd(c) + 1)).map(move |r| (c, r)))
            .collect();
        drain(cur, &cands, f)?;
    } else if order == ProgressionOrder::OjphPoPrcl as i32 {
        let cands: Vec<_> = res
            .flat_map(|r| comps.clone().filter(move |&c| r <= nd(c)).map(move |c| (c, r)))
            .collect();
        drain(cur, &cands, f)?;
    } else if order == ProgressionOrder::OjphPoCprl as i32 {
        for c in comps {
            f(Walk::Group(c as Ui32))?;
            let cands: Vec<_> = res.clone().map(|r| (c, r)).collect();
            drain(cur, &cands, f)?;
        }
    } else {
        return Err(crate::ojph_error!(0x000300EE, "progression order {order} is not position-driven"));
    }
    Ok(())
}

/// Emit one packet `(component c, resolution r, precinct pidx, layer l)` for the POC iterator.
///
/// `next_layer[key]` is the next unparsed layer for a precinct and doubles as the emit-once filter:
/// `l < next` means an earlier progression entry already produced this packet (skip, and consume no
/// bytes — it is not at this stream position), `l == next` parses it (the [`MlPrecinct`] accumulator
/// in `map` persists across entries so layers accumulate correctly), and `l > next` is an
/// impossible out-of-order gap for a well-formed POC (every entry's layer range starts at 0) and is
/// skipped defensively. No-op once the tile payload is exhausted.
#[allow(clippy::too_many_arguments)]
unsafe fn poc_emit_layer(
    comps: *mut TileComp,
    c: usize,
    r: Ui32,
    pidx: u64,
    l: Ui16,
    ccap15: Ui16,
    map: &mut std::collections::HashMap<
        (usize, Ui32, u64),
        crate::codestream::ojph_packet_ml::MlPrecinct,
    >,
    next_layer: &mut std::collections::HashMap<(usize, Ui32, u64), Ui16>,
    data_left: &mut Ui32,
    file: &mut dyn InfileBase,
    phdr: &mut Option<crate::codestream::ojph_packet_ml::PacketHeaderReader>,
) -> crate::openjph::ojph_message::OjphResult<()> {
    let key = (c, r, pidx);
    let nxt = next_layer.get(&key).copied().unwrap_or(0);
    if l != nxt {
        return Ok(());
    }
    if *data_left == 0 {
        return Ok(());
    }
    let res = unsafe { (*comps.add(c)).get_res(r) };
    if res.is_null() {
        return Ok(());
    }
    let mlp = map.entry(key).or_default();
    unsafe {
        (*res).poc_parse_layer(pidx, l, ccap15, mlp, data_left, file, phdr)?;
    }
    next_layer.insert(key, l + 1);
    Ok(())
}

impl Tile {
    /// Parse-only enumeration (additive front-end for `crate::parse::enumerate`): walk every
    /// component of this tile, emitting one [`crate::parse::CodeblockPlan`] per non-empty
    /// code-block, **without** entropy decode or DWT. `tile_idx` is this tile's flat index in
    /// the tile grid, carried into each block's geometry.
    ///
    /// # Safety
    /// The tile must be fully parsed (`read_headers` + `create`); guaranteed by the caller
    /// (`LocalCodestream::enumerate_blocks`).
    pub(crate) unsafe fn enumerate_blocks(
        &self,
        tile_idx: u32,
        sink: &mut dyn FnMut(crate::parse::CodeblockPlan),
    ) {
        for c in 0..self.num_comps as usize {
            (*self.comps.add(c)).enumerate_blocks(tile_idx, sink);
        }
    }

    pub fn pre_alloc(
        codestream: &mut LocalCodestream,
        tile_rect: Rect,
        recon_tile_rect: Rect,
        num_tileparts: &mut Ui32,
    ) -> OjphResult<()> {
        let allocator = unsafe { &mut *codestream.get_allocator() };
        let num_comps = codestream.get_siz().get_num_components() as Ui32;
        allocator.pre_alloc_obj::<TileComp>(num_comps as usize);
        allocator.pre_alloc_obj::<Rect>(num_comps as usize);
        allocator.pre_alloc_obj::<Rect>(num_comps as usize);
        allocator.pre_alloc_obj::<Ui32>(num_comps as usize);
        allocator.pre_alloc_obj::<Ui32>(num_comps as usize);
        allocator.pre_alloc_obj::<bool>(num_comps as usize);
        allocator.pre_alloc_obj::<bool>(num_comps as usize);
        allocator.pre_alloc_obj::<Ui8>(num_comps as usize);
        allocator.pre_alloc_obj::<Ui32>(num_comps as usize);
        allocator.pre_alloc_obj::<Ui8>(num_comps as usize); // roishift

        let tilepart_div = codestream.get_tilepart_div();
        let t = tilepart_div & OJPH_TILEPART_MASK;
        if t == OJPH_TILEPART_NO_DIVISIONS {
            *num_tileparts = 1;
        } else if t == OJPH_TILEPART_COMPONENTS {
            *num_tileparts = num_comps;
        } else if t == OJPH_TILEPART_RESOLUTIONS {
            let mut max_decs = 0;
            for c in 0..num_comps {
                let s = unsafe { (*codestream.get_coc(c)).get_num_decompositions() as Ui32 };
                max_decs = ojph_max(max_decs, s);
            }
            *num_tileparts = 1 + max_decs;
        } else if t == (OJPH_TILEPART_COMPONENTS | OJPH_TILEPART_RESOLUTIONS) {
            *num_tileparts = 0;
            for c in 0..num_comps {
                let s = unsafe { (*codestream.get_coc(c)).get_num_decompositions() as Ui32 };
                *num_tileparts += s + 1;
            }
        }
        if *num_tileparts > 255 {
            return Err(crate::ojph_error!(
                0x000300D1,
                "Trying to create {} tileparts; a tile cannot have more than 255 tile parts.",
                *num_tileparts
            ));
        }

        let tx0 = tile_rect.org.x;
        let ty0 = tile_rect.org.y;
        let tx1 = tile_rect.org.x + tile_rect.siz.w;
        let ty1 = tile_rect.org.y + tile_rect.siz.h;
        let recon_tx0 = recon_tile_rect.org.x;
        let recon_ty0 = recon_tile_rect.org.y;
        let recon_tx1 = recon_tile_rect.org.x + recon_tile_rect.siz.w;
        let recon_ty1 = recon_tile_rect.org.y + recon_tile_rect.siz.h;

        let mut width = 0;
        let mut colour_comp_rect = [Rect::default(); 3];
        for i in 0..num_comps {
            let downsamp = {
                let szp = codestream.get_siz();
                szp.get_downsampling(i)
            };
            let tcx0 = ojph_div_ceil(tx0, downsamp.x);
            let tcy0 = ojph_div_ceil(ty0, downsamp.y);
            let tcx1 = ojph_div_ceil(tx1, downsamp.x);
            let tcy1 = ojph_div_ceil(ty1, downsamp.y);
            let recon_tcx0 = ojph_div_ceil(recon_tx0, downsamp.x);
            let recon_tcy0 = ojph_div_ceil(recon_ty0, downsamp.y);
            let recon_tcx1 = ojph_div_ceil(recon_tx1, downsamp.x);
            let recon_tcy1 = ojph_div_ceil(recon_ty1, downsamp.y);
            let comp_rect = Rect {
                org: Point { x: tcx0, y: tcy0 },
                siz: Size {
                    w: tcx1 - tcx0,
                    h: tcy1 - tcy0,
                },
            };
            let recon_comp_rect = Rect {
                org: Point {
                    x: recon_tcx0,
                    y: recon_tcy0,
                },
                siz: Size {
                    w: recon_tcx1 - recon_tcx0,
                    h: recon_tcy1 - recon_tcy0,
                },
            };
            TileComp::pre_alloc(codestream, i, comp_rect, recon_comp_rect)?;
            width = ojph_max(width, recon_comp_rect.siz.w);
            if i < 3 {
                colour_comp_rect[i as usize] = comp_rect;
            }
        }

        if codestream.get_cod().is_employing_color_transform() {
            let rev0 = unsafe { (*codestream.get_coc(0)).is_reversible() };
            let rev1 = unsafe { (*codestream.get_coc(1)).is_reversible() };
            let rev2 = unsafe { (*codestream.get_coc(2)).is_reversible() };
            if rev0 != rev1 || rev1 != rev2 {
                return Err(crate::ojph_error!(
                    0x000300A2,
                    "When the colour transform is employed, all colour components must \
                     undergo either reversible or irreversible wavelet transform; if not, \
                     then it is not clear what colour transform should be used (reversible \
                     or irreversible).  Here we found that the first three colour components \
                     uses {}, {}, and {} transforms, respectively.",
                    if rev0 { "reversible" } else { "irreversible" },
                    if rev1 { "reversible" } else { "irreversible" },
                    if rev2 { "reversible" } else { "irreversible" }
                ));
            }
            // OpenJPH v0.31.0 hardening: the colour transform (ICT/RCT) operates on the first
            // three components jointly, so they must cover the same rectangle (origin and size)
            // on the canvas; otherwise the transform mixes samples that don't correspond to the
            // same pixel.
            if colour_comp_rect[0] != colour_comp_rect[1] || colour_comp_rect[1] != colour_comp_rect[2]
            {
                return Err(crate::ojph_error!(
                    0x000300A3,
                    "When the colour transform is employed, the first three colour components must have the same rectangle; i.e., the same origin on the canvas and the same width and height. The first three components have the following origin-size (x,y)-(w,h) values. Component 0 ({},{})-({},{}), Component 1 ({},{})-({},{}), Component 2 ({},{})-({},{})",
                    colour_comp_rect[0].org.x, colour_comp_rect[0].org.y, colour_comp_rect[0].siz.w, colour_comp_rect[0].siz.h,
                    colour_comp_rect[1].org.x, colour_comp_rect[1].org.y, colour_comp_rect[1].siz.w, colour_comp_rect[1].siz.h,
                    colour_comp_rect[2].org.x, colour_comp_rect[2].org.y, colour_comp_rect[2].siz.w, colour_comp_rect[2].siz.h
                ));
            }
            allocator.pre_alloc_obj::<LineBuf>(3);
            if rev0 {
                for _ in 0..3 {
                    allocator.pre_alloc_data::<i32>(width as usize, 0);
                }
            } else {
                for _ in 0..3 {
                    allocator.pre_alloc_data::<f32>(width as usize, 0);
                }
            }
        }
        Ok(())
    }

    pub fn finalize_alloc(
        &mut self,
        codestream: &mut LocalCodestream,
        tile_rect: Rect,
        tile_idx: Ui32,
        offset: &mut Ui32,
        num_tileparts: &mut Ui32,
    ) -> OjphResult<()> {
        let allocator = unsafe { &mut *codestream.get_allocator() };
        self.sot.init(0, tile_idx as u16, 0, 1);
        self.prog_order = codestream.access_cod().get_progression_order() as i32;
        self.num_layers = codestream.access_cod().get_num_layers();
        self.ccap15 = codestream.get_ccap15();
        self.num_bytes = 0;
        self.num_comps = codestream.get_siz().get_num_components() as Ui32;
        self.skipped_res_for_read = codestream.get_skipped_res_for_read();
        self.comps = allocator.post_alloc_obj::<TileComp>(self.num_comps as usize);
        self.comp_rects = allocator.post_alloc_obj::<Rect>(self.num_comps as usize);
        self.recon_comp_rects = allocator.post_alloc_obj::<Rect>(self.num_comps as usize);
        self.line_offsets = allocator.post_alloc_obj::<Ui32>(self.num_comps as usize);
        self.num_bits = allocator.post_alloc_obj::<Ui32>(self.num_comps as usize);
        self.is_signed = allocator.post_alloc_obj::<bool>(self.num_comps as usize);
        self.reversible = allocator.post_alloc_obj::<bool>(self.num_comps as usize);
        self.nlt_type3 = allocator.post_alloc_obj::<Ui8>(self.num_comps as usize);
        self.cur_line = allocator.post_alloc_obj::<Ui32>(self.num_comps as usize);
        self.roishift = allocator.post_alloc_obj::<Ui8>(self.num_comps as usize);
        self.profile = codestream.get_profile();
        self.tilepart_div = codestream.get_tilepart_div();
        self.need_tlm = codestream.is_tlm_needed();
        self.tile_rect = tile_rect;
        let t = self.tilepart_div & OJPH_TILEPART_MASK;
        if t == OJPH_TILEPART_NO_DIVISIONS {
            *num_tileparts = 1;
        } else if t == OJPH_TILEPART_COMPONENTS {
            *num_tileparts = self.num_comps;
        } else if t == OJPH_TILEPART_RESOLUTIONS {
            let mut max_decs = 0;
            for c in 0..self.num_comps {
                let s = unsafe { (*codestream.get_coc(c)).get_num_decompositions() as Ui32 };
                max_decs = ojph_max(max_decs, s);
            }
            *num_tileparts = 1 + max_decs;
        } else if t == (OJPH_TILEPART_COMPONENTS | OJPH_TILEPART_RESOLUTIONS) {
            *num_tileparts = 0;
            for c in 0..self.num_comps {
                let s = unsafe { (*codestream.get_coc(c)).get_num_decompositions() as Ui32 };
                *num_tileparts += s + 1;
            }
        }
        if *num_tileparts > 255 {
            return Err(crate::ojph_error!(
                0x000300D1,
                "Trying to create {} tileparts; a tile cannot have more than 255 tile parts.",
                *num_tileparts
            ));
        }

        self.resilient = codestream.is_resilient();
        let tx0 = tile_rect.org.x;
        let ty0 = tile_rect.org.y;
        let tx1 = tile_rect.org.x + tile_rect.siz.w;
        let ty1 = tile_rect.org.y + tile_rect.siz.h;
        let mut width = 0;
        for i in 0..self.num_comps as usize {
            let comp_idx = i as Ui32;
            let mut bd = 0u8;
            let mut is = false;
            let mut nlt_type = 0u8;
            let (downsamp, recon_downsamp, bit_depth, signed) = {
                let szp = codestream.get_siz();
                (
                    szp.get_downsampling(comp_idx),
                    szp.get_recon_downsampling(comp_idx),
                    szp.get_bit_depth(comp_idx),
                    szp.is_signed(comp_idx),
                )
            };
            let tcx0 = ojph_div_ceil(tx0, downsamp.x);
            let tcy0 = ojph_div_ceil(ty0, downsamp.y);
            let tcx1 = ojph_div_ceil(tx1, downsamp.x);
            let tcy1 = ojph_div_ceil(ty1, downsamp.y);
            let recon_tcx0 = ojph_div_ceil(tx0, recon_downsamp.x);
            let recon_tcy0 = ojph_div_ceil(ty0, recon_downsamp.y);
            let recon_tcx1 = ojph_div_ceil(tx1, recon_downsamp.x);
            let recon_tcy1 = ojph_div_ceil(ty1, recon_downsamp.y);
            unsafe {
                *self.line_offsets.add(i) =
                    recon_tcx0 - ojph_div_ceil(tx0 - *offset, recon_downsamp.x);
                ptr::write(
                    self.comp_rects.add(i),
                    Rect {
                        org: Point { x: tcx0, y: tcy0 },
                        siz: Size {
                            w: tcx1 - tcx0,
                            h: tcy1 - tcy0,
                        },
                    },
                );
                ptr::write(
                    self.recon_comp_rects.add(i),
                    Rect {
                        org: Point {
                            x: recon_tcx0,
                            y: recon_tcy0,
                        },
                        siz: Size {
                            w: recon_tcx1 - recon_tcx0,
                            h: recon_tcy1 - recon_tcy0,
                        },
                    },
                );
                ptr::write(self.comps.add(i), TileComp::default());
                (*self.comps.add(i)).finalize_alloc(
                    codestream,
                    self,
                    comp_idx,
                    *self.comp_rects.add(i),
                    *self.recon_comp_rects.add(i),
                )?;
                width = ojph_max(width, (*self.recon_comp_rects.add(i)).siz.w);
                *self.num_bits.add(i) = bit_depth;
                *self.is_signed.add(i) = signed;
                // Seed the tile's ROI shift from the main-header RGN table; an in-tile RGN may
                // override it later (see set_roishift, called from the tile-part marker loop).
                *self.roishift.add(i) = codestream.get_roishift(comp_idx);
                let result = codestream.get_nlt().get_nonlinear_transform(
                    comp_idx,
                    &mut bd,
                    &mut is,
                    &mut nlt_type,
                );
                if result && (bd != *self.num_bits.add(i) as Ui8 || is != *self.is_signed.add(i)) {
                    return Err(crate::ojph_error!(
                        0x000300A1,
                        "Mismatch between Ssiz (bit_depth = {}, is_signed = {}) from SIZ \
                         marker segment, and BDnlt (bit_depth = {}, is_signed = {}) from \
                         NLT marker segment, for component {}",
                        *self.num_bits.add(i),
                        if *self.is_signed.add(i) {
                            "True"
                        } else {
                            "False"
                        },
                        bd,
                        if is { "True" } else { "False" },
                        i
                    ));
                }
                *self.nlt_type3.add(i) = if result {
                    nlt_type
                } else {
                    Nonlinearity::OjphNltNoNlt as Ui8
                };
                *self.cur_line.add(i) = 0;
                *self.reversible.add(i) = (*codestream.get_coc(comp_idx)).is_reversible();
            }
        }
        *offset += tile_rect.siz.w;
        self.employ_color_transform = codestream.get_cod().is_employing_color_transform();
        if self.employ_color_transform {
            self.num_lines = 3;
            self.lines = allocator.post_alloc_obj::<LineBuf>(3);
            for i in 0..3usize {
                unsafe {
                    ptr::write(self.lines.add(i), LineBuf::default());
                    if *self.reversible {
                        (*self.lines.add(i)).wrap_i32(
                            allocator.post_alloc_data::<i32>(width as usize, 0),
                            width as usize,
                            0,
                        );
                    } else {
                        (*self.lines.add(i)).wrap_f32(
                            allocator.post_alloc_data::<f32>(width as usize, 0),
                            width as usize,
                            0,
                        );
                    }
                }
            }
        } else {
            self.lines = ptr::null_mut();
            self.num_lines = 0;
        }
        self.next_tile_part = 0;
        self.defer_packets = false;
        Ok(())
    }

    /// Apply an in-tile QCD/QCC override (`qcd`) to every component of this tile, recomputing the
    /// per-subband / per-code-block quantization that was finalized from the main QCD. Mirrors how
    /// an in-tile RGN re-targets this tile via [`Self::set_roishift`]. Each tile owns its own
    /// subbands, so subsequent tiles are unaffected and keep using the main QCD (no restore needed).
    pub fn requantize(&mut self, codestream: &LocalCodestream, qcd: &ParamQcd) -> OjphResult<()> {
        for c in 0..self.num_comps as usize {
            unsafe {
                (*self.comps.add(c)).requantize(codestream, qcd)?;
            }
        }
        Ok(())
    }

    pub fn push(&mut self, line: *mut LineBuf, comp_num: Ui32) -> OjphResult<bool> {
        let type3 = Nonlinearity::OjphNltBinaryComplementNlt as Ui8;
        debug_assert!(comp_num < self.num_comps);
        unsafe {
            if *self.cur_line.add(comp_num as usize)
                >= (*self.comp_rects.add(comp_num as usize)).siz.h
            {
                return Ok(false);
            }
            *self.cur_line.add(comp_num as usize) += 1;

            if !self.employ_color_transform || comp_num >= 3 {
                let comp_width = (*self.comp_rects.add(comp_num as usize)).siz.w;
                let tc = (*self.comps.add(comp_num as usize)).get_line();
                if *self.reversible.add(comp_num as usize) {
                    let mut shift = (1i64) << (*self.num_bits.add(comp_num as usize) - 1);
                    if *self.is_signed.add(comp_num as usize)
                        && *self.nlt_type3.add(comp_num as usize) == type3
                    {
                        rev_convert_nlt_type3(
                            line,
                            *self.line_offsets.add(comp_num as usize),
                            tc,
                            0,
                            shift + 1,
                            comp_width,
                        );
                    } else {
                        shift = if *self.is_signed.add(comp_num as usize) {
                            0
                        } else {
                            -shift
                        };
                        rev_convert(
                            line,
                            *self.line_offsets.add(comp_num as usize),
                            tc,
                            0,
                            shift,
                            comp_width,
                        );
                    }
                } else if *self.nlt_type3.add(comp_num as usize) == type3 {
                    irv_convert_to_float_nlt_type3(
                        line,
                        *self.line_offsets.add(comp_num as usize),
                        tc,
                        *self.num_bits.add(comp_num as usize),
                        *self.is_signed.add(comp_num as usize),
                        comp_width,
                    );
                } else {
                    irv_convert_to_float(
                        line,
                        *self.line_offsets.add(comp_num as usize),
                        tc,
                        *self.num_bits.add(comp_num as usize),
                        *self.is_signed.add(comp_num as usize),
                        comp_width,
                    );
                }
                (*self.comps.add(comp_num as usize)).push_line()?;
            } else {
                let mut shift = (1i64) << (*self.num_bits.add(comp_num as usize) - 1);
                let comp_width = (*self.comp_rects.add(comp_num as usize)).siz.w;
                if *self.reversible.add(comp_num as usize) {
                    if *self.is_signed.add(comp_num as usize)
                        && *self.nlt_type3.add(comp_num as usize) == type3
                    {
                        rev_convert_nlt_type3(
                            line,
                            *self.line_offsets.add(comp_num as usize),
                            self.lines.add(comp_num as usize),
                            0,
                            shift + 1,
                            comp_width,
                        );
                    } else {
                        shift = if *self.is_signed.add(comp_num as usize) {
                            0
                        } else {
                            -shift
                        };
                        rev_convert(
                            line,
                            *self.line_offsets.add(comp_num as usize),
                            self.lines.add(comp_num as usize),
                            0,
                            shift,
                            comp_width,
                        );
                    }
                    if comp_num == 2 {
                        rct_forward(
                            self.lines.add(0),
                            self.lines.add(1),
                            self.lines.add(2),
                            (*self.comps.add(0)).get_line(),
                            (*self.comps.add(1)).get_line(),
                            (*self.comps.add(2)).get_line(),
                            comp_width,
                        );
                        (*self.comps.add(0)).push_line()?;
                        (*self.comps.add(1)).push_line()?;
                        (*self.comps.add(2)).push_line()?;
                    }
                } else {
                    if *self.nlt_type3.add(comp_num as usize) == type3 {
                        irv_convert_to_float_nlt_type3(
                            line,
                            *self.line_offsets.add(comp_num as usize),
                            self.lines.add(comp_num as usize),
                            *self.num_bits.add(comp_num as usize),
                            *self.is_signed.add(comp_num as usize),
                            comp_width,
                        );
                    } else {
                        irv_convert_to_float(
                            line,
                            *self.line_offsets.add(comp_num as usize),
                            self.lines.add(comp_num as usize),
                            *self.num_bits.add(comp_num as usize),
                            *self.is_signed.add(comp_num as usize),
                            comp_width,
                        );
                    }
                    if comp_num == 2 {
                        ict_forward(
                            (*self.lines.add(0)).storage.f32_,
                            (*self.lines.add(1)).storage.f32_,
                            (*self.lines.add(2)).storage.f32_,
                            (*(*self.comps.add(0)).get_line()).storage.f32_,
                            (*(*self.comps.add(1)).get_line()).storage.f32_,
                            (*(*self.comps.add(2)).get_line()).storage.f32_,
                            comp_width,
                        );
                        (*self.comps.add(0)).push_line()?;
                        (*self.comps.add(1)).push_line()?;
                        (*self.comps.add(2)).push_line()?;
                    }
                }
            }
        }
        Ok(true)
    }

    pub fn prepare_for_flush(&mut self) {
        self.num_bytes = 0;
        for c in 0..self.num_comps as usize {
            unsafe {
                self.num_bytes += (*self.comps.add(c)).prepare_precincts();
            }
        }
    }

    pub fn fill_tlm(&mut self, tlm: &mut ParamTlm) {
        if self.tilepart_div == OJPH_TILEPART_NO_DIVISIONS {
            tlm.set_next_pair(self.sot.get_tile_index(), self.num_bytes);
        } else if self.tilepart_div == OJPH_TILEPART_RESOLUTIONS {
            debug_assert!(
                self.prog_order != ProgressionOrder::OjphPoPcrl as i32
                    && self.prog_order != ProgressionOrder::OjphPoCprl as i32
            );
            let mut max_decs = 0;
            for c in 0..self.num_comps as usize {
                unsafe {
                    max_decs = ojph_max(max_decs, (*self.comps.add(c)).get_num_decompositions());
                }
            }
            for r in 0..=max_decs {
                let mut bytes = 0;
                for c in 0..self.num_comps as usize {
                    unsafe {
                        bytes += (*self.comps.add(c)).get_num_bytes_for_resolution(r);
                    }
                }
                tlm.set_next_pair(self.sot.get_tile_index(), bytes);
            }
        } else if self.tilepart_div == OJPH_TILEPART_COMPONENTS {
            if self.prog_order == ProgressionOrder::OjphPoLrcp as i32
                || self.prog_order == ProgressionOrder::OjphPoRlcp as i32
            {
                let mut max_decs = 0;
                for c in 0..self.num_comps as usize {
                    unsafe {
                        max_decs =
                            ojph_max(max_decs, (*self.comps.add(c)).get_num_decompositions());
                    }
                }
                for r in 0..=max_decs {
                    for c in 0..self.num_comps as usize {
                        unsafe {
                            if r <= (*self.comps.add(c)).get_num_decompositions() {
                                tlm.set_next_pair(
                                    self.sot.get_tile_index(),
                                    (*self.comps.add(c)).get_num_bytes_for_resolution(r),
                                );
                            }
                        }
                    }
                }
            } else if self.prog_order == ProgressionOrder::OjphPoCprl as i32 {
                for c in 0..self.num_comps as usize {
                    unsafe {
                        tlm.set_next_pair(
                            self.sot.get_tile_index(),
                            (*self.comps.add(c)).get_num_bytes(),
                        );
                    }
                }
            } else {
                debug_assert!(
                    false,
                    "fill_tlm: unexpected progression order for COMPONENTS division"
                );
            }
        } else {
            // COMPONENTS | RESOLUTIONS combined
            debug_assert!(
                self.prog_order == ProgressionOrder::OjphPoLrcp as i32
                    || self.prog_order == ProgressionOrder::OjphPoRlcp as i32
            );
            let mut max_decs = 0;
            for c in 0..self.num_comps as usize {
                unsafe {
                    max_decs = ojph_max(max_decs, (*self.comps.add(c)).get_num_decompositions());
                }
            }
            for r in 0..=max_decs {
                for c in 0..self.num_comps as usize {
                    unsafe {
                        if r <= (*self.comps.add(c)).get_num_decompositions() {
                            tlm.set_next_pair(
                                self.sot.get_tile_index(),
                                (*self.comps.add(c)).get_num_bytes_for_resolution(r),
                            );
                        }
                    }
                }
            }
        }
    }

    pub fn flush(&mut self, file: &mut dyn OutfileBase) -> OjphResult<()> {
        let mut max_decompositions = 0;
        for c in 0..self.num_comps as usize {
            unsafe {
                max_decompositions = ojph_max(
                    max_decompositions,
                    (*self.comps.add(c)).get_num_decompositions(),
                );
            }
        }

        if self.tilepart_div == OJPH_TILEPART_NO_DIVISIONS {
            if !self.sot.write(file, self.num_bytes)? {
                return Err(crate::ojph_error!(0x00030081, "Error writing to file"));
            }

            let t = swap_byte_u16(Jp2kMarker::Sod as u16);
            if file.write(&t.to_ne_bytes())? != 2 {
                return Err(crate::ojph_error!(0x00030082, "Error writing to file"));
            }
        }

        if self.prog_order == ProgressionOrder::OjphPoLrcp as i32
            || self.prog_order == ProgressionOrder::OjphPoRlcp as i32
        {
            if self.tilepart_div == OJPH_TILEPART_NO_DIVISIONS {
                for r in 0..=max_decompositions {
                    for c in 0..self.num_comps as usize {
                        unsafe { (*self.comps.add(c)).write_precincts(r, file)? };
                    }
                }
            } else if self.tilepart_div == OJPH_TILEPART_RESOLUTIONS {
                for r in 0..=max_decompositions {
                    let mut bytes = 0;
                    for c in 0..self.num_comps as usize {
                        unsafe {
                            bytes += (*self.comps.add(c)).get_num_bytes_for_resolution(r);
                        }
                    }

                    if !self.sot.write_with_tile_part(
                        file,
                        bytes,
                        r as Ui8,
                        (max_decompositions + 1) as Ui8,
                    )? {
                        return Err(crate::ojph_error!(0x00030083, "Error writing to file"));
                    }

                    let t = swap_byte_u16(Jp2kMarker::Sod as u16);
                    if file.write(&t.to_ne_bytes())? != 2 {
                        return Err(crate::ojph_error!(0x00030084, "Error writing to file"));
                    }

                    for c in 0..self.num_comps as usize {
                        unsafe { (*self.comps.add(c)).write_precincts(r, file)? };
                    }
                }
            } else {
                let num_tileparts = self.num_comps * (max_decompositions + 1);
                for r in 0..=max_decompositions {
                    for c in 0..self.num_comps as usize {
                        unsafe {
                            if r <= (*self.comps.add(c)).get_num_decompositions() {
                                if !self.sot.write_with_tile_part(
                                    file,
                                    (*self.comps.add(c)).get_num_bytes_for_resolution(r),
                                    (c as Ui32 + r * self.num_comps) as Ui8,
                                    num_tileparts as Ui8,
                                )? {
                                    return Err(crate::ojph_error!(
                                        0x00030085,
                                        "Error writing to file"
                                    ));
                                }

                                let t = swap_byte_u16(Jp2kMarker::Sod as u16);
                                if file.write(&t.to_ne_bytes())? != 2 {
                                    return Err(crate::ojph_error!(
                                        0x00030086,
                                        "Error writing to file"
                                    ));
                                }
                                (*self.comps.add(c)).write_precincts(r, file)?;
                            }
                        }
                    }
                }
            }
        } else {
            // Position-driven orders (RPCL/PCRL/CPRL): the shared iterator picks the packets; only
            // the tile-part boundaries are emitted here.
            let order = self.prog_order;
            let tilepart_div = self.tilepart_div;
            let (comps, n) = (self.comps, self.num_comps as usize);
            let sot = &mut self.sot;
            let nd = |c: usize| unsafe { (*comps.add(c)).get_num_decompositions() };
            unsafe {
                walk_pos(&mut TileCursor(comps), &nd, order, 0..n, 0..max_decompositions + 1, &mut |ev| {
                    match ev {
                        Walk::Group(g)
                            if order == ProgressionOrder::OjphPoRpcl as i32
                                && tilepart_div == OJPH_TILEPART_RESOLUTIONS =>
                        {
                            let mut bytes = 0;
                            for c in 0..n {
                                bytes += (*comps.add(c)).get_num_bytes_for_resolution(g);
                            }
                            if !sot.write_with_tile_part(file, bytes, g as Ui8, (max_decompositions + 1) as Ui8)? {
                                return Err(crate::ojph_error!(0x00030087, "Error writing to file"));
                            }
                            let t = swap_byte_u16(Jp2kMarker::Sod as u16);
                            if file.write(&t.to_ne_bytes())? != 2 {
                                return Err(crate::ojph_error!(0x00030088, "Error writing to file"));
                            }
                        }
                        Walk::Group(g)
                            if order == ProgressionOrder::OjphPoCprl as i32
                                && tilepart_div == OJPH_TILEPART_COMPONENTS =>
                        {
                            let bytes = (*comps.add(g as usize)).get_num_bytes();
                            if !sot.write_with_tile_part(file, bytes, g as Ui8, n as Ui8)? {
                                return Err(crate::ojph_error!(0x0003008A, "Error writing to file"));
                            }
                            let t = swap_byte_u16(Jp2kMarker::Sod as u16);
                            if file.write(&t.to_ne_bytes())? != 2 {
                                return Err(crate::ojph_error!(0x0003008B, "Error writing to file"));
                            }
                        }
                        Walk::Group(_) => {}
                        Walk::Packet(c, r, _) => (*comps.add(c)).write_one_precinct(r, file)?,
                    }
                    Ok(true)
                })?;
            }
        }
        Ok(())
    }

    /// General Progression-Order-Change packet iterator (ITU-T T.800 §B.12). Active only when a POC
    /// marker is present; the five fixed COD progression orders keep their dedicated,
    /// byte-validated paths. It walks the POC entries followed by a trailing default entry spanning
    /// the whole volume in the COD progression order (exactly as a conformant decoder does), and
    /// emits every packet exactly once — the first entry that contains a packet produces it. A
    /// packet is one quality layer of one precinct; the per-precinct [`MlPrecinct`] accumulator is
    /// kept here and finalized once at the end, because a precinct's layers may be spread across
    /// non-adjacent progression entries.
    fn poc_parse(
        &mut self,
        entries: &[PocEntry],
        max_decomp: Ui32,
        data_left: &mut Ui32,
        file: &mut dyn InfileBase,
        phdr: &mut Option<crate::codestream::ojph_packet_ml::PacketHeaderReader>,
    ) -> OjphResult<()> {
        use crate::codestream::ojph_packet_ml::MlPrecinct;
        use std::collections::HashMap;

        let comps = self.comps;
        let num_comps = self.num_comps;
        let num_layers = self.num_layers;
        let ccap15 = self.ccap15;

        // POC entries, then the trailing full-volume default in COD order. For a complete POC the
        // default contributes nothing (everything is already emitted); it only mops up packets the
        // explicit entries did not cover.
        let mut all: Vec<PocEntry> = entries.to_vec();
        all.push(PocEntry {
            rs: 0,
            re: max_decomp + 1,
            cs: 0,
            ce: num_comps,
            lye: num_layers,
            po: self.prog_order as Ui8,
        });

        let mut map: HashMap<(usize, Ui32, u64), MlPrecinct> = HashMap::new();
        let mut next_layer: HashMap<(usize, Ui32, u64), Ui16> = HashMap::new();

        for e in &all {
            if *data_left == 0 {
                break;
            }
            let lye = e.lye.min(num_layers);
            let cs = e.cs as usize;
            let ce = e.ce.min(num_comps) as usize;
            let rs = e.rs;
            let re = e.re;

            unsafe {
                match e.po {
                    // LRCP: layer, resolution, component, precinct (raster).
                    0 => {
                        for l in 0..lye {
                            for r in rs..re {
                                for c in cs..ce {
                                    if r > (*comps.add(c)).get_num_decompositions() {
                                        continue;
                                    }
                                    let res = (*comps.add(c)).get_res(r);
                                    if res.is_null() {
                                        continue;
                                    }
                                    let n = (*res).poc_num_precincts();
                                    for p in 0..n {
                                        poc_emit_layer(
                                            comps, c, r, p, l, ccap15, &mut map, &mut next_layer,
                                            data_left, file, phdr,
                                        )?;
                                        if *data_left == 0 {
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // RLCP: resolution, layer, component, precinct (raster).
                    1 => {
                        for r in rs..re {
                            for l in 0..lye {
                                for c in cs..ce {
                                    if r > (*comps.add(c)).get_num_decompositions() {
                                        continue;
                                    }
                                    let res = (*comps.add(c)).get_res(r);
                                    if res.is_null() {
                                        continue;
                                    }
                                    let n = (*res).poc_num_precincts();
                                    for p in 0..n {
                                        poc_emit_layer(
                                            comps, c, r, p, l, ccap15, &mut map, &mut next_layer,
                                            data_left, file, phdr,
                                        )?;
                                        if *data_left == 0 {
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // RPCL / PCRL / CPRL: resolution/component/position orders, driven by the same
                    // iterator as the plain progressions but over this entry's component and
                    // resolution window with the POC's own precinct cursors; every chosen precinct
                    // then emits all of the entry's layers.
                    2 | 3 | 4 => {
                        let nd = |c: usize| (*comps.add(c)).get_num_decompositions();
                        let mut cur = PocCursor { comps, next: HashMap::new() };
                        walk_pos(&mut cur, &nd, e.po as i32, cs..ce, rs..re, &mut |ev| {
                            let Walk::Packet(c, r, p) = ev else { return Ok(true) };
                            for l in 0..lye {
                                poc_emit_layer(
                                    comps, c, r, p, l, ccap15, &mut map, &mut next_layer,
                                    data_left, file, phdr,
                                )?;
                                if *data_left == 0 {
                                    return Ok(false);
                                }
                            }
                            Ok(true)
                        })?;
                    }
                    _ => {
                        return Err(crate::ojph_error!(
                            0x000300BD,
                            "unsupported POC progression order {}",
                            e.po
                        ));
                    }
                }
            }
        }

        // Finalize each touched precinct exactly once, now that all of its layers are parsed.
        for (key, mut mlp) in map.into_iter() {
            let (c, r, p) = key;
            unsafe {
                let res = (*comps.add(c)).get_res(r);
                if !res.is_null() {
                    (*res).poc_finalize(p, &mut mlp)?;
                }
            }
        }
        Ok(())
    }

    /// ROI max-shift for component `comp_num` in this tile (0 = none). Defensive against an
    /// out-of-range index or an unallocated table (returns 0).
    pub fn get_roishift(&self, comp_num: Ui32) -> u8 {
        if self.roishift.is_null() || comp_num >= self.num_comps {
            return 0;
        }
        unsafe { *self.roishift.add(comp_num as usize) }
    }

    /// Override the ROI max-shift for `comp_num` in this tile (in-tile RGN marker).
    pub fn set_roishift(&mut self, comp_num: Ui32, shift: u8) {
        if self.roishift.is_null() || comp_num >= self.num_comps {
            return;
        }
        unsafe { *self.roishift.add(comp_num as usize) = shift };
    }

    pub fn parse_tile_header(
        &mut self,
        sot: &ParamSot,
        file: &mut dyn InfileBase,
        tile_start_location: &u64,
        poc: &ParamPoc,
        pkt_headers: Option<Vec<u8>>,
    ) -> OjphResult<Option<(Vec<u8>, Option<Vec<u8>>)>> {
        // Packed packet headers (PPM/PPT) for this tile-part, if any; the packet bodies stay in the
        // tile stream. `None` = the default inline-header layout.
        let phdr = pkt_headers.map(|buf| {
            crate::codestream::ojph_packet_ml::PacketHeaderReader {
                buf,
                pos: 0,
                tmp: 0,
                avail_bits: 0,
                unstuff: false,
            }
        });
        if sot.get_tile_part_index() as i32 != self.next_tile_part {
            if self.resilient {
                crate::ojph_info!(0x00030091, "wrong tile part index");
            } else {
                return Err(crate::ojph_error!(0x00030091, "wrong tile part index"));
            }
        }
        self.next_tile_part += 1;

        let payload_length = match sot.get_payload_length() {
            Some(n) => n,
            None => open_ended_payload_length(file, *tile_start_location)?,
        };
        let tile_end_location = *tile_start_location + payload_length as u64;
        // Bytes already consumed since the SOT segment (tile-part markers + SOD). Guard the
        // unsigned subtraction below: a malformed SOT whose Psot claims a tile-part shorter than
        // the header we just read would otherwise underflow `data_left` into a ~4 GiB value.
        // Reject loudly instead.
        let consumed = (file.tell() as u64).saturating_sub(*tile_start_location) as Ui32;
        if consumed > payload_length {
            if self.resilient {
                crate::ojph_info!(
                    0x000300C2,
                    "tile-part payload length is smaller than its header; skipping tile part"
                );
                return Ok(None);
            }
            return Err(crate::ojph_error!(
                0x000300C2,
                "tile-part payload length {} is smaller than the {} header bytes already consumed",
                payload_length,
                consumed
            ));
        }
        let data_left = payload_length - consumed;

        // An empty tile-part (no packet bytes) carries no data, so return before the multi-tile-part
        // guards below. This lets a multi-layer tile that an encoder split into ONE data-bearing
        // tile-part plus trailing EMPTY tile-parts decode (e.g. Kakadu's RGN conformance streams:
        // 4 tile-parts, all packets in part 0, parts 1-3 carry only SOT+SOD). Empty parts need no
        // cross-tile-part parser state; a *non-empty* subsequent tile-part still trips the guards.
        if data_left == 0 {
            return Ok(None);
        }

        // Multi-layer (num_layers>1) tile whose packets span more than one non-empty tile-part:
        // defer parsing. The tile-part payloads, concatenated, are byte-identical to a single
        // tile-part packet stream, so the caller buffers each payload and runs the (validated)
        // packet sweep ONCE over the concatenation via a MemInfile. We signal that by returning
        // the raw payload bytes here instead of sweeping. `get_num_tile_parts() != 1` covers both
        // a known multi-part count and an unknown count (TNsot == 0, flushed by the caller at
        // EOC); a declared count of 1 stays on the existing validated single-tile-part path.
        // The same applies to a POC tile: its iterator sweeps the whole volume once.
        if self.defer_packets
            || ((self.num_layers > 1 || poc.exists()) && sot.get_num_tile_parts() != 1)
        {
            self.defer_packets = true;
            // Packed packet headers (PPM/PPT) are returned with the payload: every packet header
            // ends on a byte boundary, so the per-tile-part header blocks concatenate into the
            // single header stream the one sweep reads.
            // POC progression across tile-parts IS supported on the buffered route: the caller
            // concatenates the per-tile-part payloads (byte-identical to a single tile-part packet
            // stream) and runs the sweep once, carrying the tile's POC.
            // Grow with the data actually present: `data_left` comes from Psot and can claim up to
            // 4 GiB for a tiny file, which must not be allocated up front.
            let mut bytes = Vec::new();
            let mut remaining = data_left as usize;
            while remaining > 0 {
                let old = bytes.len();
                bytes.resize(old + remaining.min(1 << 16), 0);
                let n = file.read(&mut bytes[old..]);
                bytes.truncate(old + n);
                if n == 0 {
                    break;
                }
                remaining -= n;
            }
            return Ok(Some((bytes, phdr.map(|h| h.buf))));
        }

        // Defensive backstop: a multi-layer tile that declares a single tile-part (TNsot == 1) yet
        // still presents a second non-empty part is malformed (the part-index check in `read()`
        // already rejects most such streams). The single-call sweep below keeps no cross-tile-part
        // state, so reject rather than silently double-accumulate.
        let use_poc = poc.exists();
        if (self.num_layers > 1 || use_poc) && self.next_tile_part > 1 {
            return Err(crate::ojph_error!(
                0x000300B5,
                "multi-layer or POC decode across multiple non-empty tile-parts needs a declared tile-part count (TNsot != 1)"
            ));
        }
        self.parse_packets(data_left, file, poc, phdr)?;
        file.seek(tile_end_location as Si64, InfileSeek::OjphSeekSet);
        Ok(None)
    }

    /// Run the packet sweep for one fully-assembled tile payload: compute `max_decompositions`,
    /// then iterate precincts in the tile's progression order (or via the POC iterator),
    /// materialising each packet. Extracted from [`Self::parse_tile_header`] so a multi-layer tile
    /// split across several tile-parts can buffer the per-tile-part payloads and call this ONCE over
    /// the concatenation (the concatenated payloads are byte-identical to a single-tile-part packet
    /// stream). `data_left` is the count of packet bytes available in `file`; `phdr` carries PPM/PPT
    /// packed packet headers when in use. Does NOT reposition `file` afterwards — the caller
    /// normalises the real-file cursor to the tile-part end.
    pub fn parse_packets(
        &mut self,
        mut data_left: Ui32,
        file: &mut dyn InfileBase,
        poc: &ParamPoc,
        mut phdr: Option<crate::codestream::ojph_packet_ml::PacketHeaderReader>,
    ) -> OjphResult<()> {
        let use_poc = poc.exists();
        let poc_entries: Vec<PocEntry> = poc.entries().to_vec();

        let mut max_decompositions = 0;
        for c in 0..self.num_comps as usize {
            unsafe {
                max_decompositions = ojph_max(
                    max_decompositions,
                    (*self.comps.add(c)).get_num_decompositions(),
                );
            }
        }

        let result: OjphResult<()> = (|| -> OjphResult<()> {
            if use_poc {
                // A POC marker (main-header or tile-part) fully describes the progression; route the
                // whole tile through the general POC iterator. `skipped_res_for_read` is guaranteed
                // 0 here (rejected above), so `max_decompositions` is the full resolution count.
                self.poc_parse(&poc_entries, max_decompositions, &mut data_left, file, &mut phdr)?;
            } else if self.prog_order == ProgressionOrder::OjphPoLrcp as i32
                || self.prog_order == ProgressionOrder::OjphPoRlcp as i32
            {
                if self.num_layers > 1 {
                    // Layer-outermost orders interleave every resolution inside each layer, so the
                    // skipped (dropped) resolutions' packets must still be walked to keep later
                    // layers aligned; `Resolution::skipped_res_for_read` makes them header-only.
                    // Layer-outermost: each precinct is revisited once per layer; persistent
                    // per-precinct state lives in `map` for the whole tile parse.
                    let nl = self.num_layers;
                    let mut map: std::collections::HashMap<
                        usize,
                        crate::codestream::ojph_packet_ml::MlPrecinct,
                    > = std::collections::HashMap::new();
                    let is_lrcp = self.prog_order == ProgressionOrder::OjphPoLrcp as i32;
                    if is_lrcp {
                        // L-R-C-P: layer outermost.
                        for l in 0..nl {
                            let last = l + 1 == nl;
                            for r in 0..=max_decompositions {
                                for c in 0..self.num_comps as usize {
                                    // The last layer must still be entered when the payload is
                                    // exhausted (e.g. an encoder allocated a final empty layer):
                                    // parse_all_precincts_ml runs its precinct finalize on the last
                                    // layer, and skipping the call would leave every block
                                    // un-materialised (decoded as zero).
                                    if data_left > 0 || last {
                                        unsafe {
                                            (*self.comps.add(c)).parse_precincts_ml(
                                                r, l, self.ccap15, last, &mut map,
                                                &mut data_left, file, &mut phdr,
                                            )
                                        }?;
                                    }
                                }
                            }
                        }
                    } else {
                        // R-L-C-P: layer is the second loop, last layer per resolution.
                        for r in 0..=max_decompositions {
                            for l in 0..nl {
                                let last = l + 1 == nl;
                                for c in 0..self.num_comps as usize {
                                    // The last layer must still be entered when the payload is
                                    // exhausted (e.g. an encoder allocated a final empty layer):
                                    // parse_all_precincts_ml runs its precinct finalize on the last
                                    // layer, and skipping the call would leave every block
                                    // un-materialised (decoded as zero).
                                    if data_left > 0 || last {
                                        unsafe {
                                            (*self.comps.add(c)).parse_precincts_ml(
                                                r, l, self.ccap15, last, &mut map,
                                                &mut data_left, file, &mut phdr,
                                            )
                                        }?;
                                    }
                                }
                            }
                        }
                    }
                } else {
                    max_decompositions -= self.skipped_res_for_read;
                    for r in 0..=max_decompositions {
                        for c in 0..self.num_comps as usize {
                            if data_left > 0 {
                                unsafe {
                                    (*self.comps.add(c)).parse_precincts(r, &mut data_left, file, &mut phdr)
                                }?;
                            }
                        }
                    }
                }
            } else {
                // Position-driven orders: same shared iterator as the writer. RPCL stops before the
                // resolutions dropped for reading (nothing follows them); the others walk every
                // resolution, the dropped ones being consumed header-only by `Resolution`.
                if self.prog_order == ProgressionOrder::OjphPoRpcl as i32 {
                    max_decompositions -= self.skipped_res_for_read;
                }
                let (comps, n, nl, ccap15) = (self.comps, self.num_comps as usize, self.num_layers, self.ccap15);
                let nd = |c: usize| unsafe { (*comps.add(c)).get_num_decompositions() };
                unsafe {
                    walk_pos(&mut TileCursor(comps), &nd, self.prog_order, 0..n, 0..max_decompositions + 1, &mut |ev| {
                        let Walk::Packet(c, r, _) = ev else { return Ok(true) };
                        if data_left == 0 {
                            return Ok(false);
                        }
                        if nl > 1 {
                            (*comps.add(c)).parse_one_precinct_ml(r, nl, ccap15, &mut data_left, file, &mut phdr)?;
                        } else {
                            (*comps.add(c)).parse_one_precinct(r, &mut data_left, file, &mut phdr)?;
                        }
                        Ok(true)
                    })?;
                }
            }
            Ok(())
        })();

        // The whole port is Result-based now (no `ojph_error!` panics reach here), so this is the
        // ordinary error path: on failure, resilient mode logs and continues; strict mode propagates.
        match result {
            Ok(()) => Ok(()),
            Err(e) => {
                if self.resilient {
                    crate::ojph_info!(0x00030092, "{}", e);
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    }

    pub fn pull(&mut self, tgt_line: *mut LineBuf, comp_num: Ui32) -> OjphResult<bool> {
        let type3 = Nonlinearity::OjphNltBinaryComplementNlt as Ui8;
        debug_assert!(comp_num < self.num_comps);
        unsafe {
            if *self.cur_line.add(comp_num as usize)
                >= (*self.recon_comp_rects.add(comp_num as usize)).siz.h
            {
                return Ok(false);
            }
            *self.cur_line.add(comp_num as usize) += 1;

            let comp_width = (*self.recon_comp_rects.add(comp_num as usize)).siz.w;
            if comp_width == 0 {
                return Ok(true);
            }

            if !self.employ_color_transform || self.num_comps == 1 {
                let src_line = (*self.comps.add(comp_num as usize)).pull_line()?;
                if *self.reversible.add(comp_num as usize) {
                    let mut shift = (1i64) << (*self.num_bits.add(comp_num as usize) - 1);
                    if *self.is_signed.add(comp_num as usize)
                        && *self.nlt_type3.add(comp_num as usize) == type3
                    {
                        rev_convert_nlt_type3(
                            src_line,
                            0,
                            tgt_line,
                            *self.line_offsets.add(comp_num as usize),
                            shift + 1,
                            comp_width,
                        );
                    } else {
                        shift = if *self.is_signed.add(comp_num as usize) {
                            0
                        } else {
                            shift
                        };
                        rev_convert(
                            src_line,
                            0,
                            tgt_line,
                            *self.line_offsets.add(comp_num as usize),
                            shift,
                            comp_width,
                        );
                    }
                } else if *self.nlt_type3.add(comp_num as usize) == type3 {
                    irv_convert_to_integer_nlt_type3(
                        src_line,
                        tgt_line,
                        *self.line_offsets.add(comp_num as usize),
                        *self.num_bits.add(comp_num as usize),
                        *self.is_signed.add(comp_num as usize),
                        comp_width,
                    );
                } else {
                    irv_convert_to_integer(
                        src_line,
                        tgt_line,
                        *self.line_offsets.add(comp_num as usize),
                        *self.num_bits.add(comp_num as usize),
                        *self.is_signed.add(comp_num as usize),
                        comp_width,
                    );
                }
            } else {
                debug_assert!(self.num_comps >= 3);
                if comp_num == 0 {
                    if *self.reversible.add(comp_num as usize) {
                        rct_backward(
                            (*self.comps.add(0)).pull_line()?,
                            (*self.comps.add(1)).pull_line()?,
                            (*self.comps.add(2)).pull_line()?,
                            self.lines.add(0),
                            self.lines.add(1),
                            self.lines.add(2),
                            comp_width,
                        );
                    } else {
                        ict_backward(
                            (*(*self.comps.add(0)).pull_line()?).storage.f32_,
                            (*(*self.comps.add(1)).pull_line()?).storage.f32_,
                            (*(*self.comps.add(2)).pull_line()?).storage.f32_,
                            (*self.lines.add(0)).storage.f32_,
                            (*self.lines.add(1)).storage.f32_,
                            (*self.lines.add(2)).storage.f32_,
                            comp_width,
                        );
                    }
                }

                if *self.reversible.add(comp_num as usize) {
                    let mut shift = (1i64) << (*self.num_bits.add(comp_num as usize) - 1);
                    let src_line = if comp_num < 3 {
                        self.lines.add(comp_num as usize)
                    } else {
                        (*self.comps.add(comp_num as usize)).pull_line()?
                    };
                    if *self.is_signed.add(comp_num as usize)
                        && *self.nlt_type3.add(comp_num as usize) == type3
                    {
                        rev_convert_nlt_type3(
                            src_line,
                            0,
                            tgt_line,
                            *self.line_offsets.add(comp_num as usize),
                            shift + 1,
                            comp_width,
                        );
                    } else {
                        shift = if *self.is_signed.add(comp_num as usize) {
                            0
                        } else {
                            shift
                        };
                        rev_convert(
                            src_line,
                            0,
                            tgt_line,
                            *self.line_offsets.add(comp_num as usize),
                            shift,
                            comp_width,
                        );
                    }
                } else {
                    let lbp = if comp_num < 3 {
                        self.lines.add(comp_num as usize)
                    } else {
                        (*self.comps.add(comp_num as usize)).pull_line()?
                    };
                    if *self.nlt_type3.add(comp_num as usize) == type3 {
                        irv_convert_to_integer_nlt_type3(
                            lbp,
                            tgt_line,
                            *self.line_offsets.add(comp_num as usize),
                            *self.num_bits.add(comp_num as usize),
                            *self.is_signed.add(comp_num as usize),
                            comp_width,
                        );
                    } else {
                        irv_convert_to_integer(
                            lbp,
                            tgt_line,
                            *self.line_offsets.add(comp_num as usize),
                            *self.num_bits.add(comp_num as usize),
                            *self.is_signed.add(comp_num as usize),
                            comp_width,
                        );
                    }
                }
            }
        }
        Ok(true)
    }

    pub const fn get_tile_rect(&self) -> Rect {
        self.tile_rect
    }
}
