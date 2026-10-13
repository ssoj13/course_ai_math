//! Port target:
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/openjph/ojph_codestream.h`
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/codestream/ojph_codestream.cpp`

use crate::codestream::ojph_codestream_local::LocalCodestream;
use crate::codestream::ojph_params_local::{
    OJPH_TILEPART_COMPONENTS, OJPH_TILEPART_RESOLUTIONS, ParamCod, ParamNlt, ParamQcd, ParamSiz,
};
use crate::openjph::ojph_defs::Ui32;
use crate::openjph::ojph_file::{InfileBase, OutfileBase};
use crate::openjph::ojph_mem::LineBuf;
use crate::openjph::ojph_message::OjphResult;
use crate::openjph::ojph_params::CommentExchange;

#[allow(non_camel_case_types)]
pub struct Codestream {
    state: Box<LocalCodestream>,
}

impl Default for Codestream {
    fn default() -> Self {
        Self::new()
    }
}

impl Codestream {
    pub fn new() -> Self {
        Self {
            state: LocalCodestream::new_boxed(),
        }
    }

    pub fn restart(&mut self) {
        self.state.restart();
    }

    pub fn access_siz(&mut self) -> &mut ParamSiz {
        self.state.access_siz()
    }

    pub fn access_cod(&mut self) -> &mut ParamCod {
        self.state.access_cod()
    }

    pub fn access_qcd(&self) -> &ParamQcd {
        self.state.access_qcd()
    }

    pub fn access_qcd_mut(&mut self) -> &mut ParamQcd {
        self.state.access_qcd_mut()
    }

    pub fn access_nlt(&self) -> &ParamNlt {
        self.state.get_nlt()
    }

    pub fn access_nlt_mut(&mut self) -> &mut ParamNlt {
        self.state.get_nlt_mut()
    }

    pub fn set_planar(&mut self, planar: bool) {
        self.state.set_planar(if planar { 1 } else { 0 });
    }

    pub fn set_profile(&mut self, s: &str) -> crate::openjph::ojph_message::OjphResult<()> {
        self.state.set_profile(s)
    }

    pub fn set_tilepart_divisions(&mut self, at_resolutions: bool, at_components: bool) {
        let mut value = 0;
        if at_resolutions {
            value |= OJPH_TILEPART_RESOLUTIONS;
        }
        if at_components {
            value |= OJPH_TILEPART_COMPONENTS;
        }
        self.state.set_tilepart_divisions(value);
    }

    pub fn is_tilepart_division_at_resolutions(&self) -> bool {
        (self.state.get_tilepart_div() & OJPH_TILEPART_RESOLUTIONS) != 0
    }

    pub fn is_tilepart_division_at_components(&self) -> bool {
        (self.state.get_tilepart_div() & OJPH_TILEPART_COMPONENTS) != 0
    }

    pub fn request_tlm_marker(&mut self, needed: bool) {
        self.state.request_tlm_marker(needed);
    }

    pub fn is_tlm_requested(&self) -> bool {
        self.state.is_tlm_needed()
    }

    pub fn is_planar(&self) -> bool {
        self.state.is_planar()
    }

    pub fn write_headers(
        &mut self,
        file: &mut dyn OutfileBase,
        comments: *const CommentExchange,
        num_comments: Ui32,
    ) -> OjphResult<()> {
        // Convert raw pointer + count into an optional slice for the local implementation.
        let slice = if !comments.is_null() && num_comments > 0 {
            Some(unsafe { core::slice::from_raw_parts(comments, num_comments as usize) })
        } else {
            None
        };
        self.state.write_headers(file, slice)
    }

    pub fn write_headers_slice(
        &mut self,
        file: &mut dyn OutfileBase,
        comments: &[CommentExchange],
    ) -> OjphResult<()> {
        let slice = if comments.is_empty() {
            None
        } else {
            Some(comments)
        };
        self.state.write_headers(file, slice)
    }

    /// Lossy encode only: quantize by truncation (T.800 dead-zone) on every SIMD tier, making the
    /// bitstream CPU-independent. Default (`false`) keeps OpenJPH's tier-dependent behaviour
    /// (scalar truncates, SSE2/AVX2 round to nearest).
    pub fn set_trunc_quant(&mut self, trunc: bool) {
        self.state.set_trunc_quant(trunc);
    }

    pub fn enable_resilience(&mut self) -> crate::openjph::ojph_message::OjphResult<()> {
        self.state.enable_resilience()
    }

    pub fn read_headers(&mut self, file: &mut dyn InfileBase) -> OjphResult<()> {
        self.state.read_headers(file)
    }

    pub fn restrict_input_resolution(
        &mut self,
        skipped_res_for_read: Ui32,
        skipped_res_for_recon: Ui32,
    ) -> OjphResult<()> {
        self.state
            .restrict_input_resolution(skipped_res_for_read, skipped_res_for_recon)
    }

    pub fn create(&mut self) -> OjphResult<()> {
        self.state.read()
    }

    pub fn pull(&mut self, comp_num: &mut Ui32) -> OjphResult<*mut LineBuf> {
        self.state.pull(comp_num)
    }

    /// Safe variant of `pull` — returns a reference valid until the next codec call.
    pub fn pull_ref(&mut self, comp_num: &mut Ui32) -> OjphResult<&mut LineBuf> {
        let ptr = self.state.pull(comp_num)?;
        if ptr.is_null() {
            // Pulling past the last line of the last component yields null.
            return Err(crate::ojph_error!(0x000300EF, "pull past the end of the image"));
        }
        // SAFETY: non-null (checked above) and valid for the lifetime of `&mut self` (no aliasing
        // while we hold &mut Codestream).
        Ok(unsafe { &mut *ptr })
    }

    pub fn flush(&mut self) -> crate::openjph::ojph_message::OjphResult<()> {
        self.state.flush()
    }

    pub fn close(&mut self) {
        self.state.close();
    }

    pub fn exchange(
        &mut self,
        line: *mut LineBuf,
        next_component: &mut Ui32,
    ) -> OjphResult<*mut LineBuf> {
        self.state.exchange(line, next_component)
    }

    /// Safe bootstrapping exchange: gets the first `LineBuf` without submitting one.
    pub fn exchange_first(&mut self, next_component: &mut Ui32) -> OjphResult<&mut LineBuf> {
        let ptr = self.state.exchange(std::ptr::null_mut(), next_component)?;
        if ptr.is_null() {
            return Err(crate::ojph_error!(0x000300F0, "the codestream has no line to fill"));
        }
        // SAFETY: see pull_ref — non-null, valid for &mut self lifetime, no aliasing.
        Ok(unsafe { &mut *ptr })
    }

    /// Fill the current line buffer via a closure, then advance to the next component.
    ///
    /// `fill(lb, next_comp)` is called with the current `LineBuf` and the component index
    /// that was just confirmed.  The returned `next_comp` value is written back to the
    /// caller's variable after the internal `exchange` call.
    ///
    /// This keeps the raw `*mut LineBuf` pointer entirely inside jph-rs.
    pub fn fill_and_exchange<F>(
        &mut self,
        cur_line: &mut *mut LineBuf,
        next_comp: &mut Ui32,
        fill: F,
    ) -> OjphResult<()>
    where
        F: FnOnce(&mut LineBuf, Ui32),
    {
        if cur_line.is_null() {
            return Err(crate::ojph_error!(0x000300F1, "fill_and_exchange called after the last line"));
        }
        let confirmed_comp = *next_comp;
        // SAFETY: non-null (checked above); OpenJPH owns it and it is valid until the next exchange.
        let lb = unsafe { &mut **cur_line };
        fill(lb, confirmed_comp);
        *cur_line = self.state.exchange(*cur_line, next_comp)?;
        Ok(())
    }

    /// Parse-only enumeration (additive front-end for [`crate::parse::enumerate`]): collect one
    /// [`crate::parse::CodeblockPlan`] per non-empty code-block, **without** entropy decode or
    /// DWT. Delegates to [`LocalCodestream::enumerate_blocks`] (which reads private tile-tree
    /// fields). Call only after [`Self::read_headers`] + [`Self::create`].
    pub(crate) fn enumerate_blocks(&mut self, sink: &mut dyn FnMut(crate::parse::CodeblockPlan)) {
        // SAFETY: read_headers + create allocated the tile tree and populated every coded_cbs;
        // this is a read-only walk of that parsed state (no line pulled, no entropy decode).
        unsafe { self.state.enumerate_blocks(sink) };
    }

    pub fn state(&self) -> &LocalCodestream {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut LocalCodestream {
        &mut self.state
    }
}
