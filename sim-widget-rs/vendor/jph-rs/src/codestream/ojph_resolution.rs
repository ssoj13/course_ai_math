//! Port target:
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/codestream/ojph_resolution.h`
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/codestream/ojph_resolution.cpp`

use crate::codestream::ojph_codestream_local::LocalCodestream;
use crate::codestream::ojph_params_local::{DfsDwtType, ParamCod, ParamQcd};
use crate::codestream::ojph_precinct::Precinct;
use crate::codestream::ojph_subband::Subband;
use crate::codestream::ojph_tile_comp::TileComp;
use crate::openjph::ojph_base::{Point, Rect, Size};
use crate::openjph::ojph_defs::{Si32, Si64, Ui16, Ui32, ojph_max, ojph_min};
use crate::openjph::ojph_file::{InfileBase, OutfileBase};
use crate::openjph::ojph_mem::{
    LFT_32BIT, LFT_64BIT, LFT_SIZE_MASK, LiftingBuf, LineBuf, mem_elastic_allocator,
};
use crate::openjph::ojph_message::OjphResult;
use crate::transform::ojph_transform::{
    irv_horz_ana, irv_horz_syn, irv_vert_step, irv_vert_times_k, rev_horz_ana, rev_horz_syn,
    rev_vert_step,
};
use core::ptr;

unsafe fn copy_line_buf(dst: *mut LineBuf, src: *mut LineBuf, width: Ui32) {
    ptr::copy_nonoverlapping(
        (*src).storage.p as *const u8,
        (*dst).storage.p as *mut u8,
        width as usize * ((*dst).flags & LFT_SIZE_MASK) as usize,
    );
}

unsafe fn shl_line_buf(line: *mut LineBuf, width: Ui32) {
    if (*line).flags & LFT_32BIT != 0 {
        let mut sp = (*line).storage.i32_;
        for _ in 0..width {
            *sp <<= 1;
            sp = sp.add(1);
        }
    } else {
        debug_assert!((*line).flags & LFT_64BIT != 0);
        let mut sp = (*line).storage.i64_;
        for _ in 0..width {
            *sp <<= 1;
            sp = sp.add(1);
        }
    }
}

unsafe fn shr_line_buf(line: *mut LineBuf, width: Ui32) {
    if (*line).flags & LFT_32BIT != 0 {
        let mut sp = (*line).storage.i32_;
        for _ in 0..width {
            *sp >>= 1;
            sp = sp.add(1);
        }
    } else {
        debug_assert!((*line).flags & LFT_64BIT != 0);
        let mut sp = (*line).storage.i64_;
        for _ in 0..width {
            *sp >>= 1;
            sp = sp.add(1);
        }
    }
}

unsafe fn mul_line_buf_f32(line: *mut LineBuf, width: Ui32, k: f32) {
    let mut sp = (*line).storage.f32_;
    for _ in 0..width {
        *sp *= k;
        sp = sp.add(1);
    }
}

#[repr(C)]
pub struct Resolution {
    reversible: bool,
    skipped_res_for_read: bool,
    skipped_res_for_recon: bool,
    num_steps: Ui32,
    res_num: Ui32,
    comp_num: Ui32,
    num_bytes: Ui32,
    comp_downsamp: Point,
    res_rect: Rect,
    lines: *mut LineBuf,
    ssp: *mut LiftingBuf,
    aug: *mut LiftingBuf,
    sig: *mut LiftingBuf,
    bands: *mut Subband,
    parent_comp: *mut TileComp,
    parent_res: *mut Resolution,
    child_res: *mut Resolution,
    precincts: *mut Precinct,
    num_precincts: Size,
    log_pp: Size,
    max_num_levels: Ui32,
    tag_tree_size: i32,
    level_index: [Ui32; 20],
    cur_precinct_loc: Point,
    atk: *const crate::codestream::ojph_params_local::ParamAtk,
    transform_flags: Ui32,
    cur_line: Ui32,
    rows_to_produce: Ui32,
    vert_even: bool,
    horz_even: bool,
    elastic: *mut mem_elastic_allocator,
}

impl Default for Resolution {
    fn default() -> Self {
        Self {
            reversible: false,
            skipped_res_for_read: false,
            skipped_res_for_recon: false,
            num_steps: 0,
            res_num: 0,
            comp_num: 0,
            num_bytes: 0,
            comp_downsamp: Point::default(),
            res_rect: Rect::default(),
            lines: ptr::null_mut(),
            ssp: ptr::null_mut(),
            aug: ptr::null_mut(),
            sig: ptr::null_mut(),
            bands: ptr::null_mut(),
            parent_comp: ptr::null_mut(),
            parent_res: ptr::null_mut(),
            child_res: ptr::null_mut(),
            precincts: ptr::null_mut(),
            num_precincts: Size::default(),
            log_pp: Size::default(),
            max_num_levels: 0,
            tag_tree_size: 0,
            level_index: [0; 20],
            cur_precinct_loc: Point::default(),
            atk: ptr::null(),
            transform_flags: 0,
            cur_line: 0,
            rows_to_produce: 0,
            vert_even: false,
            horz_even: false,
            elastic: ptr::null_mut(),
        }
    }
}

impl Resolution {
    pub const HORZ_TRX: Ui32 = 0x01;
    pub const VERT_TRX: Ui32 = 0x02;

    pub fn pre_alloc(
        codestream: &mut LocalCodestream,
        res_rect: Rect,
        recon_res_rect: Rect,
        comp_num: Ui32,
        res_num: Ui32,
    ) -> OjphResult<()> {
        let allocator = unsafe { &mut *codestream.get_allocator() };
        let cdp = unsafe { &*codestream.get_coc(comp_num) };
        let num_decomps = cdp.get_num_decompositions() as Ui32;
        let t = num_decomps - codestream.get_skipped_res_for_recon();
        let skipped_res_for_recon = res_num > t;

        let atk = unsafe { &*cdp.access_atk() };
        let mut ds = DfsDwtType::BidirDwt;
        if cdp.is_dfs_defined() {
            let dfs = codestream.access_dfs_ptr();
            if dfs.is_null() {
                return Err(crate::ojph_error!(
                    0x00070001,
                    "There is a problem with codestream marker segments. COD/COC specifies the use of a DFS marker but there are no DFS markers within the main codestream headers"
                ));
            } else {
                let dfs_idx: Ui16 = cdp.get_dfs_index();
                let dfs = unsafe { (*dfs).get_dfs(dfs_idx as i32) };
                if let Some(dfs) = dfs {
                    ds = dfs.get_dwt_type(num_decomps - res_num + 1);
                } else {
                    return Err(crate::ojph_error!(
                        0x00070002,
                        "There is a problem with codestream marker segments. COD/COC specifies the use of a DFS marker with index {}, but there are no such marker within the main codestream headers",
                        dfs_idx
                    ));
                }
            }
        }

        let mut transform_flags = 0;
        if res_num > 0 {
            match ds {
                DfsDwtType::BidirDwt => transform_flags = Self::HORZ_TRX | Self::VERT_TRX,
                DfsDwtType::HorzDwt => transform_flags = Self::HORZ_TRX,
                DfsDwtType::VertDwt => transform_flags = Self::VERT_TRX,
                DfsDwtType::NoDwt => {}
            }
        }

        let trx0 = res_rect.org.x;
        let try0 = res_rect.org.y;
        let trx1 = res_rect.org.x + res_rect.siz.w;
        let try1 = res_rect.org.y + res_rect.siz.h;
        allocator.pre_alloc_obj::<Subband>(4);
        if res_num > 0 {
            match ds {
                DfsDwtType::BidirDwt => {
                    for i in 0..4u32 {
                        let tbx0 = (trx0.wrapping_sub(i & 1).wrapping_add(1)) >> 1;
                        let tbx1 = (trx1.wrapping_sub(i & 1).wrapping_add(1)) >> 1;
                        let tby0 = (try0.wrapping_sub(i >> 1).wrapping_add(1)) >> 1;
                        let tby1 = (try1.wrapping_sub(i >> 1).wrapping_add(1)) >> 1;
                        let re = Rect {
                            org: Point { x: tbx0, y: tby0 },
                            siz: Size {
                                w: tbx1 - tbx0,
                                h: tby1 - tby0,
                            },
                        };
                        if i == 0 {
                            allocator.pre_alloc_obj::<Resolution>(1);
                            Self::pre_alloc(
                                codestream,
                                re,
                                if skipped_res_for_recon {
                                    recon_res_rect
                                } else {
                                    re
                                },
                                comp_num,
                                res_num - 1,
                            )?;
                        } else {
                            Subband::pre_alloc(codestream, re, comp_num, res_num, transform_flags);
                        }
                    }
                }
                DfsDwtType::VertDwt => {
                    let mut re = res_rect;
                    let tby0 = (try0 + 1) >> 1;
                    let tby1 = (try1 + 1) >> 1;
                    re.org.y = tby0;
                    re.siz.h = tby1 - tby0;
                    allocator.pre_alloc_obj::<Resolution>(1);
                    Self::pre_alloc(
                        codestream,
                        re,
                        if skipped_res_for_recon {
                            recon_res_rect
                        } else {
                            re
                        },
                        comp_num,
                        res_num - 1,
                    )?;
                    re = res_rect;
                    let tby0 = try0 >> 1;
                    let tby1 = try1 >> 1;
                    re.org.y = tby0;
                    re.siz.h = tby1 - tby0;
                    Subband::pre_alloc(codestream, re, comp_num, res_num, transform_flags);
                }
                DfsDwtType::HorzDwt => {
                    let mut re = res_rect;
                    let tbx0 = (trx0 + 1) >> 1;
                    let tbx1 = (trx1 + 1) >> 1;
                    re.org.x = tbx0;
                    re.siz.w = tbx1 - tbx0;
                    allocator.pre_alloc_obj::<Resolution>(1);
                    Self::pre_alloc(
                        codestream,
                        re,
                        if skipped_res_for_recon {
                            recon_res_rect
                        } else {
                            re
                        },
                        comp_num,
                        res_num - 1,
                    )?;
                    re = res_rect;
                    let tbx0 = trx0 >> 1;
                    let tbx1 = trx1 >> 1;
                    re.org.x = tbx0;
                    re.siz.w = tbx1 - tbx0;
                    Subband::pre_alloc(codestream, re, comp_num, res_num, transform_flags);
                }
                DfsDwtType::NoDwt => {
                    allocator.pre_alloc_obj::<Resolution>(1);
                    Self::pre_alloc(
                        codestream,
                        res_rect,
                        if skipped_res_for_recon {
                            recon_res_rect
                        } else {
                            res_rect
                        },
                        comp_num,
                        res_num - 1,
                    )?;
                }
            }
        } else {
            Subband::pre_alloc(codestream, res_rect, comp_num, res_num, transform_flags);
        }

        let log_pp = cdp.get_log_precinct_size(res_num);
        if trx0 != trx1 && try0 != try1 {
            let num_precincts = Size {
                w: ((trx1 + (1 << log_pp.w) - 1) >> log_pp.w) - (trx0 >> log_pp.w),
                h: ((try1 + (1 << log_pp.h) - 1) >> log_pp.h) - (try0 >> log_pp.h),
            };
            allocator.pre_alloc_obj::<Precinct>(num_precincts.area() as usize);
        }

        if !skipped_res_for_recon {
            let num_steps = atk.get_num_steps();
            allocator.pre_alloc_obj::<LineBuf>((num_steps + 2) as usize);
            allocator.pre_alloc_obj::<LiftingBuf>((num_steps + 2) as usize);
            let qp = unsafe { &*codestream.access_qcd().get_qcc(comp_num) };
            let precision = qp.propose_precision(cdp as *const ParamCod);
            let reversible = atk.is_reversible();
            let width = (res_rect.siz.w + 1) as usize;
            if reversible {
                if precision <= 32 {
                    for _ in 0..num_steps {
                        allocator.pre_alloc_data::<Si32>(width, 1);
                    }
                    allocator.pre_alloc_data::<Si32>(width, 1);
                    allocator.pre_alloc_data::<Si32>(width, 1);
                } else {
                    for _ in 0..num_steps {
                        allocator.pre_alloc_data::<Si64>(width, 1);
                    }
                    allocator.pre_alloc_data::<Si64>(width, 1);
                    allocator.pre_alloc_data::<Si64>(width, 1);
                }
            } else {
                for _ in 0..num_steps {
                    allocator.pre_alloc_data::<f32>(width, 1);
                }
                allocator.pre_alloc_data::<f32>(width, 1);
                allocator.pre_alloc_data::<f32>(width, 1);
            }
        }
        Ok(())
    }

    pub fn finalize_alloc(
        &mut self,
        codestream: &mut LocalCodestream,
        res_rect: Rect,
        recon_res_rect: Rect,
        comp_num: Ui32,
        res_num: Ui32,
        comp_downsamp: Point,
        res_downsamp: Point,
        parent_tile_comp: *mut TileComp,
        parent_res: *mut Resolution,
    ) -> OjphResult<()> {
        let allocator = unsafe { &mut *codestream.get_allocator() };
        let cdp = unsafe { &*codestream.get_coc(comp_num) };
        self.res_rect = res_rect;
        self.comp_num = comp_num;
        self.res_num = res_num;
        self.comp_downsamp = comp_downsamp;
        self.parent_comp = parent_tile_comp;
        self.parent_res = parent_res;
        self.elastic = codestream.get_elastic_alloc();
        self.atk = cdp.access_atk();
        self.child_res = ptr::null_mut();
        self.precincts = ptr::null_mut();
        self.lines = ptr::null_mut();
        self.ssp = ptr::null_mut();
        self.sig = ptr::null_mut();
        self.aug = ptr::null_mut();

        let num_decomps = cdp.get_num_decompositions() as Ui32;
        let mut t = num_decomps - codestream.get_skipped_res_for_recon();
        self.skipped_res_for_recon = res_num > t;
        t = num_decomps - codestream.get_skipped_res_for_read();
        self.skipped_res_for_read = res_num > t;

        // Determine DWT type from DFS marker if present (same logic as pre_alloc)
        let mut ds = DfsDwtType::BidirDwt;
        if cdp.is_dfs_defined() {
            let dfs_ptr = codestream.access_dfs_ptr();
            if dfs_ptr.is_null() {
                return Err(crate::ojph_error!(
                    0x00070011,
                    "There is a problem with codestream marker segments. COD/COC specifies the use of a DFS marker but there are no DFS markers within the main codestream headers"
                ));
            } else {
                let dfs_idx: Ui16 = cdp.get_dfs_index();
                let dfs = unsafe { (*dfs_ptr).get_dfs(dfs_idx as i32) };
                if let Some(dfs) = dfs {
                    ds = dfs.get_dwt_type(num_decomps - res_num + 1);
                } else {
                    return Err(crate::ojph_error!(
                        0x00070012,
                        "There is a problem with codestream marker segments. COD/COC specifies the use of a DFS marker with index {}, but there are no such marker within the main codestream headers",
                        dfs_idx
                    ));
                }
            }
        }

        self.transform_flags = 0;
        if res_num > 0 {
            match ds {
                DfsDwtType::BidirDwt => self.transform_flags = Self::HORZ_TRX | Self::VERT_TRX,
                DfsDwtType::HorzDwt => self.transform_flags = Self::HORZ_TRX,
                DfsDwtType::VertDwt => self.transform_flags = Self::VERT_TRX,
                DfsDwtType::NoDwt => {}
            }
        }

        let trx0 = res_rect.org.x;
        let try0 = res_rect.org.y;
        let trx1 = res_rect.org.x + res_rect.siz.w;
        let try1 = res_rect.org.y + res_rect.siz.h;

        self.bands = allocator.post_alloc_obj::<Subband>(4);
        for i in 0..4usize {
            unsafe {
                ptr::write(self.bands.add(i), Subband::default());
            }
        }

        if res_num > 0 {
            match ds {
                DfsDwtType::BidirDwt => {
                    for i in 0..4u32 {
                        let tbx0 = (trx0.wrapping_sub(i & 1).wrapping_add(1)) >> 1;
                        let tbx1 = (trx1.wrapping_sub(i & 1).wrapping_add(1)) >> 1;
                        let tby0 = (try0.wrapping_sub(i >> 1).wrapping_add(1)) >> 1;
                        let tby1 = (try1.wrapping_sub(i >> 1).wrapping_add(1)) >> 1;
                        let re = Rect {
                            org: Point { x: tbx0, y: tby0 },
                            siz: Size {
                                w: tbx1 - tbx0,
                                h: tby1 - tby0,
                            },
                        };
                        if i == 0 {
                            let next_res_downsamp = Point {
                                x: res_downsamp.x * 2,
                                y: res_downsamp.y * 2,
                            };
                            unsafe {
                                self.child_res = allocator.post_alloc_obj::<Resolution>(1);
                                ptr::write(self.child_res, Resolution::default());
                                (*self.child_res).finalize_alloc(
                                    codestream,
                                    re,
                                    if self.skipped_res_for_recon {
                                        recon_res_rect
                                    } else {
                                        re
                                    },
                                    comp_num,
                                    res_num - 1,
                                    comp_downsamp,
                                    next_res_downsamp,
                                    parent_tile_comp,
                                    self,
                                )?;
                            }
                        } else {
                            unsafe {
                                (*self.bands.add(i as usize))
                                    .finalize_alloc(codestream, re, self, res_num, i)?;
                            }
                        }
                    }
                }
                DfsDwtType::VertDwt => {
                    // Child resolution: vertically downsampled LL subband
                    let mut re = res_rect;
                    let tby0 = (try0 + 1) >> 1;
                    let tby1 = (try1 + 1) >> 1;
                    re.org.y = tby0;
                    re.siz.h = tby1 - tby0;
                    let next_res_downsamp = Point {
                        x: res_downsamp.x,
                        y: res_downsamp.y * 2,
                    };
                    unsafe {
                        self.child_res = allocator.post_alloc_obj::<Resolution>(1);
                        ptr::write(self.child_res, Resolution::default());
                        (*self.child_res).finalize_alloc(
                            codestream,
                            re,
                            if self.skipped_res_for_recon {
                                recon_res_rect
                            } else {
                                re
                            },
                            comp_num,
                            res_num - 1,
                            comp_downsamp,
                            next_res_downsamp,
                            parent_tile_comp,
                            self,
                        )?;
                    }
                    // Band 2: vertically high-pass (LH in vertical-only DWT)
                    re = res_rect;
                    let tby0 = try0 >> 1;
                    let tby1 = try1 >> 1;
                    re.org.y = tby0;
                    re.siz.h = tby1 - tby0;
                    unsafe {
                        (*self.bands.add(2)).finalize_alloc(codestream, re, self, res_num, 2)?;
                    }
                }
                DfsDwtType::HorzDwt => {
                    // Child resolution: horizontally downsampled LL subband
                    let mut re = res_rect;
                    let tbx0 = (trx0 + 1) >> 1;
                    let tbx1 = (trx1 + 1) >> 1;
                    re.org.x = tbx0;
                    re.siz.w = tbx1 - tbx0;
                    let next_res_downsamp = Point {
                        x: res_downsamp.x * 2,
                        y: res_downsamp.y,
                    };
                    unsafe {
                        self.child_res = allocator.post_alloc_obj::<Resolution>(1);
                        ptr::write(self.child_res, Resolution::default());
                        (*self.child_res).finalize_alloc(
                            codestream,
                            re,
                            if self.skipped_res_for_recon {
                                recon_res_rect
                            } else {
                                re
                            },
                            comp_num,
                            res_num - 1,
                            comp_downsamp,
                            next_res_downsamp,
                            parent_tile_comp,
                            self,
                        )?;
                    }
                    // Band 1: horizontally high-pass (HL in horizontal-only DWT)
                    re = res_rect;
                    let tbx0 = trx0 >> 1;
                    let tbx1 = trx1 >> 1;
                    re.org.x = tbx0;
                    re.siz.w = tbx1 - tbx0;
                    unsafe {
                        (*self.bands.add(1)).finalize_alloc(codestream, re, self, res_num, 1)?;
                    }
                }
                DfsDwtType::NoDwt => {
                    // No transform: child resolution covers same rect
                    unsafe {
                        self.child_res = allocator.post_alloc_obj::<Resolution>(1);
                        ptr::write(self.child_res, Resolution::default());
                        (*self.child_res).finalize_alloc(
                            codestream,
                            res_rect,
                            if self.skipped_res_for_recon {
                                recon_res_rect
                            } else {
                                res_rect
                            },
                            comp_num,
                            res_num - 1,
                            comp_downsamp,
                            res_downsamp,
                            parent_tile_comp,
                            self,
                        )?;
                    }
                }
            }
        } else {
            // res_num == 0: leaf resolution, no child, finalize LL subband only
            self.child_res = ptr::null_mut();
            unsafe {
                (*self.bands).finalize_alloc(codestream, res_rect, self, res_num, 0)?;
            }
        }

        self.log_pp = cdp.get_log_precinct_size(res_num);
        self.num_precincts = Size::default();
        if trx0 != trx1 && try0 != try1 {
            self.num_precincts.w =
                ((trx1 + (1 << self.log_pp.w) - 1) >> self.log_pp.w) - (trx0 >> self.log_pp.w);
            self.num_precincts.h =
                ((try1 + (1 << self.log_pp.h) - 1) >> self.log_pp.h) - (try0 >> self.log_pp.h);
            self.precincts =
                allocator.post_alloc_obj::<Precinct>(self.num_precincts.area() as usize);
            for i in 0..self.num_precincts.area() as usize {
                unsafe {
                    ptr::write(self.precincts.add(i), Precinct::default());
                }
            }
        }

        // Initialize each precinct with geometry and band pointer.
        // x/y_lower_bound is the top-left corner of the first precinct grid cell
        // aligned to precinct-size boundaries.
        let x_lower_bound = (trx0 >> self.log_pp.w) << self.log_pp.w;
        let y_lower_bound = (try0 >> self.log_pp.h) << self.log_pp.h;
        let tile_top_left = unsafe { (*(*parent_tile_comp).get_tile()).get_tile_rect().org };
        let mut pp = self.precincts;
        for y in 0..self.num_precincts.h {
            let ppy0 = y_lower_bound + (y << self.log_pp.h);
            for x in 0..self.num_precincts.w {
                let ppx0 = x_lower_bound + (x << self.log_pp.w);
                let t = Point {
                    x: ojph_max(res_downsamp.x * ppx0, tile_top_left.x),
                    y: ojph_max(res_downsamp.y * ppy0, tile_top_left.y),
                };
                unsafe {
                    (*pp).img_point = t;
                    (*pp).bands = self.bands;
                    // SOP/EPH are signalled globally in the COD (Scod bits 1/2), never in a COC
                    // (whose Scoc only carries the precinct-defined bit). Reading them from `cdp`
                    // returns false for any component that has a COC, so the SOP marker is not
                    // skipped and the packet header desyncs. Always take them from the main COD.
                    let gcod = codestream.get_cod();
                    (*pp).may_use_sop = gcod.packets_may_use_sop();
                    (*pp).uses_eph = gcod.packets_use_eph();
                    // HT vs Part-1 is per-component (a COC can override the main COD), so take it
                    // from this component's coding params, not the global COD.
                    (*pp).is_ht = cdp.is_ht();
                    // Part-1 coding modes (low 6 bits of the code-block style) drive the per-pass
                    // RESTART segment-length model in the packet parser; taken per-component too.
                    (*pp).cmodes = crate::coding::ojph_cmodes::part1(cdp.get_block_style());
                    (*pp).scratch = codestream.get_precinct_scratch();
                    (*pp).coded = ptr::null_mut();
                    pp = pp.add(1);
                }
            }
        }

        // Fill each precinct's codeblock index ranges per band.
        for i in 0..4usize {
            unsafe {
                if (*self.bands.add(i)).exists() {
                    (*self.bands.add(i)).get_cb_indices(self.num_precincts, self.precincts);
                }
            }
        }

        // Compute tag-tree level structure.
        // log_PP is adjusted for the number of transform dimensions to get the
        // ratio of precinct size to codeblock size in each axis, then the
        // larger of the two ratios determines how many tag-tree levels are needed.
        let log_cb = cdp.get_log_block_dims();
        let log_pp_w = self.log_pp.w
            - if self.transform_flags & Self::HORZ_TRX != 0 {
                1
            } else {
                0
            };
        let log_pp_h = self.log_pp.h
            - if self.transform_flags & Self::VERT_TRX != 0 {
                1
            } else {
                0
            };
        let ratio_w = log_pp_w - ojph_min(log_cb.w, log_pp_w);
        let ratio_h = log_pp_h - ojph_min(log_cb.h, log_pp_h);
        self.max_num_levels = ojph_max(ratio_w, ratio_h);
        let mut val = 1u32 << (self.max_num_levels << 1);
        // tag_tree_size = sum of geometric series: 4^L + 4^(L-1) + ... + 1 = (4^(L+1) - 1) / 3
        self.tag_tree_size = ((val * 4 + 2) / 3) as i32;
        self.max_num_levels += 1;
        self.level_index[0] = 0;
        for i in 1..=self.max_num_levels as usize {
            self.level_index[i] = self.level_index[i - 1] + val;
            val >>= 2;
        }
        self.cur_precinct_loc = Point { x: 0, y: 0 };

        if !self.skipped_res_for_recon {
            let atk = unsafe { &*self.atk };
            self.reversible = atk.is_reversible();
            self.num_steps = atk.get_num_steps();
            self.lines = allocator.post_alloc_obj::<LineBuf>((self.num_steps + 2) as usize);
            self.ssp = allocator.post_alloc_obj::<LiftingBuf>((self.num_steps + 2) as usize);
            self.sig = unsafe { self.ssp.add(self.num_steps as usize) };
            self.aug = unsafe { self.ssp.add(self.num_steps as usize + 1) };

            for i in 0..self.num_steps as usize {
                unsafe {
                    ptr::write(self.lines.add(i), LineBuf::default());
                    ptr::write(self.ssp.add(i), LiftingBuf::default());
                    (*self.ssp.add(i)).line = self.lines.add(i);
                }
            }
            unsafe {
                ptr::write(self.lines.add(self.num_steps as usize), LineBuf::default());
                ptr::write(
                    self.lines.add(self.num_steps as usize + 1),
                    LineBuf::default(),
                );
                ptr::write(self.sig, LiftingBuf::default());
                ptr::write(self.aug, LiftingBuf::default());
                (*self.sig).line = self.lines.add(self.num_steps as usize);
                (*self.aug).line = self.lines.add(self.num_steps as usize + 1);
            }

            let qp = unsafe { &*codestream.access_qcd().get_qcc(comp_num) };
            let precision = qp.propose_precision(cdp as *const ParamCod);
            let width = (res_rect.siz.w + 1) as usize;
            if self.reversible {
                if precision <= 32 {
                    for i in 0..self.num_steps as usize {
                        unsafe {
                            (*(*self.ssp.add(i)).line).wrap_i32(
                                allocator.post_alloc_data::<Si32>(width, 1),
                                width,
                                1,
                            );
                        }
                    }
                    unsafe {
                        (*(*self.sig).line).wrap_i32(
                            allocator.post_alloc_data::<Si32>(width, 1),
                            width,
                            1,
                        );
                        (*(*self.aug).line).wrap_i32(
                            allocator.post_alloc_data::<Si32>(width, 1),
                            width,
                            1,
                        );
                    }
                } else {
                    for i in 0..self.num_steps as usize {
                        unsafe {
                            (*(*self.ssp.add(i)).line).wrap_i64(
                                allocator.post_alloc_data::<Si64>(width, 1),
                                width,
                                1,
                            );
                        }
                    }
                    unsafe {
                        (*(*self.sig).line).wrap_i64(
                            allocator.post_alloc_data::<Si64>(width, 1),
                            width,
                            1,
                        );
                        (*(*self.aug).line).wrap_i64(
                            allocator.post_alloc_data::<Si64>(width, 1),
                            width,
                            1,
                        );
                    }
                }
            } else {
                for i in 0..self.num_steps as usize {
                    unsafe {
                        (*(*self.ssp.add(i)).line).wrap_f32(
                            allocator.post_alloc_data::<f32>(width, 1),
                            width,
                            1,
                        );
                    }
                }
                unsafe {
                    (*(*self.sig).line).wrap_f32(
                        allocator.post_alloc_data::<f32>(width, 1),
                        width,
                        1,
                    );
                    (*(*self.aug).line).wrap_f32(
                        allocator.post_alloc_data::<f32>(width, 1),
                        width,
                        1,
                    );
                }
            }

            self.cur_line = 0;
            self.rows_to_produce = res_rect.siz.h;
            self.vert_even = (res_rect.org.y & 1) == 0;
            self.horz_even = (res_rect.org.x & 1) == 0;
        }
        Ok(())
    }

    pub fn get_line(&mut self) -> *mut LineBuf {
        if self.vert_even {
            self.cur_line += 1;
            unsafe {
                (*self.sig).active = true;
                (*self.sig).line
            }
        } else {
            self.cur_line += 1;
            unsafe {
                (*self.aug).active = true;
                (*self.aug).line
            }
        }
    }

    pub fn push_line(&mut self) -> OjphResult<()> {
        if self.res_num == 0 {
            debug_assert!(self.child_res.is_null());
            unsafe {
                (*self.bands).exchange_buf(if self.vert_even {
                    (*self.sig).line
                } else {
                    (*self.aug).line
                });
                (*self.bands).push_line()?;
            }
            return Ok(());
        }

        let width = self.res_rect.siz.w;
        if width == 0 {
            return Ok(());
        }

        unsafe {
            if self.reversible {
                if self.res_rect.siz.h > 1 {
                    if !self.vert_even && self.cur_line < self.res_rect.siz.h {
                        self.vert_even = !self.vert_even;
                        return Ok(());
                    }

                    loop {
                        for i in 0..self.num_steps {
                            if (*self.aug).active
                                && ((*self.sig).active || (*self.ssp.add(i as usize)).active)
                            {
                                let dp = (*self.aug).line;
                                let sp1 = if (*self.sig).active {
                                    (*self.sig).line
                                } else {
                                    (*self.ssp.add(i as usize)).line
                                };
                                let sp2 = if (*self.ssp.add(i as usize)).active {
                                    (*self.ssp.add(i as usize)).line
                                } else {
                                    (*self.sig).line
                                };
                                let s = (*self.atk).get_step(self.num_steps - i - 1);
                                rev_vert_step(s, sp1, sp2, dp, width, false);
                            }
                            let t = *self.aug;
                            *self.aug = *self.ssp.add(i as usize);
                            *self.ssp.add(i as usize) = *self.sig;
                            *self.sig = t;
                        }

                        if (*self.aug).active {
                            rev_horz_ana(
                                self.atk,
                                (*self.bands.add(2)).get_line(),
                                (*self.bands.add(3)).get_line(),
                                (*self.aug).line,
                                width,
                                self.horz_even,
                            );
                            (*self.bands.add(2)).push_line()?;
                            (*self.bands.add(3)).push_line()?;
                            (*self.aug).active = false;
                            self.rows_to_produce -= 1;
                        }
                        if (*self.sig).active {
                            rev_horz_ana(
                                self.atk,
                                (*self.child_res).get_line(),
                                (*self.bands.add(1)).get_line(),
                                (*self.sig).line,
                                width,
                                self.horz_even,
                            );
                            (*self.bands.add(1)).push_line()?;
                            (*self.child_res).push_line()?;
                            (*self.sig).active = false;
                            self.rows_to_produce -= 1;
                        }
                        self.vert_even = !self.vert_even;
                        if !(self.cur_line >= self.res_rect.siz.h && self.rows_to_produce > 0) {
                            break;
                        }
                    }
                } else if self.vert_even {
                    rev_horz_ana(
                        self.atk,
                        (*self.child_res).get_line(),
                        (*self.bands.add(1)).get_line(),
                        (*self.sig).line,
                        width,
                        self.horz_even,
                    );
                    (*self.bands.add(1)).push_line()?;
                    (*self.child_res).push_line()?;
                } else {
                    shl_line_buf((*self.aug).line, width);
                    rev_horz_ana(
                        self.atk,
                        (*self.bands.add(2)).get_line(),
                        (*self.bands.add(3)).get_line(),
                        (*self.aug).line,
                        width,
                        self.horz_even,
                    );
                    (*self.bands.add(2)).push_line()?;
                    (*self.bands.add(3)).push_line()?;
                }
            } else if self.res_rect.siz.h > 1 {
                if !self.vert_even && self.cur_line < self.res_rect.siz.h {
                    self.vert_even = !self.vert_even;
                    return Ok(());
                }

                loop {
                    for i in 0..self.num_steps {
                        if (*self.aug).active
                            && ((*self.sig).active || (*self.ssp.add(i as usize)).active)
                        {
                            let dp = (*self.aug).line;
                            let sp1 = if (*self.sig).active {
                                (*self.sig).line
                            } else {
                                (*self.ssp.add(i as usize)).line
                            };
                            let sp2 = if (*self.ssp.add(i as usize)).active {
                                (*self.ssp.add(i as usize)).line
                            } else {
                                (*self.sig).line
                            };
                            let s = (*self.atk).get_step(self.num_steps - i - 1);
                            irv_vert_step(s, sp1, sp2, dp, width, false);
                        }
                        let t = *self.aug;
                        *self.aug = *self.ssp.add(i as usize);
                        *self.ssp.add(i as usize) = *self.sig;
                        *self.sig = t;
                    }

                    if (*self.aug).active {
                        let k = (*self.atk).get_k();
                        irv_vert_times_k(k, (*self.aug).line, width);
                        irv_horz_ana(
                            self.atk,
                            (*self.bands.add(2)).get_line(),
                            (*self.bands.add(3)).get_line(),
                            (*self.aug).line,
                            width,
                            self.horz_even,
                        );
                        (*self.bands.add(2)).push_line()?;
                        (*self.bands.add(3)).push_line()?;
                        (*self.aug).active = false;
                        self.rows_to_produce -= 1;
                    }
                    if (*self.sig).active {
                        let k_inv = 1.0f32 / (*self.atk).get_k();
                        irv_vert_times_k(k_inv, (*self.sig).line, width);
                        irv_horz_ana(
                            self.atk,
                            (*self.child_res).get_line(),
                            (*self.bands.add(1)).get_line(),
                            (*self.sig).line,
                            width,
                            self.horz_even,
                        );
                        (*self.bands.add(1)).push_line()?;
                        (*self.child_res).push_line()?;
                        (*self.sig).active = false;
                        self.rows_to_produce -= 1;
                    }
                    self.vert_even = !self.vert_even;
                    if !(self.cur_line >= self.res_rect.siz.h && self.rows_to_produce > 0) {
                        break;
                    }
                }
            } else if self.vert_even {
                irv_horz_ana(
                    self.atk,
                    (*self.child_res).get_line(),
                    (*self.bands.add(1)).get_line(),
                    (*self.sig).line,
                    width,
                    self.horz_even,
                );
                (*self.bands.add(1)).push_line()?;
                (*self.child_res).push_line()?;
            } else {
                mul_line_buf_f32((*self.aug).line, width, 2.0f32);
                irv_horz_ana(
                    self.atk,
                    (*self.bands.add(2)).get_line(),
                    (*self.bands.add(3)).get_line(),
                    (*self.aug).line,
                    width,
                    self.horz_even,
                );
                (*self.bands.add(2)).push_line()?;
                (*self.bands.add(3)).push_line()?;
            }
        }
        Ok(())
    }

    /// Parse-only enumeration (additive front-end for `crate::parse::enumerate`): walk this
    /// resolution's code-blocks, **without** entropy decode or DWT synthesis. The band set and
    /// recursion mirror the inverse-DWT pull in [`Self::pull_line`] exactly: res 0 has only the
    /// LL band (`bands[0]`) and a null `child_res`; res > 0 recurses into `child_res` (the lower
    /// resolution) and then enumerates HL/LH/HH (`bands[1..=3]`), never touching `bands[0]`.
    /// Empty bands are skipped inside [`Subband::enumerate_blocks`].
    ///
    /// # Safety
    /// The resolution tree must be fully parsed (`read_headers` + `create`). The caller chain
    /// (`TileComp`/`Tile`/`LocalCodestream::enumerate_blocks`) guarantees this.
    pub(crate) unsafe fn enumerate_blocks(
        &self,
        comp_num: Ui32,
        tile_idx: u32,
        sink: &mut dyn FnMut(crate::parse::CodeblockPlan),
    ) {
        if self.res_num == 0 {
            (*self.bands).enumerate_blocks(comp_num, tile_idx, sink);
        } else {
            if !self.child_res.is_null() {
                (*self.child_res).enumerate_blocks(comp_num, tile_idx, sink);
            }
            for b in 1..4usize {
                (*self.bands.add(b)).enumerate_blocks(comp_num, tile_idx, sink);
            }
        }
    }

    pub fn pull_line(&mut self) -> OjphResult<*mut LineBuf> {
        if self.res_num == 0 {
            debug_assert!(self.child_res.is_null());
            return Ok(unsafe { (*self.bands).pull_line()? });
        }

        if self.skipped_res_for_recon {
            return Ok(unsafe { (*self.child_res).pull_line()? });
        }

        let width = self.res_rect.siz.w;
        if width == 0 {
            return Ok(ptr::null_mut());
        }

        unsafe {
            if (self.transform_flags & Self::VERT_TRX) != 0 {
                if self.reversible {
                    if self.res_rect.siz.h > 1 {
                        if (*self.sig).active {
                            (*self.sig).active = false;
                            return Ok((*self.sig).line);
                        }
                        loop {
                            if self.cur_line < self.res_rect.siz.h {
                                if self.vert_even {
                                    if (self.transform_flags & Self::HORZ_TRX) != 0 {
                                        rev_horz_syn(
                                            self.atk,
                                            (*self.aug).line,
                                            (*self.child_res).pull_line()?,
                                            (*self.bands.add(1)).pull_line()?,
                                            width,
                                            self.horz_even,
                                        );
                                    } else {
                                        copy_line_buf(
                                            (*self.aug).line,
                                            (*self.child_res).pull_line()?,
                                            width,
                                        );
                                    }
                                    (*self.aug).active = true;
                                    self.vert_even = !self.vert_even;
                                    self.cur_line += 1;
                                    continue;
                                } else {
                                    if (self.transform_flags & Self::HORZ_TRX) != 0 {
                                        rev_horz_syn(
                                            self.atk,
                                            (*self.sig).line,
                                            (*self.bands.add(2)).pull_line()?,
                                            (*self.bands.add(3)).pull_line()?,
                                            width,
                                            self.horz_even,
                                        );
                                    } else {
                                        copy_line_buf(
                                            (*self.sig).line,
                                            (*self.bands.add(2)).pull_line()?,
                                            width,
                                        );
                                    }
                                    (*self.sig).active = true;
                                    self.vert_even = !self.vert_even;
                                    self.cur_line += 1;
                                }
                            }

                            for i in 0..self.num_steps {
                                if (*self.aug).active
                                    && ((*self.sig).active || (*self.ssp.add(i as usize)).active)
                                {
                                    let dp = (*self.aug).line;
                                    let sp1 = if (*self.sig).active {
                                        (*self.sig).line
                                    } else {
                                        (*self.ssp.add(i as usize)).line
                                    };
                                    let sp2 = if (*self.ssp.add(i as usize)).active {
                                        (*self.ssp.add(i as usize)).line
                                    } else {
                                        (*self.sig).line
                                    };
                                    let s = (*self.atk).get_step(i);
                                    rev_vert_step(s, sp1, sp2, dp, width, true);
                                }
                                let t = *self.aug;
                                *self.aug = *self.ssp.add(i as usize);
                                *self.ssp.add(i as usize) = *self.sig;
                                *self.sig = t;
                            }

                            if (*self.aug).active {
                                (*self.aug).active = false;
                                return Ok((*self.aug).line);
                            }
                            if (*self.sig).active {
                                (*self.sig).active = false;
                                return Ok((*self.sig).line);
                            }
                        }
                    } else {
                        if self.vert_even {
                            if (self.transform_flags & Self::HORZ_TRX) != 0 {
                                rev_horz_syn(
                                    self.atk,
                                    (*self.aug).line,
                                    (*self.child_res).pull_line()?,
                                    (*self.bands.add(1)).pull_line()?,
                                    width,
                                    self.horz_even,
                                );
                            } else {
                                copy_line_buf(
                                    (*self.aug).line,
                                    (*self.child_res).pull_line()?,
                                    width,
                                );
                            }
                        } else {
                            if (self.transform_flags & Self::HORZ_TRX) != 0 {
                                rev_horz_syn(
                                    self.atk,
                                    (*self.aug).line,
                                    (*self.bands.add(2)).pull_line()?,
                                    (*self.bands.add(3)).pull_line()?,
                                    width,
                                    self.horz_even,
                                );
                            } else {
                                copy_line_buf(
                                    (*self.aug).line,
                                    (*self.bands.add(2)).pull_line()?,
                                    width,
                                );
                            }
                            shr_line_buf((*self.aug).line, width);
                        }
                        return Ok((*self.aug).line);
                    }
                } else if self.res_rect.siz.h > 1 {
                    if (*self.sig).active {
                        (*self.sig).active = false;
                        return Ok((*self.sig).line);
                    }
                    loop {
                        if self.cur_line < self.res_rect.siz.h {
                            if self.vert_even {
                                if (self.transform_flags & Self::HORZ_TRX) != 0 {
                                    irv_horz_syn(
                                        self.atk,
                                        (*self.aug).line,
                                        (*self.child_res).pull_line()?,
                                        (*self.bands.add(1)).pull_line()?,
                                        width,
                                        self.horz_even,
                                    );
                                } else {
                                    copy_line_buf(
                                        (*self.aug).line,
                                        (*self.child_res).pull_line()?,
                                        width,
                                    );
                                }
                                (*self.aug).active = true;
                                self.vert_even = !self.vert_even;
                                self.cur_line += 1;
                                let k = (*self.atk).get_k();
                                irv_vert_times_k(k, (*self.aug).line, width);
                                continue;
                            } else {
                                if (self.transform_flags & Self::HORZ_TRX) != 0 {
                                    irv_horz_syn(
                                        self.atk,
                                        (*self.sig).line,
                                        (*self.bands.add(2)).pull_line()?,
                                        (*self.bands.add(3)).pull_line()?,
                                        width,
                                        self.horz_even,
                                    );
                                } else {
                                    copy_line_buf(
                                        (*self.sig).line,
                                        (*self.bands.add(2)).pull_line()?,
                                        width,
                                    );
                                }
                                (*self.sig).active = true;
                                self.vert_even = !self.vert_even;
                                self.cur_line += 1;
                                let k_inv = 1.0f32 / (*self.atk).get_k();
                                irv_vert_times_k(k_inv, (*self.sig).line, width);
                            }
                        }

                        for i in 0..self.num_steps {
                            if (*self.aug).active
                                && ((*self.sig).active || (*self.ssp.add(i as usize)).active)
                            {
                                let dp = (*self.aug).line;
                                let sp1 = if (*self.sig).active {
                                    (*self.sig).line
                                } else {
                                    (*self.ssp.add(i as usize)).line
                                };
                                let sp2 = if (*self.ssp.add(i as usize)).active {
                                    (*self.ssp.add(i as usize)).line
                                } else {
                                    (*self.sig).line
                                };
                                let s = (*self.atk).get_step(i);
                                irv_vert_step(s, sp1, sp2, dp, width, true);
                            }
                            let t = *self.aug;
                            *self.aug = *self.ssp.add(i as usize);
                            *self.ssp.add(i as usize) = *self.sig;
                            *self.sig = t;
                        }

                        if (*self.aug).active {
                            (*self.aug).active = false;
                            return Ok((*self.aug).line);
                        }
                        if (*self.sig).active {
                            (*self.sig).active = false;
                            return Ok((*self.sig).line);
                        }
                    }
                } else {
                    if self.vert_even {
                        if (self.transform_flags & Self::HORZ_TRX) != 0 {
                            irv_horz_syn(
                                self.atk,
                                (*self.aug).line,
                                (*self.child_res).pull_line()?,
                                (*self.bands.add(1)).pull_line()?,
                                width,
                                self.horz_even,
                            );
                        } else {
                            copy_line_buf((*self.aug).line, (*self.child_res).pull_line()?, width);
                        }
                    } else {
                        if (self.transform_flags & Self::HORZ_TRX) != 0 {
                            irv_horz_syn(
                                self.atk,
                                (*self.aug).line,
                                (*self.bands.add(2)).pull_line()?,
                                (*self.bands.add(3)).pull_line()?,
                                width,
                                self.horz_even,
                            );
                        } else {
                            copy_line_buf(
                                (*self.aug).line,
                                (*self.bands.add(2)).pull_line()?,
                                width,
                            );
                        }
                        mul_line_buf_f32((*self.aug).line, width, 0.5f32);
                    }
                    return Ok((*self.aug).line);
                }
            } else if self.reversible {
                if (self.transform_flags & Self::HORZ_TRX) != 0 {
                    rev_horz_syn(
                        self.atk,
                        (*self.aug).line,
                        (*self.child_res).pull_line()?,
                        (*self.bands.add(1)).pull_line()?,
                        width,
                        self.horz_even,
                    );
                } else {
                    copy_line_buf((*self.aug).line, (*self.child_res).pull_line()?, width);
                }
                Ok((*self.aug).line)
            } else {
                if (self.transform_flags & Self::HORZ_TRX) != 0 {
                    irv_horz_syn(
                        self.atk,
                        (*self.aug).line,
                        (*self.child_res).pull_line()?,
                        (*self.bands.add(1)).pull_line()?,
                        width,
                        self.horz_even,
                    );
                } else {
                    copy_line_buf((*self.aug).line, (*self.child_res).pull_line()?, width);
                }
                Ok((*self.aug).line)
            }
        }
    }

    pub const fn get_rect(&self) -> Rect {
        self.res_rect
    }
    pub const fn get_comp_num(&self) -> Ui32 {
        self.comp_num
    }
    /// ROI max-shift for this resolution's component, resolved from the owning tile (0 = none).
    /// Walks `parent_comp -> tile`; safe against null back-pointers.
    pub fn get_roishift(&self) -> u8 {
        if self.parent_comp.is_null() {
            return 0;
        }
        let tile = unsafe { (*self.parent_comp).get_tile() };
        if tile.is_null() {
            return 0;
        }
        unsafe { (*tile).get_roishift(self.comp_num) }
    }
    pub const fn has_horz_transform(&self) -> bool {
        (self.transform_flags & Self::HORZ_TRX) != 0
    }
    pub const fn has_vert_transform(&self) -> bool {
        (self.transform_flags & Self::VERT_TRX) != 0
    }
    pub fn prepare_precinct(&mut self) -> Ui32 {
        let mut lower_resolutions_bytes = 0;
        if self.res_num != 0 {
            lower_resolutions_bytes = unsafe { (*self.child_res).prepare_precinct() };
        }

        self.num_bytes = 0;
        for i in 0..self.num_precincts.area() {
            unsafe {
                self.num_bytes += (*self.precincts.add(i as usize)).prepare_precinct(
                    self.tag_tree_size,
                    self.level_index.as_mut_ptr(),
                    self.elastic,
                );
            }
        }
        self.num_bytes + lower_resolutions_bytes
    }
    pub fn write_precincts(&mut self, file: &mut dyn OutfileBase) -> OjphResult<()> {
        for i in 0..self.num_precincts.area() {
            unsafe {
                (*self.precincts.add(i as usize)).write(file)?;
            }
        }
        Ok(())
    }
    pub fn get_top_left_precinct(&mut self, top_left: &mut Point) -> bool {
        let idx = self.cur_precinct_loc.x + self.cur_precinct_loc.y * self.num_precincts.w;
        if (idx as u64) < self.num_precincts.area() {
            unsafe {
                *top_left = (*self.precincts.add(idx as usize)).img_point;
            }
            true
        } else {
            false
        }
    }
    pub fn write_one_precinct(&mut self, file: &mut dyn OutfileBase) -> OjphResult<()> {
        let idx = self.cur_precinct_loc.x + self.cur_precinct_loc.y * self.num_precincts.w;
        debug_assert!((idx as u64) < self.num_precincts.area());
        unsafe {
            (*self.precincts.add(idx as usize)).write(file)?;
        }
        self.cur_precinct_loc.x += 1;
        if self.cur_precinct_loc.x >= self.num_precincts.w {
            self.cur_precinct_loc.x = 0;
            self.cur_precinct_loc.y += 1;
        }
        Ok(())
    }
    pub const fn next_resolution(&self) -> *mut Resolution {
        self.child_res
    }
    /// Apply an in-tile QCD/QCC override to every subband of this resolution and, recursively, of
    /// the lower-resolution `child_res` chain (the LL pyramid). See [`Subband::requantize`].
    pub fn requantize(&mut self, codestream: &LocalCodestream, qcd: &ParamQcd) -> OjphResult<()> {
        if !self.bands.is_null() {
            for i in 0..4usize {
                unsafe {
                    (*self.bands.add(i)).requantize(codestream, qcd)?;
                }
            }
        }
        if !self.child_res.is_null() {
            unsafe {
                (*self.child_res).requantize(codestream, qcd)?;
            }
        }
        Ok(())
    }
    pub fn parse_all_precincts(
        &mut self,
        data_left: &mut Ui32,
        file: &mut dyn InfileBase,
        phdr: &mut Option<crate::codestream::ojph_packet_ml::PacketHeaderReader>,
    ) -> OjphResult<()> {
        let idx = self.cur_precinct_loc.x + self.cur_precinct_loc.y * self.num_precincts.w;
        for i in (idx as u64)..self.num_precincts.area() {
            if *data_left == 0 {
                break;
            }
            unsafe {
                (*self.precincts.add(i as usize)).parse(
                    self.tag_tree_size,
                    self.level_index.as_mut_ptr(),
                    self.elastic,
                    data_left,
                    file,
                    self.skipped_res_for_read,
                    phdr,
                )?;
            }
            self.cur_precinct_loc.x += 1;
            if self.cur_precinct_loc.x >= self.num_precincts.w {
                self.cur_precinct_loc.x = 0;
                self.cur_precinct_loc.y += 1;
            }
        }
        Ok(())
    }
    pub fn parse_one_precinct(
        &mut self,
        data_left: &mut Ui32,
        file: &mut dyn InfileBase,
        phdr: &mut Option<crate::codestream::ojph_packet_ml::PacketHeaderReader>,
    ) -> OjphResult<()> {
        let idx = self.cur_precinct_loc.x + self.cur_precinct_loc.y * self.num_precincts.w;
        debug_assert!((idx as u64) < self.num_precincts.area());
        if *data_left == 0 {
            return Ok(());
        }
        unsafe {
            (*self.precincts.add(idx as usize)).parse(
                self.tag_tree_size,
                self.level_index.as_mut_ptr(),
                self.elastic,
                data_left,
                file,
                self.skipped_res_for_read,
                phdr,
            )?;
        }
        self.cur_precinct_loc.x += 1;
        if self.cur_precinct_loc.x >= self.num_precincts.w {
            self.cur_precinct_loc.x = 0;
            self.cur_precinct_loc.y += 1;
        }
        Ok(())
    }
    /// Multi-layer parse of the current precinct for a layer-innermost progression order
    /// (RPCL/PCRL/CPRL): all `num_layers` packets of this precinct are consecutive in the stream,
    /// so a per-precinct [`MlPrecinct`] lives entirely within this call — no cross-call persistence,
    /// no arena ownership, no leak. Parses each layer in turn, then folds placeholder passes and
    /// materialises each block's contiguous `next_coded` (see [`Precinct::finalize_ml`]).
    pub fn parse_one_precinct_ml(
        &mut self,
        num_layers: Ui16,
        ccap15: Ui16,
        data_left: &mut Ui32,
        file: &mut dyn InfileBase,
        phdr: &mut Option<crate::codestream::ojph_packet_ml::PacketHeaderReader>,
    ) -> OjphResult<()> {
        let idx = self.cur_precinct_loc.x + self.cur_precinct_loc.y * self.num_precincts.w;
        debug_assert!((idx as u64) < self.num_precincts.area());
        let mut mlp = crate::codestream::ojph_packet_ml::MlPrecinct::default();
        unsafe {
            let p = self.precincts.add(idx as usize);
            for l in 0..num_layers {
                if *data_left == 0 {
                    break;
                }
                (*p).parse_ml_layer(&mut mlp, l, ccap15, data_left, file, self.skipped_res_for_read, phdr)?;
            }
            if !self.skipped_res_for_read {
                (*p).finalize_ml(&mut mlp, self.elastic)?;
            }
        }
        self.cur_precinct_loc.x += 1;
        if self.cur_precinct_loc.x >= self.num_precincts.w {
            self.cur_precinct_loc.x = 0;
            self.cur_precinct_loc.y += 1;
        }
        Ok(())
    }
    /// Multi-layer parse of ALL precincts of this resolution for ONE quality layer, used by the
    /// layer-outermost progression orders (LRCP/RLCP) where each precinct is revisited once per
    /// layer, non-consecutively. Per-precinct [`MlPrecinct`] state lives in `map` (keyed by precinct
    /// pointer) so it survives across the non-adjacent layer visits; on `is_last_layer` each
    /// precinct is finalized (placeholder fold + contiguous `next_coded`). Precincts are swept from
    /// index 0 each layer (not via `cur_precinct_loc`, which single-layer uses for tile-part
    /// resumption).
    #[allow(clippy::too_many_arguments)]
    pub fn parse_all_precincts_ml(
        &mut self,
        layer_idx: Ui16,
        ccap15: Ui16,
        is_last_layer: bool,
        map: &mut std::collections::HashMap<usize, crate::codestream::ojph_packet_ml::MlPrecinct>,
        data_left: &mut Ui32,
        file: &mut dyn InfileBase,
        phdr: &mut Option<crate::codestream::ojph_packet_ml::PacketHeaderReader>,
    ) -> OjphResult<()> {
        let area = self.num_precincts.area();
        for i in 0..area {
            if *data_left == 0 {
                break;
            }
            unsafe {
                let p = self.precincts.add(i as usize);
                let mlp = map.entry(p as usize).or_default();
                (*p).parse_ml_layer(mlp, layer_idx, ccap15, data_left, file, self.skipped_res_for_read, phdr)?;
            }
        }
        if is_last_layer && !self.skipped_res_for_read {
            for i in 0..area {
                unsafe {
                    let p = self.precincts.add(i as usize);
                    if let Some(mlp) = map.get_mut(&(p as usize)) {
                        (*p).finalize_ml(mlp, self.elastic)?;
                    }
                }
            }
        }
        Ok(())
    }
    /// Number of precincts in this resolution (`npw * nph`). 0 for an empty resolution. Used by the
    /// POC packet iterator to bound its precinct enumeration.
    pub fn poc_num_precincts(&self) -> u64 {
        self.num_precincts.area()
    }

    /// Image-domain top-left of precinct `idx` (raster order). The POC iterator uses this to merge
    /// the per-component precinct streams in spatial `(y, x)` order for the position-driven
    /// progression orders (RPCL/PCRL/CPRL).
    pub fn poc_precinct_point(&self, idx: u64) -> Point {
        unsafe { (*self.precincts.add(idx as usize)).img_point }
    }

    /// Parse one quality layer of precinct `idx` into the externally-owned `mlp` accumulator. The
    /// POC iterator drives precincts out of stream order and revisits each once per layer (possibly
    /// across several progression entries), so it keeps the [`MlPrecinct`] state itself and calls
    /// this per `(precinct, layer)`; layers must be presented in increasing order starting at 0
    /// (the iterator guarantees this via its per-precinct next-layer counter). No-op when the
    /// stream is exhausted (`data_left == 0`).
    pub fn poc_parse_layer(
        &mut self,
        idx: u64,
        layer_idx: Ui16,
        ccap15: Ui16,
        mlp: &mut crate::codestream::ojph_packet_ml::MlPrecinct,
        data_left: &mut Ui32,
        file: &mut dyn InfileBase,
        phdr: &mut Option<crate::codestream::ojph_packet_ml::PacketHeaderReader>,
    ) -> OjphResult<()> {
        if *data_left == 0 {
            return Ok(());
        }
        unsafe {
            let p = self.precincts.add(idx as usize);
            (*p).parse_ml_layer(mlp, layer_idx, ccap15, data_left, file, self.skipped_res_for_read, phdr)?;
        }
        Ok(())
    }

    /// Finalize precinct `idx` after all of its parsed layers (placeholder fold + contiguous
    /// `next_coded`). The POC iterator calls this once per touched precinct at the end of the tile
    /// parse, since a precinct's layers may be spread across non-adjacent progression entries.
    pub fn poc_finalize(
        &mut self,
        idx: u64,
        mlp: &mut crate::codestream::ojph_packet_ml::MlPrecinct,
    ) -> OjphResult<()> {
        // A dropped resolution has nothing to decode (its bodies were never accumulated).
        if self.skipped_res_for_read {
            return Ok(());
        }
        unsafe {
            let p = self.precincts.add(idx as usize);
            (*p).finalize_ml(mlp, self.elastic)?;
        }
        Ok(())
    }

    pub const fn get_num_bytes(&self) -> Ui32 {
        self.num_bytes
    }
    pub fn get_num_bytes_for_resolution(&self, resolution_num: Ui32) -> Ui32 {
        if resolution_num == self.res_num {
            self.num_bytes
        } else if self.child_res.is_null() {
            0
        } else {
            unsafe { (*self.child_res).get_num_bytes_for_resolution(resolution_num) }
        }
    }
}
