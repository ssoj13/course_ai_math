//! Port target:
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/codestream/ojph_tile_comp.h`
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/codestream/ojph_tile_comp.cpp`

use crate::codestream::ojph_codestream_local::LocalCodestream;
use crate::codestream::ojph_params_local::ParamQcd;
use crate::codestream::ojph_resolution::Resolution;
use crate::codestream::ojph_tile::Tile;
use crate::openjph::ojph_base::{Point, Rect};
use crate::openjph::ojph_defs::{Ui16, Ui32};
use crate::openjph::ojph_file::{InfileBase, OutfileBase};
use crate::openjph::ojph_mem::LineBuf;
use crate::openjph::ojph_message::OjphResult;
use core::ptr;

#[repr(C)]
pub struct TileComp {
    parent_tile: *mut Tile,
    res: *mut Resolution,
    comp_rect: Rect,
    comp_downsamp: Point,
    num_decomps: Ui32,
    comp_num: Ui32,
    num_bytes: Ui32,
}

impl Default for TileComp {
    fn default() -> Self {
        Self {
            parent_tile: ptr::null_mut(),
            res: ptr::null_mut(),
            comp_rect: Rect::default(),
            comp_downsamp: Point::default(),
            num_decomps: 0,
            comp_num: 0,
            num_bytes: 0,
        }
    }
}

impl TileComp {
    /// Parse-only enumeration (additive front-end for `crate::parse::enumerate`): forward the
    /// walk to this component's TOP [`Resolution`], which recurses down to res 0. `comp_num`
    /// is this component's image index, carried into each block's geometry.
    ///
    /// # Safety
    /// The component tree must be fully parsed (`read_headers` + `create`); guaranteed by the
    /// caller (`Tile::enumerate_blocks`).
    pub(crate) unsafe fn enumerate_blocks(
        &self,
        tile_idx: u32,
        sink: &mut dyn FnMut(crate::parse::CodeblockPlan),
    ) {
        (*self.res).enumerate_blocks(self.comp_num, tile_idx, sink);
    }

    pub fn pre_alloc(
        codestream: &mut LocalCodestream,
        comp_num: Ui32,
        comp_rect: Rect,
        recon_comp_rect: Rect,
    ) -> OjphResult<()> {
        let allocator = unsafe { &mut *codestream.get_allocator() };
        let num_decomps =
            unsafe { (*codestream.get_coc(comp_num)).get_num_decompositions() as Ui32 };
        allocator.pre_alloc_obj::<Resolution>(1);
        Resolution::pre_alloc(
            codestream,
            comp_rect,
            recon_comp_rect,
            comp_num,
            num_decomps,
        )
    }

    pub fn finalize_alloc(
        &mut self,
        codestream: &mut LocalCodestream,
        parent: *mut Tile,
        comp_num: Ui32,
        comp_rect: Rect,
        recon_comp_rect: Rect,
    ) -> OjphResult<()> {
        let allocator = unsafe { &mut *codestream.get_allocator() };
        self.num_decomps =
            unsafe { (*codestream.get_coc(comp_num)).get_num_decompositions() as Ui32 };
        self.comp_downsamp = codestream.get_siz().get_downsampling(comp_num);
        self.comp_rect = comp_rect;
        self.parent_tile = parent;
        self.comp_num = comp_num;
        self.num_bytes = 0;
        self.res = allocator.post_alloc_obj::<Resolution>(1);
        unsafe {
            ptr::write(self.res, Resolution::default());
            (*self.res).finalize_alloc(
                codestream,
                comp_rect,
                recon_comp_rect,
                comp_num,
                self.num_decomps,
                self.comp_downsamp,
                self.comp_downsamp,
                self,
                ptr::null_mut(),
            )?;
        }
        Ok(())
    }

    /// Apply an in-tile QCD/QCC override to this component's resolution pyramid. See
    /// [`crate::codestream::ojph_subband::Subband::requantize`].
    pub fn requantize(&mut self, codestream: &LocalCodestream, qcd: &ParamQcd) -> OjphResult<()> {
        if !self.res.is_null() {
            unsafe {
                (*self.res).requantize(codestream, qcd)?;
            }
        }
        Ok(())
    }

    pub const fn get_num_resolutions(&self) -> Ui32 {
        self.num_decomps + 1
    }
    pub const fn get_num_decompositions(&self) -> Ui32 {
        self.num_decomps
    }
    pub const fn get_tile(&self) -> *mut Tile {
        self.parent_tile
    }
    pub fn get_line(&mut self) -> *mut LineBuf {
        unsafe { (*self.res).get_line() }
    }
    pub fn push_line(&mut self) -> OjphResult<()> {
        unsafe { (*self.res).push_line() }
    }
    pub fn pull_line(&mut self) -> OjphResult<*mut LineBuf> {
        unsafe { (*self.res).pull_line() }
    }
    pub fn prepare_precincts(&mut self) -> Ui32 {
        self.num_bytes = unsafe { (*self.res).prepare_precinct() };
        self.num_bytes
    }

    /// Resolution object for absolute resolution level `res_num` (0 = lowest/LL, `num_decomps` =
    /// full). Returns null when `res_num` exceeds this component's decomposition count (a component
    /// with fewer resolutions than the image maximum has no packets at the high levels). The single
    /// resolution lookup for every precinct read/write path below and for the POC iterator.
    pub fn get_res(&mut self, res_num: Ui32) -> *mut Resolution {
        let Some(mut steps) = self.num_decomps.checked_sub(res_num) else {
            return ptr::null_mut();
        };
        let mut r = self.res;
        while steps > 0 && !r.is_null() {
            unsafe {
                r = (*r).next_resolution();
            }
            steps -= 1;
        }
        r
    }

    /// Run `f` on resolution `res_num`, or do nothing (`Ok`) when this component has no such level.
    fn with_res<T: Default>(
        &mut self,
        res_num: Ui32,
        f: impl FnOnce(&mut Resolution) -> OjphResult<T>,
    ) -> OjphResult<T> {
        let r = self.get_res(res_num);
        if r.is_null() { Ok(T::default()) } else { f(unsafe { &mut *r }) }
    }

    pub fn write_precincts(&mut self, res_num: Ui32, file: &mut dyn OutfileBase) -> OjphResult<()> {
        self.with_res(res_num, |r| r.write_precincts(file))
    }

    pub fn get_top_left_precinct(&mut self, res_num: Ui32, top_left: &mut Point) -> bool {
        let r = self.get_res(res_num);
        !r.is_null() && unsafe { (*r).get_top_left_precinct(top_left) }
    }

    pub fn write_one_precinct(
        &mut self,
        res_num: Ui32,
        file: &mut dyn OutfileBase,
    ) -> OjphResult<()> {
        self.with_res(res_num, |r| r.write_one_precinct(file))
    }

    pub fn parse_precincts(
        &mut self,
        res_num: Ui32,
        data_left: &mut Ui32,
        file: &mut dyn InfileBase,
        phdr: &mut Option<crate::codestream::ojph_packet_ml::PacketHeaderReader>,
    ) -> OjphResult<()> {
        self.with_res(res_num, |r| r.parse_all_precincts(data_left, file, phdr))
    }

    pub fn parse_one_precinct(
        &mut self,
        res_num: Ui32,
        data_left: &mut Ui32,
        file: &mut dyn InfileBase,
        phdr: &mut Option<crate::codestream::ojph_packet_ml::PacketHeaderReader>,
    ) -> OjphResult<()> {
        self.with_res(res_num, |r| r.parse_one_precinct(data_left, file, phdr))
    }

    /// Multi-layer variant of [`Self::parse_precincts`] (all precincts of a resolution, one layer)
    /// for layer-outermost progression orders (LRCP/RLCP). `map` carries the persistent per-precinct
    /// state across the non-consecutive layer visits.
    #[allow(clippy::too_many_arguments)]
    pub fn parse_precincts_ml(
        &mut self,
        res_num: Ui32,
        layer_idx: Ui16,
        ccap15: Ui16,
        is_last_layer: bool,
        map: &mut std::collections::HashMap<usize, crate::codestream::ojph_packet_ml::MlPrecinct>,
        data_left: &mut Ui32,
        file: &mut dyn InfileBase,
        phdr: &mut Option<crate::codestream::ojph_packet_ml::PacketHeaderReader>,
    ) -> OjphResult<()> {
        self.with_res(res_num, |r| {
            r.parse_all_precincts_ml(layer_idx, ccap15, is_last_layer, map, data_left, file, phdr)
        })
    }

    /// Multi-layer variant of [`Self::parse_one_precinct`] for layer-innermost progression orders.
    #[allow(clippy::too_many_arguments)]
    pub fn parse_one_precinct_ml(
        &mut self,
        res_num: Ui32,
        num_layers: Ui16,
        ccap15: Ui16,
        data_left: &mut Ui32,
        file: &mut dyn InfileBase,
        phdr: &mut Option<crate::codestream::ojph_packet_ml::PacketHeaderReader>,
    ) -> OjphResult<()> {
        self.with_res(res_num, |r| r.parse_one_precinct_ml(num_layers, ccap15, data_left, file, phdr))
    }

    pub const fn get_num_bytes(&self) -> Ui32 {
        self.num_bytes
    }
    pub fn get_num_bytes_for_resolution(&self, resolution_num: Ui32) -> Ui32 {
        unsafe { (*self.res).get_num_bytes_for_resolution(resolution_num) }
    }
}
