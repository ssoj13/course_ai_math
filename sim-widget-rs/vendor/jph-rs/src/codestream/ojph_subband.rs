//! Port target:
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/codestream/ojph_subband.h`
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/codestream/ojph_subband.cpp`

use crate::codestream::ojph_codeblock::{Codeblock, CodedCbHeader};
use crate::codestream::ojph_codestream_local::LocalCodestream;
use crate::codestream::ojph_params_local::ParamQcd;
use crate::codestream::ojph_precinct::Precinct;
use crate::codestream::ojph_resolution::Resolution;
use crate::openjph::ojph_base::{Rect, Size};
use crate::openjph::ojph_defs::{Ui32, ojph_max, ojph_min};
use crate::openjph::ojph_mem::{LineBuf, mem_elastic_allocator};
use crate::openjph::ojph_message::OjphResult;
use core::ptr;

#[repr(C)]
pub struct Subband {
    pub(crate) empty: bool,
    pub(crate) res_num: Ui32,
    pub(crate) band_num: Ui32,
    pub(crate) reversible: bool,
    pub(crate) band_rect: Rect,
    pub(crate) lines: *mut LineBuf,
    pub(crate) parent: *mut Resolution,
    pub(crate) blocks: *mut Codeblock,
    pub(crate) num_blocks: Size,
    pub(crate) log_pp: Size,
    pub(crate) xcb_prime: Ui32,
    pub(crate) ycb_prime: Ui32,
    pub(crate) cur_cb_row: Ui32,
    pub(crate) cur_line: i32,
    pub(crate) cur_cb_height: i32,
    pub(crate) delta: f32,
    pub(crate) delta_inv: f32,
    pub(crate) k_max: Ui32,
    pub(crate) coded_cbs: *mut CodedCbHeader,
    pub(crate) elastic: *mut mem_elastic_allocator,
}

impl Default for Subband {
    fn default() -> Self {
        Self {
            empty: true,
            res_num: 0,
            band_num: 0,
            reversible: false,
            band_rect: Rect::default(),
            lines: ptr::null_mut(),
            parent: ptr::null_mut(),
            blocks: ptr::null_mut(),
            num_blocks: Size::default(),
            log_pp: Size::default(),
            xcb_prime: 0,
            ycb_prime: 0,
            cur_cb_row: 0,
            cur_line: 0,
            cur_cb_height: 0,
            delta: 0.0,
            delta_inv: 0.0,
            k_max: 0,
            coded_cbs: ptr::null_mut(),
            elastic: ptr::null_mut(),
        }
    }
}

impl Subband {
    pub fn pre_alloc(
        codestream: &mut LocalCodestream,
        band_rect: Rect,
        comp_num: Ui32,
        res_num: Ui32,
        transform_flags: Ui32,
    ) {
        let allocator = unsafe { &mut *codestream.get_allocator() };
        let empty = band_rect.siz.w == 0 || band_rect.siz.h == 0;
        if empty {
            return;
        }

        let cdp = unsafe { &*codestream.get_coc(comp_num) };
        let log_cb = cdp.get_log_block_dims();
        let log_pp = cdp.get_log_precinct_size(res_num);
        let x_off = if transform_flags & Resolution::HORZ_TRX != 0 {
            1
        } else {
            0
        };
        let y_off = if transform_flags & Resolution::VERT_TRX != 0 {
            1
        } else {
            0
        };
        let xcb_prime = ojph_min(log_cb.w, log_pp.w - x_off);
        let ycb_prime = ojph_min(log_cb.h, log_pp.h - y_off);
        let nominal = Size::new(1u32 << xcb_prime, 1u32 << ycb_prime);

        let tbx0 = band_rect.org.x;
        let tby0 = band_rect.org.y;
        let tbx1 = band_rect.org.x + band_rect.siz.w;
        let tby1 = band_rect.org.y + band_rect.siz.h;
        let mut num_blocks = Size::default();
        num_blocks.w = ((tbx1 + (1 << xcb_prime) - 1) >> xcb_prime) - (tbx0 >> xcb_prime);
        num_blocks.h = ((tby1 + (1 << ycb_prime) - 1) >> ycb_prime) - (tby0 >> ycb_prime);

        allocator.pre_alloc_obj::<Codeblock>(num_blocks.w as usize);
        allocator.pre_alloc_obj::<CodedCbHeader>(num_blocks.area() as usize);

        let qp = unsafe { &*codestream.access_qcd().get_qcc(comp_num) };
        let precision = qp.propose_precision(cdp as *const _);
        let atk = unsafe { &*cdp.access_atk() };
        let reversible = atk.is_reversible();

        for _ in 0..num_blocks.w {
            Codeblock::pre_alloc(codestream, nominal, precision);
        }

        allocator.pre_alloc_obj::<LineBuf>(1);
        let width = (band_rect.siz.w + 1) as usize;
        if reversible {
            if precision <= 32 {
                allocator.pre_alloc_data::<i32>(width, 1);
            } else {
                allocator.pre_alloc_data::<i64>(width, 1);
            }
        } else {
            allocator.pre_alloc_data::<f32>(width, 1);
        }
    }

    pub fn finalize_alloc(
        &mut self,
        codestream: &mut LocalCodestream,
        band_rect: Rect,
        res: *mut Resolution,
        res_num: Ui32,
        subband_num: Ui32,
    ) -> OjphResult<()> {
        let allocator = unsafe { &mut *codestream.get_allocator() };
        self.elastic = codestream.get_elastic_alloc();
        self.res_num = res_num;
        self.band_num = subband_num;
        self.band_rect = band_rect;
        self.parent = res;

        let cdp = unsafe { &*codestream.get_coc((*self.parent).get_comp_num()) };
        self.reversible = unsafe { (*cdp.access_atk()).is_reversible() };
        let log_cb = cdp.get_log_block_dims();
        self.log_pp = cdp.get_log_precinct_size(res_num);

        let x_off = if unsafe { (*self.parent).has_horz_transform() } {
            1
        } else {
            0
        };
        let y_off = if unsafe { (*self.parent).has_vert_transform() } {
            1
        } else {
            0
        };
        self.xcb_prime = ojph_min(log_cb.w, self.log_pp.w - x_off);
        self.ycb_prime = ojph_min(log_cb.h, self.log_pp.h - y_off);
        let nominal = Size::new(1u32 << self.xcb_prime, 1u32 << self.ycb_prime);

        self.cur_cb_row = 0;
        self.cur_line = 0;
        self.cur_cb_height = 0;

        let comp_num = unsafe { (*self.parent).get_comp_num() };
        let qcd = unsafe { &*codestream.access_qcd().get_qcc(comp_num) };
        self.set_quant(codestream, cdp, qcd)?;
        let precision = qcd.propose_precision(cdp as *const _);

        self.empty = band_rect.siz.w == 0 || band_rect.siz.h == 0;
        if self.empty {
            return Ok(());
        }

        let tbx0 = band_rect.org.x;
        let tby0 = band_rect.org.y;
        let tbx1 = band_rect.org.x + band_rect.siz.w;
        let tby1 = band_rect.org.y + band_rect.siz.h;

        self.num_blocks = Size::default();
        self.num_blocks.w =
            ((tbx1 + (1 << self.xcb_prime) - 1) >> self.xcb_prime) - (tbx0 >> self.xcb_prime);
        self.num_blocks.h =
            ((tby1 + (1 << self.ycb_prime) - 1) >> self.ycb_prime) - (tby0 >> self.ycb_prime);

        self.blocks = allocator.post_alloc_obj::<Codeblock>(self.num_blocks.w as usize);
        self.coded_cbs = allocator.post_alloc_obj::<CodedCbHeader>(self.num_blocks.area() as usize);
        for i in 0..self.num_blocks.area() as usize {
            unsafe {
                ptr::write(self.coded_cbs.add(i), CodedCbHeader::default());
                (*self.coded_cbs.add(i)).kmax = self.k_max;
            }
        }

        let x_lower_bound = (tbx0 >> self.xcb_prime) << self.xcb_prime;
        let y_lower_bound = (tby0 >> self.ycb_prime) << self.ycb_prime;
        let mut cb_size = Size::default();
        cb_size.h = ojph_min(tby1, y_lower_bound + nominal.h) - tby0;
        self.cur_cb_height = cb_size.h as i32;
        let mut line_offset = 0;
        for i in 0..self.num_blocks.w as usize {
            let cbx0 = ojph_max(tbx0, x_lower_bound + i as Ui32 * nominal.w);
            let cbx1 = ojph_min(tbx1, x_lower_bound + (i as Ui32 + 1) * nominal.w);
            cb_size.w = cbx1 - cbx0;
            unsafe {
                ptr::write(self.blocks.add(i), Codeblock::default());
                (*self.blocks.add(i)).finalize_alloc(
                    codestream,
                    self,
                    nominal,
                    cb_size,
                    self.coded_cbs.add(i),
                    self.k_max,
                    line_offset,
                    precision,
                    comp_num,
                );
            }
            line_offset += cb_size.w as i32;
        }

        self.lines = allocator.post_alloc_obj::<LineBuf>(1);
        unsafe {
            ptr::write(self.lines, LineBuf::default());
        }
        let width = (band_rect.siz.w + 1) as usize;
        if self.reversible {
            if precision <= 32 {
                unsafe {
                    (*self.lines).wrap_i32(allocator.post_alloc_data::<i32>(width, 1), width, 1)
                };
            } else {
                unsafe {
                    (*self.lines).wrap_i64(allocator.post_alloc_data::<i64>(width, 1), width, 1)
                };
            }
        } else {
            unsafe { (*self.lines).wrap_f32(allocator.post_alloc_data::<f32>(width, 1), width, 1) };
        }
        Ok(())
    }

    pub fn get_cb_indices(&mut self, num_precincts: Size, precincts: *mut Precinct) {
        if self.empty {
            return;
        }

        let res_rect = unsafe { (*self.parent).get_rect() };
        let trx0 = res_rect.org.x;
        let try0 = res_rect.org.y;
        let trx1 = res_rect.org.x + res_rect.siz.w;
        let try1 = res_rect.org.y + res_rect.siz.h;
        let pc_lft = (res_rect.org.x >> self.log_pp.w) << self.log_pp.w;
        let pc_top = (res_rect.org.y >> self.log_pp.h) << self.log_pp.h;
        let x_shift = if unsafe { (*self.parent).has_horz_transform() } {
            1
        } else {
            0
        };
        let y_shift = if unsafe { (*self.parent).has_vert_transform() } {
            1
        } else {
            0
        };

        let mut coly = 0;
        let mut colx = 0;
        for y in 0..num_precincts.h {
            let mut pcy0 = ojph_max(try0, pc_top + (y << self.log_pp.h));
            let mut pcy1 = ojph_min(try1, pc_top + ((y + 1) << self.log_pp.h));
            pcy0 = (pcy0
                .wrapping_sub(self.band_num >> 1)
                .wrapping_add((1 << y_shift) - 1))
                >> y_shift;
            pcy1 = (pcy1
                .wrapping_sub(self.band_num >> 1)
                .wrapping_add((1 << y_shift) - 1))
                >> y_shift;

            let p = unsafe { precincts.add((y * num_precincts.w) as usize) };
            let yb =
                ((pcy1 + (1 << self.ycb_prime) - 1) >> self.ycb_prime) - (pcy0 >> self.ycb_prime);
            colx = 0;
            for x in 0..num_precincts.w {
                let mut pcx0 = ojph_max(trx0, pc_lft + (x << self.log_pp.w));
                let mut pcx1 = ojph_min(trx1, pc_lft + ((x + 1) << self.log_pp.w));
                pcx0 = (pcx0
                    .wrapping_sub(self.band_num & 1)
                    .wrapping_add((1 << x_shift) - 1))
                    >> x_shift;
                pcx1 = (pcx1
                    .wrapping_sub(self.band_num & 1)
                    .wrapping_add((1 << x_shift) - 1))
                    >> x_shift;

                let bp = unsafe { &mut (*p.add(x as usize)).cb_idxs[self.band_num as usize] };
                let xb = ((pcx1 + (1 << self.xcb_prime) - 1) >> self.xcb_prime)
                    - (pcx0 >> self.xcb_prime);
                bp.org.x = colx;
                bp.org.y = coly;
                bp.siz.w = xb;
                bp.siz.h = yb;
                colx += xb;
            }
            coly += yb;
        }
        debug_assert!(colx == self.num_blocks.w && coly == self.num_blocks.h);
    }

    pub fn exchange_buf(&mut self, l: *mut LineBuf) {
        if self.empty {
            return;
        }

        unsafe {
            debug_assert!(
                (*l).pre_size == (*self.lines).pre_size
                    && (*l).size == (*self.lines).size
                    && (*l).flags == (*self.lines).flags
            );
            let p = (*self.lines).storage.p;
            (*self.lines).storage.p = (*l).storage.p;
            (*l).storage.p = p;
        }
    }

    pub const fn get_line(&self) -> *mut LineBuf {
        self.lines
    }

    pub fn push_line(&mut self) -> OjphResult<()> {
        if self.empty {
            return Ok(());
        }

        for i in 0..self.num_blocks.w as usize {
            unsafe {
                (*self.blocks.add(i)).push(self.lines);
            }
        }
        self.cur_line += 1;
        if self.cur_line >= self.cur_cb_height {
            for i in 0..self.num_blocks.w as usize {
                unsafe {
                    (*self.blocks.add(i)).encode(self.elastic)?;
                }
            }

            self.cur_cb_row += 1;
            if self.cur_cb_row < self.num_blocks.h {
                self.cur_line = 0;
                let tbx0 = self.band_rect.org.x;
                let tby0 = self.band_rect.org.y;
                let tbx1 = self.band_rect.org.x + self.band_rect.siz.w;
                let tby1 = self.band_rect.org.y + self.band_rect.siz.h;
                let nominal = Size::new(1u32 << self.xcb_prime, 1u32 << self.ycb_prime);
                let x_lower_bound = (tbx0 >> self.xcb_prime) << self.xcb_prime;
                let y_lower_bound = (tby0 >> self.ycb_prime) << self.ycb_prime;
                let cby0 = y_lower_bound + self.cur_cb_row * nominal.h;
                let cby1 = ojph_min(tby1, cby0 + nominal.h);
                let mut cb_size = Size::default();
                cb_size.h = cby1 - ojph_max(tby0, cby0);
                self.cur_cb_height = cb_size.h as i32;
                for i in 0..self.num_blocks.w as usize {
                    let cbx0 = ojph_max(tbx0, x_lower_bound + i as Ui32 * nominal.w);
                    let cbx1 = ojph_min(tbx1, x_lower_bound + (i as Ui32 + 1) * nominal.w);
                    cb_size.w = cbx1 - cbx0;
                    unsafe {
                        (*self.blocks.add(i)).recreate(
                            cb_size,
                            self.coded_cbs
                                .add(i + self.cur_cb_row as usize * self.num_blocks.w as usize),
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// Derive `k_max` and, for the irreversible path, `delta`/`delta_inv` for this subband from the
    /// quantization parameters `qcd` (main QCD at allocation, tile QCD on an in-tile override) and
    /// the component's COD/COC (`cdp`, decomposition count + DFS). Single place for both callers.
    /// The `1 << (31 - k_max)` scaling only exists for `k_max <= 31`; anything larger is a corrupt
    /// exponent and is refused rather than wrapped.
    fn set_quant(&mut self, codestream: &LocalCodestream, cdp: &crate::codestream::ojph_params_local::ParamCod, qcd: &ParamQcd) -> OjphResult<()> {
        let comp_num = unsafe { (*self.parent).get_comp_num() };
        let mut dfs = ptr::null();
        if cdp.is_dfs_defined() {
            dfs = codestream.access_dfs_ptr();
            if !dfs.is_null() {
                dfs = match unsafe { (*dfs).get_dfs(cdp.get_dfs_index() as i32) } {
                    Some(v) => v as *const _,
                    None => ptr::null(),
                };
            }
        }
        let qp = unsafe { &*qcd.get_qcc(comp_num) };
        let num_decomps = cdp.get_num_decompositions() as Ui32;
        self.k_max = qp.get_kmax(dfs, num_decomps, self.res_num, self.band_num);
        if !self.reversible {
            if self.k_max > 31 {
                return Err(crate::ojph_error!(
                    0x000300EC,
                    "irreversible subband needs {} magnitude bits, more than the supported 31",
                    self.k_max
                ));
            }
            let mut d = qp.get_irrev_delta(dfs, num_decomps, self.res_num, self.band_num)?;
            d /= (1u32 << (31 - self.k_max)) as f32;
            self.delta = d;
            self.delta_inv = 1.0f32 / d;
        }
        Ok(())
    }

    /// Apply an in-tile QCD/QCC override: recompute `k_max` (and `delta`/`delta_inv` for the
    /// irreversible path) from `qcd` — the quantization parameters that apply to this subband's
    /// tile — then push the refreshed values into the already-finalized code-blocks (used by the
    /// inverse quantizer) and coded headers (whose `kmax` bounds the parsed missing-MSBs). This
    /// mirrors the quantization block of [`Self::finalize_alloc`] without re-allocating anything.
    /// No-op for unused band slots (never finalized); harmless for empty bands (no code-blocks).
    pub fn requantize(&mut self, codestream: &LocalCodestream, qcd: &ParamQcd) -> OjphResult<()> {
        if self.parent.is_null() {
            return Ok(());
        }
        let comp_num = unsafe { (*self.parent).get_comp_num() };
        let cdp = unsafe { &*codestream.get_coc(comp_num) };
        self.set_quant(codestream, cdp, qcd)?;
        if !self.blocks.is_null() {
            for i in 0..self.num_blocks.w as usize {
                unsafe {
                    (*self.blocks.add(i)).requantize(self.k_max, self.delta);
                }
            }
        }
        if !self.coded_cbs.is_null() {
            for i in 0..self.num_blocks.area() as usize {
                unsafe {
                    (*self.coded_cbs.add(i)).kmax = self.k_max;
                }
            }
        }
        Ok(())
    }

    pub const fn get_delta(&self) -> f32 {
        self.delta
    }

    pub const fn exists(&self) -> bool {
        !self.empty
    }

    /// Parse-only enumeration (additive front-end for `crate::parse::enumerate`): emit one
    /// [`crate::parse::CodeblockPlan`] per non-empty code-block in this subband, **without**
    /// entropy decode or DWT. This is a direct port of the code-block grid math in
    /// [`Self::pull_line`] (the `cur_line<=0` block, lines ~464-510) and the plan-field read
    /// in `Codeblock::capture_debug_record`, with `recreate`/`decode` removed — every plan
    /// field already exists in the parsed `CodedCbHeader` once `read_headers`/`create` have run.
    ///
    /// Inclusion predicate mirrors `Codeblock::decode` (skip empty blocks: zero cleanup-pass
    /// length, zero passes, or a null coded list) — this both matches what the oracle records
    /// and avoids dereferencing a null `next_coded`.
    ///
    /// # Safety
    /// The subband must be fully parsed: `coded_cbs`/`blocks` populated and `band_rect` set
    /// (state left by `read_headers` + `create`). The caller (`LocalCodestream::enumerate_blocks`)
    /// guarantees this.
    pub(crate) unsafe fn enumerate_blocks(
        &self,
        comp_num: Ui32,
        tile_idx: u32,
        sink: &mut dyn FnMut(crate::parse::CodeblockPlan),
    ) {
        if self.empty {
            return;
        }
        let tbx0 = self.band_rect.org.x;
        let tby0 = self.band_rect.org.y;
        let tbx1 = tbx0 + self.band_rect.siz.w;
        let tby1 = tby0 + self.band_rect.siz.h;
        let nom_w = 1u32 << self.xcb_prime;
        let nom_h = 1u32 << self.ycb_prime;
        let x_lower = (tbx0 >> self.xcb_prime) << self.xcb_prime;
        let y_lower = (tby0 >> self.ycb_prime) << self.ycb_prime;
        for row in 0..self.num_blocks.h {
            let cby0 = ojph_max(tby0, y_lower + row * nom_h);
            let cby1 = ojph_min(tby1, y_lower + (row + 1) * nom_h);
            for col in 0..self.num_blocks.w {
                let cbx0 = ojph_max(tbx0, x_lower + col * nom_w);
                let cbx1 = ojph_min(tbx1, x_lower + (col + 1) * nom_w);
                // coded_cbs is the full grid, row-major; blocks[] is one row wide (reused per
                // row), and is_ht/precision are uniform per subband — so index blocks by col.
                let idx = (col + row * self.num_blocks.w) as usize;
                let cb = *self.coded_cbs.add(idx);
                if cb.pass_length[0] == 0 || cb.num_passes == 0 || cb.next_coded.is_null() {
                    continue;
                }
                let len = (cb.pass_length[0] + cb.pass_length[1]) as usize;
                let src = (*cb.next_coded)
                    .buf
                    .add(CodedCbHeader::PREFIX_BUF_SIZE as usize);
                let mut coded = vec![0u8; len];
                ptr::copy_nonoverlapping(src, coded.as_mut_ptr(), len);
                let blk = &*self.blocks.add(col as usize);
                sink(crate::parse::CodeblockPlan {
                    coded,
                    len_cup: cb.pass_length[0],
                    len_ref: cb.pass_length[1],
                    num_passes: cb.num_passes,
                    missing_msbs: cb.missing_msbs,
                    width: cbx1 - cbx0,
                    height: cby1 - cby0,
                    is_64bit: blk.is_64bit(),
                    is_ht: blk.is_ht(),
                    geom: crate::parse::Geom {
                        comp: comp_num,
                        res: self.res_num,
                        band: self.band_num,
                        tile: tile_idx,
                        bx0: cbx0 - tbx0,
                        by0: cby0 - tby0,
                        band_w: self.band_rect.siz.w,
                        band_h: self.band_rect.siz.h,
                    },
                });
            }
        }
    }

    pub fn pull_line(&mut self) -> OjphResult<*mut LineBuf> {
        if self.empty {
            return Ok(self.lines);
        }

        // ROI max-shift (RGN) for this subband's component (0 = none); applied per code-block right
        // after decode, on the raw magnitude buffer (see Codeblock::apply_roi).
        let roishift = unsafe { (*self.parent).get_roishift() };

        self.cur_line -= 1;
        if self.cur_line <= 0 && self.cur_cb_row < self.num_blocks.h {
            let tbx0 = self.band_rect.org.x;
            let tby0 = self.band_rect.org.y;
            let tbx1 = self.band_rect.org.x + self.band_rect.siz.w;
            let tby1 = self.band_rect.org.y + self.band_rect.siz.h;
            let nominal = Size::new(1u32 << self.xcb_prime, 1u32 << self.ycb_prime);
            let x_lower_bound = (tbx0 >> self.xcb_prime) << self.xcb_prime;
            let y_lower_bound = (tby0 >> self.ycb_prime) << self.ycb_prime;
            let cby0 = ojph_max(tby0, y_lower_bound + self.cur_cb_row * nominal.h);
            let cby1 = ojph_min(tby1, y_lower_bound + (self.cur_cb_row + 1) * nominal.h);
            let mut cb_size = Size::default();
            cb_size.h = cby1 - cby0;
            self.cur_line = cb_size.h as i32;
            self.cur_cb_height = cb_size.h as i32;
            for i in 0..self.num_blocks.w as usize {
                let cbx0 = ojph_max(tbx0, x_lower_bound + i as Ui32 * nominal.w);
                let cbx1 = ojph_min(tbx1, x_lower_bound + (i as Ui32 + 1) * nominal.w);
                cb_size.w = cbx1 - cbx0;
                unsafe {
                    (*self.blocks.add(i)).recreate(
                        cb_size,
                        self.coded_cbs
                            .add(i + self.cur_cb_row as usize * self.num_blocks.w as usize),
                    );
                    // Debug-only: record this block's placement for the GPU port's oracle,
                    // in the same order the per-block decode tap fires (see debug_extract).
                    #[cfg(feature = "debug-extract")]
                    crate::others::debug_extract::set_geom(
                        (*self.parent).get_comp_num(),
                        self.res_num,
                        self.band_num,
                        cbx0 - tbx0,
                        cby0 - tby0,
                        self.band_rect.siz.w,
                        self.band_rect.siz.h,
                    );
                    // Pass roishift so a Part-1 irreversible block can defer its reconstruction
                    // mid-point to apply_roi (the ROI form is a fixed offset, not per-sample).
                    (*self.blocks.add(i)).decode(roishift)?;
                    // ROI max-shift (RGN): rescale this block's background coefficients on the raw
                    // magnitude buffer, before pull_line runs tx_from_cb. No-op when roishift == 0.
                    if roishift != 0 {
                        (*self.blocks.add(i)).apply_roi(roishift);
                    }
                }
            }
            self.cur_cb_row += 1;
        }

        debug_assert!(self.cur_line >= 0);
        for i in 0..self.num_blocks.w as usize {
            unsafe {
                (*self.blocks.add(i)).pull_line(self.lines);
            }
        }
        Ok(self.lines)
    }

    pub const fn get_parent(&self) -> *mut Resolution {
        self.parent
    }
}
