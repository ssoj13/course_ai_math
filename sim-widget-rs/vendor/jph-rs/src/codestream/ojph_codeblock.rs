//! Port target:
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/codestream/ojph_codeblock.h`
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/codestream/ojph_codeblock.cpp`

use crate::codestream::ojph_codeblock_fun::{
    CbEncoderFun32, CodeblockFun, TxFromCbFun64, TxToCbFun64,
};
use crate::codestream::ojph_codestream_gen::{gen_rev_tx_from_cb64, gen_rev_tx_to_cb64};
use crate::codestream::ojph_codestream_local::LocalCodestream;
use crate::codestream::ojph_subband::Subband;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use crate::coding::ojph_block_encoder_avx2::ojph_encode_codeblock_avx2;
#[cfg(target_arch = "x86_64")]
use crate::coding::ojph_block_encoder_avx512::ojph_encode_codeblock_avx512;
use crate::openjph::ojph_arch::BYTE_ALIGNMENT;
use crate::openjph::ojph_defs::{Si64, Ui32, Ui64};
use crate::openjph::ojph_mem::{CodedLists, LFT_32BIT, LFT_64BIT, LineBuf, mem_elastic_allocator};
use core::ptr;

#[repr(C)]
#[derive(Clone, Copy)]
pub union CodeblockBuf {
    pub buf32: *mut Ui32,
    pub buf64: *mut Ui64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union CodeblockMaxVals {
    pub max_val32: [Ui32; 8],
    pub max_val64: [Ui64; 4],
}

impl Default for CodeblockBuf {
    fn default() -> Self {
        Self {
            buf32: ptr::null_mut(),
        }
    }
}

impl Default for CodeblockMaxVals {
    fn default() -> Self {
        Self { max_val32: [0; 8] }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CodedCbHeader {
    pub pass_length: [Ui32; 2],
    pub num_passes: Ui32,
    pub kmax: Ui32,
    pub missing_msbs: Ui32,
    pub next_coded: *mut CodedLists,
    /// Multi-layer parse state (unused on the single-layer path, which matches OpenJPH): whether
    /// this code-block has contributed in a previous quality layer, and its running `Lblock`.
    /// Persist across the per-layer packet parses of one precinct (ITU-T T.800 B.10).
    pub already_included: bool,
    pub lblock: Ui32,
    /// RESTART (T.800 code-block style 0x04) per-pass segment lengths: each coding pass forms its
    /// own codeword segment, so the Part-1 packet parser stores `seg_count` (= `num_passes`) byte
    /// lengths here for the block decoder driver. Null / `0` on every non-RESTART block (which uses
    /// the single `pass_length[0]` segment). A raw pointer + `u16` keep `CodedCbHeader` `Copy`; the
    /// array lives in the elastic allocator, so it outlives the parse like `next_coded`.
    pub seg_lengths: *mut Ui32,
    pub seg_count: u16,
    /// HT MIXED mode (T.814): this block sits in an HT component but was coded with the original
    /// Part-1 (MQ) coder, as the multi-layer packet parser determined from its first segment.
    /// `Codeblock::decode` routes such blocks to the Part-1 decoder instead of the HT one.
    pub orig_j2k: bool,
}

impl Default for CodedCbHeader {
    fn default() -> Self {
        Self {
            pass_length: [0; 2],
            num_passes: 0,
            kmax: 0,
            missing_msbs: 0,
            next_coded: ptr::null_mut(),
            already_included: false,
            lblock: 0,
            seg_lengths: ptr::null_mut(),
            seg_count: 0,
            orig_j2k: false,
        }
    }
}

impl CodedCbHeader {
    pub const PREFIX_BUF_SIZE: i32 = 8;
    pub const SUFFIX_BUF_SIZE: i32 = 16;
}

#[repr(C)]
pub struct Codeblock {
    precision: Ui32,
    buf: CodeblockBuf,
    nominal_size: crate::openjph::ojph_base::Size,
    cb_size: crate::openjph::ojph_base::Size,
    stride: Ui32,
    buf_size: Ui32,
    parent: *mut Subband,
    line_offset: i32,
    cur_line: Ui32,
    delta: f32,
    delta_inv: f32,
    k_max: Ui32,
    reversible: bool,
    resilient: bool,
    stripe_causal: bool,
    /// HT (HTJ2K) vs Part-1 (legacy MQ/EBCOT) block coding, from the code-block style HT bit.
    is_ht: bool,
    /// Part-1 coding modes (SPcod/SPcoc low 6 bits: BYPASS/RESET/RESTART/CAUSAL/ERTERM/SEGMARK);
    /// unused on the HT path.
    cmodes: u16,
    zero_block: bool,
    max_val: CodeblockMaxVals,
    coded_cb: *mut CodedCbHeader,
    codeblock_functions: CodeblockFun,
}

impl Default for Codeblock {
    fn default() -> Self {
        Self {
            precision: 0,
            buf: CodeblockBuf {
                buf32: ptr::null_mut(),
            },
            nominal_size: crate::openjph::ojph_base::Size::default(),
            cb_size: crate::openjph::ojph_base::Size::default(),
            stride: 0,
            buf_size: 0,
            parent: ptr::null_mut(),
            line_offset: 0,
            cur_line: 0,
            delta: 0.0,
            delta_inv: 0.0,
            k_max: 0,
            reversible: false,
            resilient: false,
            stripe_causal: false,
            is_ht: true,
            cmodes: 0,
            zero_block: false,
            max_val: CodeblockMaxVals { max_val64: [0; 4] },
            coded_cb: ptr::null_mut(),
            codeblock_functions: CodeblockFun::default(),
        }
    }
}

impl Codeblock {
    const BUF32: Ui32 = 4;
    const BUF64: Ui32 = 8;

    /// Parse-only accessor (additive, for `crate::parse::enumerate`): HT (HTJ2K) vs Part-1
    /// block coding. Exposes the private `is_ht` field so the parse front-end can carry the
    /// coding mode into each `CodeblockPlan` without running entropy decode. Uniform per
    /// subband (set once at alloc), so reading it from `blocks[col]` is sufficient.
    pub(crate) fn is_ht(&self) -> bool {
        self.is_ht
    }

    /// Parse-only accessor (additive, for `crate::parse::enumerate`): `true` when this block
    /// uses the 64-bit magnitude path (`precision != BUF32`). The GPU port decodes only the
    /// 32-bit path, so the front-end carries this flag for the planner to loud-reject 64-bit
    /// blocks rather than mistaking truncated values for data (mirrors `capture_debug_record`).
    pub(crate) fn is_64bit(&self) -> bool {
        self.precision != Self::BUF32
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[inline]
    unsafe fn encode_cb32_direct(
        encoder: CbEncoderFun32,
        buf: *mut Ui32,
        missing_msbs: Ui32,
        num_passes: Ui32,
        width: Ui32,
        height: Ui32,
        stride: Ui32,
        lengths: &mut [Ui32; 2],
        elastic: *mut mem_elastic_allocator,
        coded: &mut *mut CodedLists,
    ) -> crate::openjph::ojph_message::OjphResult<()> {
        let enc = encoder as *const ();
        if enc == ojph_encode_codeblock_avx2 as *const () {
            return Self::encode_cb32_avx2_target(
                buf,
                missing_msbs,
                num_passes,
                width,
                height,
                stride,
                lengths,
                elastic,
                coded,
            );
        }
        #[cfg(target_arch = "x86_64")]
        if enc == ojph_encode_codeblock_avx512 as *const () {
            return Self::encode_cb32_avx512_target(
                buf,
                missing_msbs,
                num_passes,
                width,
                height,
                stride,
                lengths,
                elastic,
                coded,
            );
        }
        encoder(
            buf,
            missing_msbs,
            num_passes,
            width,
            height,
            stride,
            lengths,
            elastic,
            coded,
        )
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[target_feature(enable = "avx2")]
    #[inline]
    unsafe fn encode_cb32_avx2_target(
        buf: *mut Ui32,
        missing_msbs: Ui32,
        num_passes: Ui32,
        width: Ui32,
        height: Ui32,
        stride: Ui32,
        lengths: &mut [Ui32; 2],
        elastic: *mut mem_elastic_allocator,
        coded: &mut *mut CodedLists,
    ) -> crate::openjph::ojph_message::OjphResult<()> {
        ojph_encode_codeblock_avx2(
            buf,
            missing_msbs,
            num_passes,
            width,
            height,
            stride,
            lengths,
            elastic,
            coded,
        )
    }

    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx512f")]
    #[inline]
    unsafe fn encode_cb32_avx512_target(
        buf: *mut Ui32,
        missing_msbs: Ui32,
        num_passes: Ui32,
        width: Ui32,
        height: Ui32,
        stride: Ui32,
        lengths: &mut [Ui32; 2],
        elastic: *mut mem_elastic_allocator,
        coded: &mut *mut CodedLists,
    ) -> crate::openjph::ojph_message::OjphResult<()> {
        ojph_encode_codeblock_avx512(
            buf,
            missing_msbs,
            num_passes,
            width,
            height,
            stride,
            lengths,
            elastic,
            coded,
        )
    }

    #[inline]
    unsafe fn resolve_tx_to_cb64(&self) -> TxToCbFun64 {
        debug_assert!(self.codeblock_functions.tx_to_cb64.is_some());
        self.codeblock_functions
            .tx_to_cb64
            .unwrap_or(gen_rev_tx_to_cb64)
    }

    #[inline]
    unsafe fn resolve_tx_from_cb64(&self) -> TxFromCbFun64 {
        debug_assert!(self.codeblock_functions.tx_from_cb64.is_some());
        self.codeblock_functions
            .tx_from_cb64
            .unwrap_or(gen_rev_tx_from_cb64)
    }

    pub fn pre_alloc(
        codestream: &mut LocalCodestream,
        nominal: crate::openjph::ojph_base::Size,
        precision: Ui32,
    ) {
        let allocator = unsafe { &mut *codestream.get_allocator() };
        debug_assert!(BYTE_ALIGNMENT / core::mem::size_of::<Ui32>() as u32 > 1);
        let f = BYTE_ALIGNMENT / core::mem::size_of::<Ui32>() as u32 - 1;
        let stride = (nominal.w + f) & !f;
        if precision <= 32 {
            allocator.pre_alloc_data::<Ui32>(nominal.h as usize * stride as usize, 0);
        } else {
            allocator.pre_alloc_data::<Ui64>(nominal.h as usize * stride as usize, 0);
        }
    }

    pub fn finalize_alloc(
        &mut self,
        codestream: &mut LocalCodestream,
        parent: *mut Subband,
        nominal: crate::openjph::ojph_base::Size,
        cb_size: crate::openjph::ojph_base::Size,
        coded_cb: *mut CodedCbHeader,
        k_max: Ui32,
        line_offset: i32,
        precision: Ui32,
        comp_idx: Ui32,
    ) {
        let allocator = unsafe { &mut *codestream.get_allocator() };
        let f = BYTE_ALIGNMENT / core::mem::size_of::<Ui32>() as u32 - 1;
        self.stride = (nominal.w + f) & !f;
        self.buf_size = self.stride * nominal.h;

        if precision <= 32 {
            self.precision = Self::BUF32;
            self.buf = CodeblockBuf {
                buf32: allocator.post_alloc_data::<Ui32>(self.buf_size as usize, 0),
            };
        } else {
            self.precision = Self::BUF64;
            self.buf = CodeblockBuf {
                buf64: allocator.post_alloc_data::<Ui64>(self.buf_size as usize, 0),
            };
        }

        self.nominal_size = nominal;
        self.cb_size = cb_size;
        self.parent = parent;
        self.line_offset = line_offset;
        self.cur_line = 0;
        self.delta = unsafe { (*parent).get_delta() };
        self.delta_inv = 1.0f32 / self.delta;
        self.k_max = k_max;
        self.max_val = CodeblockMaxVals { max_val64: [0; 4] };
        let coc = unsafe { &*codestream.get_coc(comp_idx) };
        self.reversible = coc.is_reversible();
        self.resilient = codestream.is_resilient();
        self.stripe_causal = coc.get_block_vertical_causality();
        self.is_ht = coc.is_ht();
        self.cmodes = crate::coding::ojph_cmodes::part1(coc.get_block_style());
        self.zero_block = false;
        self.coded_cb = coded_cb;
        self.codeblock_functions.init(self.reversible, codestream.is_trunc_quant());
    }

    /// Refresh the cached quantization (`delta`/`delta_inv`/`k_max`) after an in-tile QCD/QCC
    /// override re-quantized the parent subband. Mirrors the assignment in [`Self::finalize_alloc`]
    /// (the geometry/buffers established there stay valid). For the reversible path `delta` is the
    /// unused subband default (`0.0`), reproducing the value `finalize_alloc` would have cached.
    pub fn requantize(&mut self, k_max: Ui32, delta: f32) {
        self.delta = delta;
        self.delta_inv = 1.0f32 / delta;
        self.k_max = k_max;
    }

    pub fn push(&mut self, line: *mut LineBuf) {
        unsafe {
            if self.precision == Self::BUF32 {
                debug_assert!((*line).flags & LFT_32BIT != 0);
                let sp = if self.reversible {
                    (*line).storage.i32_.add(self.line_offset as usize) as *const core::ffi::c_void
                } else {
                    (*line).storage.f32_.add(self.line_offset as usize) as *const core::ffi::c_void
                };
                let dp = self
                    .buf
                    .buf32
                    .add(self.cur_line as usize * self.stride as usize);
                (self.codeblock_functions.tx_to_cb32)(
                    sp,
                    dp,
                    self.k_max,
                    self.delta_inv,
                    self.cb_size.w,
                    self.max_val.max_val32.as_mut_ptr(),
                );
                self.cur_line += 1;
            } else {
                debug_assert!(self.precision == Self::BUF64);
                debug_assert!((*line).flags & LFT_64BIT != 0);
                let sp =
                    (*line).storage.i64_.add(self.line_offset as usize) as *const core::ffi::c_void;
                let dp = self
                    .buf
                    .buf64
                    .add(self.cur_line as usize * self.stride as usize);
                let tx_to_cb64 = self.resolve_tx_to_cb64();
                tx_to_cb64(
                    sp,
                    dp,
                    self.k_max,
                    self.delta_inv,
                    self.cb_size.w,
                    self.max_val.max_val64.as_mut_ptr(),
                );
                self.cur_line += 1;
            }
        }
    }

    pub fn encode(
        &mut self,
        elastic: *mut mem_elastic_allocator,
    ) -> crate::openjph::ojph_message::OjphResult<()> {
        unsafe {
            if self.precision == Self::BUF32 {
                let mv =
                    (self.codeblock_functions.find_max_val32)(self.max_val.max_val32.as_mut_ptr());
                if mv >= 1u32 << (31 - self.k_max) {
                    (*self.coded_cb).missing_msbs = self.k_max - 1;
                    debug_assert!((*self.coded_cb).missing_msbs > 0);
                    debug_assert!((*self.coded_cb).missing_msbs < self.k_max);
                    (*self.coded_cb).num_passes = 1;
                    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
                    {
                        Self::encode_cb32_direct(
                            self.codeblock_functions.encode_cb32,
                            self.buf.buf32,
                            self.k_max - 1,
                            1,
                            self.cb_size.w,
                            self.cb_size.h,
                            self.stride,
                            &mut (*self.coded_cb).pass_length,
                            elastic,
                            &mut (*self.coded_cb).next_coded,
                        )?;
                    }
                    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
                    (self.codeblock_functions.encode_cb32)(
                        self.buf.buf32,
                        self.k_max - 1,
                        1,
                        self.cb_size.w,
                        self.cb_size.h,
                        self.stride,
                        &mut (*self.coded_cb).pass_length,
                        elastic,
                        &mut (*self.coded_cb).next_coded,
                    )?;
                }
            } else {
                debug_assert!(self.precision == Self::BUF64);
                let mv =
                    (self.codeblock_functions.find_max_val64)(self.max_val.max_val64.as_mut_ptr());
                if mv >= 1u64 << (63 - self.k_max) {
                    (*self.coded_cb).missing_msbs = self.k_max - 1;
                    debug_assert!((*self.coded_cb).missing_msbs > 0);
                    debug_assert!((*self.coded_cb).missing_msbs < self.k_max);
                    (*self.coded_cb).num_passes = 1;
                    (self.codeblock_functions.encode_cb64)(
                        self.buf.buf64,
                        self.k_max - 1,
                        1,
                        self.cb_size.w,
                        self.cb_size.h,
                        self.stride,
                        &mut (*self.coded_cb).pass_length,
                        elastic,
                        &mut (*self.coded_cb).next_coded,
                    )?;
                }
            }
        }
        Ok(())
    }

    pub fn recreate(
        &mut self,
        cb_size: crate::openjph::ojph_base::Size,
        coded_cb: *mut CodedCbHeader,
    ) {
        debug_assert!(cb_size.h * self.stride <= self.buf_size && cb_size.w <= self.stride);
        self.cb_size = cb_size;
        self.coded_cb = coded_cb;
        self.cur_line = 0;
        self.max_val = CodeblockMaxVals { max_val64: [0; 4] };
        self.zero_block = false;
    }

    /// Snapshot the just-decoded code-block — its coded input bytes plus the
    /// reconstructed sign-magnitude output — for the GPU-port oracle
    /// ([`crate::others::debug_extract`]). Only meaningful immediately after a
    /// successful [`Self::decode`]. Debug-only; gated by `debug-extract`.
    ///
    /// # Safety
    /// `self.coded_cb`, its `next_coded` list, and `self.buf` must be the live,
    /// fully-decoded state left by [`Self::decode`] (the call site guarantees this).
    #[cfg(feature = "debug-extract")]
    unsafe fn capture_debug_record(&self) -> crate::others::debug_extract::CodeblockRecord {
        let cb = &*self.coded_cb;
        let len = (cb.pass_length[0] + cb.pass_length[1]) as usize;
        let src = (*cb.next_coded)
            .buf
            .add(CodedCbHeader::PREFIX_BUF_SIZE as usize);
        let mut coded = vec![0u8; len];
        ptr::copy_nonoverlapping(src, coded.as_mut_ptr(), len);

        let w = self.cb_size.w as usize;
        let h = self.cb_size.h as usize;
        let stride = self.stride as usize;
        // The 32-bit `decoded` holds the 32-bit magnitude path; the 64-bit path exceeds `u32`, so
        // its sign-magnitude coefficients go to `decoded64` (the additive oracle tap the GPU
        // `cleanup64` kernel is gated against). Exactly one of the two is populated per block.
        let is_64bit = self.precision != Self::BUF32;
        let mut decoded: Vec<u32> = Vec::new();
        let mut decoded64: Vec<u64> = Vec::new();
        if is_64bit {
            let mut d = vec![0u64; w * h];
            for row in 0..h {
                for col in 0..w {
                    d[row * w + col] = *self.buf.buf64.add(row * stride + col);
                }
            }
            decoded64 = d;
        } else {
            let mut d = vec![0u32; w * h];
            for row in 0..h {
                for col in 0..w {
                    d[row * w + col] = *self.buf.buf32.add(row * stride + col);
                }
            }
            decoded = d;
        }

        crate::others::debug_extract::CodeblockRecord {
            coded,
            missing_msbs: cb.missing_msbs,
            num_passes: cb.num_passes,
            len_cup: cb.pass_length[0],
            len_ref: cb.pass_length[1],
            width: self.cb_size.w,
            height: self.cb_size.h,
            is_64bit,
            decoded,
            decoded64,
            // Filled by debug_extract::record() (u_ext_fired: from the 64-bit decode's own
            // coverage flag; geom: from the subband-set current geometry).
            u_ext_fired: false,
            geom: Default::default(),
        }
    }

    pub fn decode(&mut self, roishift: u8) -> crate::openjph::ojph_message::OjphResult<()> {
        unsafe {
            if (*self.coded_cb).pass_length[0] > 0
                && (*self.coded_cb).num_passes > 0
                && !(*self.coded_cb).next_coded.is_null()
            {
                let result = if !self.is_ht || (*self.coded_cb).orig_j2k {
                    // Part-1 (legacy MQ/EBCOT) block coding, for a Part-1 component or an original
                    // J2K block inside an HT MIXED component. Only 32-bit precision is wired (legacy
                    // J2K bit depths fit); a 64-bit Part-1 block is an unsupported rare case and is
                    // rejected loudly via the `false` below rather than mis-decoded.
                    if self.precision != Self::BUF32 {
                        false
                    } else {
                        let coded_len = (*self.coded_cb).pass_length[0] as usize;
                        let coded = core::slice::from_raw_parts(
                            (*(*self.coded_cb).next_coded)
                                .buf
                                .add(CodedCbHeader::PREFIX_BUF_SIZE as usize),
                            coded_len,
                        );
                        let decoded =
                            core::slice::from_raw_parts_mut(self.buf.buf32, self.buf_size as usize);
                        // RESTART per-pass segment lengths (empty for every other Part-1 block,
                        // which forms a single segment over `pass_length[0]`).
                        let seg_lengths: &[Ui32] = if (*self.coded_cb).seg_count != 0 {
                            core::slice::from_raw_parts(
                                (*self.coded_cb).seg_lengths,
                                (*self.coded_cb).seg_count as usize,
                            )
                        } else {
                            &[]
                        };
                        crate::coding::ojph_block_decoder_part1::decode_codeblock_part1(
                            coded,
                            decoded,
                            self.stride as usize,
                            (*self.coded_cb).missing_msbs,
                            (*self.coded_cb).num_passes,
                            self.cb_size.w as usize,
                            self.cb_size.h as usize,
                            (*self.parent).band_num as u8,
                            self.cmodes,
                            self.reversible,
                            roishift,
                            seg_lengths,
                            self.k_max,
                        )
                    }
                } else if self.precision == Self::BUF32 {
                    (self.codeblock_functions.decode_cb32)(
                        (*(*self.coded_cb).next_coded)
                            .buf
                            .add(CodedCbHeader::PREFIX_BUF_SIZE as usize),
                        self.buf.buf32,
                        (*self.coded_cb).missing_msbs,
                        (*self.coded_cb).num_passes,
                        (*self.coded_cb).pass_length[0],
                        (*self.coded_cb).pass_length[1],
                        self.cb_size.w,
                        self.cb_size.h,
                        self.stride,
                        self.stripe_causal,
                    )
                } else {
                    debug_assert!(self.precision == Self::BUF64);
                    (self.codeblock_functions.decode_cb64)(
                        (*(*self.coded_cb).next_coded)
                            .buf
                            .add(CodedCbHeader::PREFIX_BUF_SIZE as usize),
                        self.buf.buf64,
                        (*self.coded_cb).missing_msbs,
                        (*self.coded_cb).num_passes,
                        (*self.coded_cb).pass_length[0],
                        (*self.coded_cb).pass_length[1],
                        self.cb_size.w,
                        self.cb_size.h,
                        self.stride,
                        self.stripe_causal,
                    )
                };

                // Debug-only oracle tap for the GPU port (`jph-wgpu-rs`): capture this
                // block's exact coded input + decoded output so the GPU entropy kernels
                // (K1 MEL+VLC, K2 MagSgn) can be validated bit-exactly, one code-block at
                // a time. Purely additive, behind the off-by-default `debug-extract`
                // feature — the 1:1 decode path above is untouched.
                #[cfg(feature = "debug-extract")]
                if result {
                    crate::others::debug_extract::record(self.capture_debug_record());
                }

                if !result {
                    if self.resilient {
                        crate::ojph_info!(0x000300A1, "Error decoding a codeblock.");
                        self.zero_block = true;
                    } else {
                        return Err(crate::ojph_error!(0x000300A1, "Error decoding a codeblock."));
                    }
                }
            } else {
                self.zero_block = true;
            }
        }
        Ok(())
    }

    /// Apply the JPEG 2000 ROI max-shift (ITU-T T.800 §H.1) to the just-decoded code-block, in
    /// place on the fixed-point magnitude buffer before [`Self::pull_line`] runs `tx_from_cb`.
    ///
    /// This is the OpenHTJ2K `j2k_dequant_rev_scalar` convention (port target), which differs from
    /// the naïve "shift the foreground down" reading: jph-rs decodes magnitudes against the FULL
    /// bit budget `k_max = M_b` (background precision **plus** the ROI shift), so foreground
    /// coefficients already land in the correct high bitplanes while background coefficients sit
    /// entirely below `pLSB = 31 - k_max`. Those background coefficients (`(mag & !mask) == 0`,
    /// `mask` = the low `pLSB` bits) are the ones scaled **up** by `roishift` to restore their true
    /// magnitude; foreground is left untouched. Must run before the `>> pLSB` reduction in
    /// `tx_from_cb`, hence on the raw buffer here rather than on the final subband coefficient.
    pub fn apply_roi(&mut self, roishift: u8) {
        if self.zero_block || roishift == 0 {
            return;
        }
        let w = self.cb_size.w as usize;
        let h = self.cb_size.h as usize;
        let stride = self.stride as usize;
        unsafe {
            if self.precision == Self::BUF32 {
                let mask: Ui32 = Ui32::MAX >> (self.k_max + 1);
                // Part-1 irreversible blocks defer the reconstruction mid-point to here: under ROI it
                // is a FIXED `1 << (pLSB - 1)` (OpenHTJ2K `j2k_dequant`, N_b = M_b) added to ALL
                // non-zero coefficients AFTER the background up-shift. Reversible and HT blocks keep
                // the up-shift-only behaviour (writing foreground back unchanged is a no-op).
                // Original (Part-1) blocks inside an HT MIXED component are reconstructed like any
                // other Part-1 block, so this is a per-block decision, not a per-component one.
                let recon = !self.reversible && (!self.is_ht || (*self.coded_cb).orig_j2k);
                let plsb = 31i32 - self.k_max as i32;
                for y in 0..h {
                    let row = self.buf.buf32.add(y * stride);
                    for x in 0..w {
                        let p = row.add(x);
                        let v = *p;
                        let mut mag = v & 0x7FFF_FFFF;
                        if (mag & !mask) == 0 {
                            mag <<= roishift;
                        }
                        if recon && mag != 0 && plsb >= 1 {
                            mag |= 1 << (plsb - 1);
                        }
                        *p = (v & 0x8000_0000) | mag;
                    }
                }
            } else {
                debug_assert!(self.precision == Self::BUF64);
                let mask: u64 = u64::MAX >> (self.k_max + 1);
                for y in 0..h {
                    let row = self.buf.buf64.add(y * stride);
                    for x in 0..w {
                        let p = row.add(x);
                        let v = *p;
                        let mag = v & 0x7FFF_FFFF_FFFF_FFFF;
                        if (mag & !mask) == 0 {
                            *p = (v & 0x8000_0000_0000_0000) | (mag << roishift);
                        }
                    }
                }
            }
        }
    }

    pub fn pull_line(&mut self, line: *mut LineBuf) {
        unsafe {
            if self.precision == Self::BUF32 {
                debug_assert!((*line).flags & LFT_32BIT != 0);
                let dp = if self.reversible {
                    (*line).storage.i32_.add(self.line_offset as usize) as *mut core::ffi::c_void
                } else {
                    (*line).storage.f32_.add(self.line_offset as usize) as *mut core::ffi::c_void
                };
                if !self.zero_block {
                    let sp = self
                        .buf
                        .buf32
                        .add(self.cur_line as usize * self.stride as usize);
                    (self.codeblock_functions.tx_from_cb32)(
                        sp,
                        dp,
                        self.k_max,
                        self.delta,
                        self.cb_size.w,
                    );
                } else {
                    (self.codeblock_functions.mem_clear)(
                        dp,
                        self.cb_size.w as usize * core::mem::size_of::<Ui32>(),
                    );
                }
            } else {
                debug_assert!(self.precision == Self::BUF64);
                // A 64-bit code-block feeds an i64 line when reversible, but an f32 line when
                // irreversible (the wavelet always runs in f32 there; `gen_irv_tx_from_cb64` writes
                // floats), so the line's element size differs between the two.
                debug_assert!(
                    (*line).flags & if self.reversible { LFT_64BIT } else { LFT_32BIT } != 0
                );
                let elem = if self.reversible {
                    core::mem::size_of::<Si64>()
                } else {
                    core::mem::size_of::<f32>()
                };
                let dp =
                    (*line).storage.i64_.add(self.line_offset as usize) as *mut core::ffi::c_void;
                if !self.zero_block {
                    let sp = self
                        .buf
                        .buf64
                        .add(self.cur_line as usize * self.stride as usize);
                    let tx_from_cb64 = self.resolve_tx_from_cb64();
                    tx_from_cb64(sp, dp, self.k_max, self.delta, self.cb_size.w);
                } else {
                    (self.codeblock_functions.mem_clear)(dp, self.cb_size.w as usize * elem);
                }
            }
            self.cur_line += 1;
            debug_assert!(self.cur_line <= self.cb_size.h);
        }
    }
}
