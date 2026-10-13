//! Function-pointer dispatch table.

use std::sync::OnceLock;

use crate::arch;

/// Resolved dispatch table for the running CPU.  Append-only field order.
#[derive(Clone, Copy)]
pub struct Dispatch {
    /// See [`crate::sat_pack_i16_to_u8`].
    pub sat_pack_i16_to_u8: unsafe fn(src: *const i16, dst: *mut u8, n: usize),
    /// See [`crate::sat_pack_i32_to_u16`].
    pub sat_pack_i32_to_u16: unsafe fn(src: *const i32, dst: *mut u16, n: usize, max: u16),
    /// See [`crate::add_clip_to_pixel_u16`].
    pub add_clip_to_pixel_u16:
        unsafe fn(pred: *const u16, res: *const i16, dst: *mut u16, n: usize, max: u16),
    /// See [`crate::add_clip_to_pixel_u8`].
    pub add_clip_to_pixel_u8: unsafe fn(pred: *const u8, res: *const i16, dst: *mut u8, n: usize),
    /// See [`crate::clamp_i32_to_i16`].
    pub clamp_i32_to_i16: unsafe fn(buf: *mut i32, n: usize),
    /// See [`crate::abs_diff_u8`].
    pub abs_diff_u8: unsafe fn(a: *const u8, b: *const u8, dst: *mut u8, n: usize),
    /// See [`crate::mac_widen_i16_to_i32`].
    pub mac_widen_i16_to_i32: unsafe fn(a: *const i16, b: *const i16, acc: *mut i32, n: usize),
    /// See [`crate::transpose_4x4_i16`].
    pub transpose_4x4_i16: unsafe fn(src: *const i16, dst: *mut i16),
    /// See [`crate::transpose_8x8_i16`].
    pub transpose_8x8_i16: unsafe fn(src: *const i16, dst: *mut i16),
    /// See [`crate::sat_add_i16`].
    pub sat_add_i16: unsafe fn(a: *const i16, b: *const i16, dst: *mut i16, n: usize),
    /// See [`crate::sat_sub_i16`].
    pub sat_sub_i16: unsafe fn(a: *const i16, b: *const i16, dst: *mut i16, n: usize),
    /// See [`crate::abs_i16`].
    pub abs_i16: unsafe fn(src: *const i16, dst: *mut i16, n: usize),
    /// See [`crate::mul_hi_round_i16`].
    pub mul_hi_round_i16: unsafe fn(a: *const i16, b: *const i16, dst: *mut i16, n: usize),
    /// See [`crate::round_shift_narrow_i32_to_i16`].
    pub round_shift_narrow_i32_to_i16:
        unsafe fn(src: *const i32, dst: *mut i16, n: usize, shift: u32),
    /// See [`crate::sad_block_u8`].
    pub sad_block_u8: unsafe fn(
        a: *const u8,
        a_stride: isize,
        b: *const u8,
        b_stride: isize,
        w: usize,
        h: usize,
    ) -> u32,
    /// See [`crate::satd_block_4x4_u8`].
    pub satd_block_4x4_u8:
        unsafe fn(a: *const u8, a_stride: isize, b: *const u8, b_stride: isize) -> u32,
    /// See [`crate::satd_block_8x8_u8`].
    pub satd_block_8x8_u8:
        unsafe fn(a: *const u8, a_stride: isize, b: *const u8, b_stride: isize) -> u32,
    /// See [`crate::sse_block_u8`].
    pub sse_block_u8: unsafe fn(
        a: *const u8,
        a_stride: isize,
        b: *const u8,
        b_stride: isize,
        w: usize,
        h: usize,
    ) -> u64,
    /// See [`crate::mc_avg_u8`].
    pub mc_avg_u8: unsafe fn(
        a: *const u8,
        a_stride: isize,
        b: *const u8,
        b_stride: isize,
        dst: *mut u8,
        dst_stride: isize,
        w: usize,
        h: usize,
    ),
    /// See [`crate::variance_block_u8`].
    pub variance_block_u8:
        unsafe fn(src: *const u8, stride: isize, w: usize, h: usize) -> (u32, u64),
    /// See [`crate::mc_8tap_h_u8`].
    pub mc_8tap_h_u8: unsafe fn(
        dst: *mut u8,
        dst_stride: isize,
        src: *const u8,
        src_stride: isize,
        w: usize,
        h: usize,
        filter: *const i16,
    ),
    /// See [`crate::mc_8tap_v_u8`].
    pub mc_8tap_v_u8: unsafe fn(
        dst: *mut u8,
        dst_stride: isize,
        src: *const u8,
        src_stride: isize,
        w: usize,
        h: usize,
        filter: *const i16,
    ),
    /// See [`crate::mc_4tap_h_u8`].
    pub mc_4tap_h_u8: unsafe fn(
        dst: *mut u8,
        dst_stride: isize,
        src: *const u8,
        src_stride: isize,
        w: usize,
        h: usize,
        filter: *const i16,
        shift: u32,
    ),
    /// See [`crate::mc_4tap_v_u8`].
    pub mc_4tap_v_u8: unsafe fn(
        dst: *mut u8,
        dst_stride: isize,
        src: *const u8,
        src_stride: isize,
        w: usize,
        h: usize,
        filter: *const i16,
        shift: u32,
    ),
    /// See [`crate::mc_6tap_h_u8`].
    pub mc_6tap_h_u8: unsafe fn(
        dst: *mut u8,
        dst_stride: isize,
        src: *const u8,
        src_stride: isize,
        w: usize,
        h: usize,
        filter: *const i16,
        shift: u32,
    ),
    /// See [`crate::mc_6tap_v_u8`].
    pub mc_6tap_v_u8: unsafe fn(
        dst: *mut u8,
        dst_stride: isize,
        src: *const u8,
        src_stride: isize,
        w: usize,
        h: usize,
        filter: *const i16,
        shift: u32,
    ),
    /// See [`crate::mc_bilinear_h_u8`].
    pub mc_bilinear_h_u8: unsafe fn(
        dst: *mut u8,
        dst_stride: isize,
        src: *const u8,
        src_stride: isize,
        w: usize,
        h: usize,
        filter: *const i16,
        shift: u32,
    ),
    /// See [`crate::mc_bilinear_v_u8`].
    pub mc_bilinear_v_u8: unsafe fn(
        dst: *mut u8,
        dst_stride: isize,
        src: *const u8,
        src_stride: isize,
        w: usize,
        h: usize,
        filter: *const i16,
        shift: u32,
    ),
    /// See [`crate::mc_8tap_h_u8_to_i16`].
    pub mc_8tap_h_u8_to_i16: unsafe fn(
        dst: *mut i16,
        dst_stride: isize,
        src: *const u8,
        src_stride: isize,
        w: usize,
        h: usize,
        filter: *const i16,
        shift: u32,
    ),
    /// See [`crate::mc_8tap_v_i16_to_u8`].
    pub mc_8tap_v_i16_to_u8: unsafe fn(
        dst: *mut u8,
        dst_stride: isize,
        src: *const i16,
        src_stride: isize,
        w: usize,
        h: usize,
        filter: *const i16,
        shift: u32,
    ),
    /// See [`crate::mc_4tap_h_u8_to_i16`].
    pub mc_4tap_h_u8_to_i16: unsafe fn(
        dst: *mut i16,
        dst_stride: isize,
        src: *const u8,
        src_stride: isize,
        w: usize,
        h: usize,
        filter: *const i16,
        shift: u32,
    ),
    /// See [`crate::mc_4tap_v_i16_to_u8`].
    pub mc_4tap_v_i16_to_u8: unsafe fn(
        dst: *mut u8,
        dst_stride: isize,
        src: *const i16,
        src_stride: isize,
        w: usize,
        h: usize,
        filter: *const i16,
        shift: u32,
    ),
    /// See [`crate::mc_6tap_h_u8_to_i16`].
    pub mc_6tap_h_u8_to_i16: unsafe fn(
        dst: *mut i16,
        dst_stride: isize,
        src: *const u8,
        src_stride: isize,
        w: usize,
        h: usize,
        filter: *const i16,
        shift: u32,
    ),
    /// See [`crate::mc_6tap_v_i16_to_u8`].
    pub mc_6tap_v_i16_to_u8: unsafe fn(
        dst: *mut u8,
        dst_stride: isize,
        src: *const i16,
        src_stride: isize,
        w: usize,
        h: usize,
        filter: *const i16,
        shift: u32,
    ),
    /// See [`crate::sad_x4_block_u8`].
    pub sad_x4_block_u8: unsafe fn(
        a: *const u8,
        a_stride: isize,
        b0: *const u8,
        b1: *const u8,
        b2: *const u8,
        b3: *const u8,
        b_stride: isize,
        w: usize,
        h: usize,
    ) -> [u32; 4],
    /// See [`crate::dequant_block_i16`].
    pub dequant_block_i16: unsafe fn(buf: *mut i16, n: usize, scale: i16, shift: u32),
    /// See [`crate::weighted_avg_u8`].
    pub weighted_avg_u8: unsafe fn(
        a: *const u8,
        a_stride: isize,
        b: *const u8,
        b_stride: isize,
        dst: *mut u8,
        dst_stride: isize,
        w: usize,
        h: usize,
        w0: i16,
        w1: i16,
        offset: i32,
        shift: u32,
    ),
    /// See [`crate::lincomb3_clip_u8`].
    pub lincomb3_clip_u8: unsafe fn(
        p0: *const u8,
        p1: *const u8,
        p2: *const u8,
        dst: *mut u8,
        n: usize,
        c0: i16,
        c1: i16,
        c2: i16,
        offset: i32,
        shift: u32,
    ),
    /// See [`crate::hadamard_4x4_i16`].
    pub hadamard_4x4_i16: unsafe fn(src: *const i16, dst: *mut i16),
    /// See [`crate::mc_avg_u16`].
    pub mc_avg_u16: unsafe fn(
        a: *const u16,
        a_stride: isize,
        b: *const u16,
        b_stride: isize,
        dst: *mut u16,
        dst_stride: isize,
        w: usize,
        h: usize,
    ),
    /// See [`crate::weighted_avg_u16`].
    pub weighted_avg_u16: unsafe fn(
        a: *const u16,
        a_stride: isize,
        b: *const u16,
        b_stride: isize,
        dst: *mut u16,
        dst_stride: isize,
        w: usize,
        h: usize,
        w0: i16,
        w1: i16,
        offset: i32,
        shift: u32,
        max: u16,
    ),
    /// See [`crate::rct_fwd_i32`].
    pub rct_fwd_i32: unsafe fn(
        r: *const i32,
        g: *const i32,
        b: *const i32,
        y: *mut i32,
        cb: *mut i32,
        cr: *mut i32,
        n: usize,
    ),
    /// See [`crate::rct_bwd_i32`].
    pub rct_bwd_i32: unsafe fn(
        y: *const i32,
        cb: *const i32,
        cr: *const i32,
        r: *mut i32,
        g: *mut i32,
        b: *mut i32,
        n: usize,
    ),
    /// See [`crate::mc_8tap_h_i32_to_i32`].
    pub mc_8tap_h_i32_to_i32: unsafe fn(
        dst: *mut i32,
        dst_stride: isize,
        src: *const i32,
        src_stride: isize,
        w: usize,
        h: usize,
        filter: *const i8,
        shift: u32,
    ),
    /// See [`crate::mc_8tap_v_i32_to_i32`].
    pub mc_8tap_v_i32_to_i32: unsafe fn(
        dst: *mut i32,
        dst_stride: isize,
        src: *const i32,
        src_stride: isize,
        w: usize,
        h: usize,
        filter: *const i8,
        shift: u32,
    ),
    /// See [`crate::mc_8tap_v_i16_to_i32`].
    pub mc_8tap_v_i16_to_i32: unsafe fn(
        dst: *mut i32,
        dst_stride: isize,
        src: *const i16,
        src_stride: isize,
        w: usize,
        h: usize,
        filter: *const i8,
        shift: u32,
    ),
    /// See [`crate::mc_4tap_h_i32_to_i32`].
    pub mc_4tap_h_i32_to_i32: unsafe fn(
        dst: *mut i32,
        dst_stride: isize,
        src: *const i32,
        src_stride: isize,
        w: usize,
        h: usize,
        filter: *const i8,
        shift: u32,
    ),
    /// See [`crate::mc_4tap_v_i32_to_i32`].
    pub mc_4tap_v_i32_to_i32: unsafe fn(
        dst: *mut i32,
        dst_stride: isize,
        src: *const i32,
        src_stride: isize,
        w: usize,
        h: usize,
        filter: *const i8,
        shift: u32,
    ),
    /// See [`crate::mc_4tap_v_i16_to_i32`].
    pub mc_4tap_v_i16_to_i32: unsafe fn(
        dst: *mut i32,
        dst_stride: isize,
        src: *const i16,
        src_stride: isize,
        w: usize,
        h: usize,
        filter: *const i8,
        shift: u32,
    ),
    /// See [`crate::mc_8tap_h_u16_to_i32`].
    pub mc_8tap_h_u16_to_i32: unsafe fn(
        dst: *mut i32,
        dst_stride: isize,
        src: *const u16,
        src_stride: isize,
        w: usize,
        h: usize,
        filter: *const i8,
        shift: u32,
    ),
    /// See [`crate::mc_8tap_v_u16_to_i32`].
    pub mc_8tap_v_u16_to_i32: unsafe fn(
        dst: *mut i32,
        dst_stride: isize,
        src: *const u16,
        src_stride: isize,
        w: usize,
        h: usize,
        filter: *const i8,
        shift: u32,
    ),
    /// See [`crate::mc_4tap_h_u16_to_i32`].
    pub mc_4tap_h_u16_to_i32: unsafe fn(
        dst: *mut i32,
        dst_stride: isize,
        src: *const u16,
        src_stride: isize,
        w: usize,
        h: usize,
        filter: *const i8,
        shift: u32,
    ),
    /// See [`crate::mc_4tap_v_u16_to_i32`].
    pub mc_4tap_v_u16_to_i32: unsafe fn(
        dst: *mut i32,
        dst_stride: isize,
        src: *const u16,
        src_stride: isize,
        w: usize,
        h: usize,
        filter: *const i8,
        shift: u32,
    ),
    /// See [`crate::mc_uni_clip_i32_to_u16`].
    pub mc_uni_clip_i32_to_u16: unsafe fn(
        dst: *mut u16,
        dst_stride: isize,
        v: *const i32,
        v_stride: isize,
        w: usize,
        h: usize,
        offset: i32,
        shift: u32,
        max: u16,
    ),
    /// See [`crate::mc_uni_w_clip_i32_to_u16`].
    pub mc_uni_w_clip_i32_to_u16: unsafe fn(
        dst: *mut u16,
        dst_stride: isize,
        v: *const i32,
        v_stride: isize,
        w: usize,
        h: usize,
        wx: i32,
        offset: i32,
        shift: u32,
        ox: i32,
        max: u16,
    ),
    /// See [`crate::mc_bi_clip_i32_to_u16`].
    pub mc_bi_clip_i32_to_u16: unsafe fn(
        dst: *mut u16,
        dst_stride: isize,
        v: *const i32,
        v_stride: isize,
        src2: *const i16,
        src2_stride: isize,
        w: usize,
        h: usize,
        offset: i32,
        shift: u32,
        max: u16,
    ),
    /// See [`crate::mc_bi_w_clip_i32_to_u16`].
    pub mc_bi_w_clip_i32_to_u16: unsafe fn(
        dst: *mut u16,
        dst_stride: isize,
        v: *const i32,
        v_stride: isize,
        src2: *const i16,
        src2_stride: isize,
        w: usize,
        h: usize,
        wx0: i32,
        wx1: i32,
        bias: i32,
        rshift: u32,
        max: u16,
    ),
    /// See [`crate::mc_store_i32_to_i16`].
    pub mc_store_i32_to_i16: unsafe fn(
        dst: *mut i16,
        dst_stride: isize,
        v: *const i32,
        v_stride: isize,
        w: usize,
        h: usize,
    ),
    /// See [`crate::hevc_idct4_i16_inplace`].
    pub hevc_idct4_i16_inplace: unsafe fn(coeffs: *mut i16, bit_depth: u8),
    /// See [`crate::hevc_dst4_i16_inplace`].
    pub hevc_dst4_i16_inplace: unsafe fn(coeffs: *mut i16, bit_depth: u8),
    /// See [`crate::hevc_idct_dc_i16_inplace`].
    pub hevc_idct_dc_i16_inplace: unsafe fn(coeffs: *mut i16, log2_size: u32, bit_depth: u8),
    /// See [`crate::hevc_idct8_i16_inplace`].
    pub hevc_idct8_i16_inplace: unsafe fn(coeffs: *mut i16, col_limit: i32, bit_depth: u8),
    /// See [`crate::hevc_idct16_i16_inplace`].
    pub hevc_idct16_i16_inplace: unsafe fn(coeffs: *mut i16, col_limit: i32, bit_depth: u8),
    /// See [`crate::hevc_idct32_i16_inplace`].
    pub hevc_idct32_i16_inplace: unsafe fn(coeffs: *mut i16, col_limit: i32, bit_depth: u8),
    /// See [`crate::sao_band_u16`].
    pub sao_band_u16: unsafe fn(
        dst: *mut u16,
        dst_stride: isize,
        src: *const u16,
        src_stride: isize,
        w: usize,
        h: usize,
        offset_table: *const i16,
        band_shift: u32,
        max: u16,
    ),
    /// See [`crate::sao_edge_u16`].
    pub sao_edge_u16: unsafe fn(
        dst: *mut u16,
        dst_stride: isize,
        src: *const u16,
        src_stride: isize,
        w: usize,
        h: usize,
        eo_class: u32,
        offsets: *const i16,
        max: u16,
    ),
    /// See [`crate::hevc_intra_planar_u16`].
    pub hevc_intra_planar_u16: unsafe fn(
        dst: *mut u16,
        dst_stride: isize,
        top: *const u16,
        left: *const u16,
        log2_size: u32,
    ),
    /// See [`crate::hevc_intra_angular_u16`].
    pub hevc_intra_angular_u16: unsafe fn(
        dst: *mut u16,
        dst_stride: isize,
        ref_buf: *const i32,
        bias: usize,
        size: usize,
        angle: i32,
        vertical: bool,
    ),
    /// See [`crate::hevc_deblock_luma_strong_u16`].
    pub hevc_deblock_luma_strong_u16: unsafe fn(
        plane: *mut u16,
        base: usize,
        xstride: isize,
        ystride: isize,
        tc: i32,
        tc2: i32,
        tc3: i32,
        max: i32,
    ),
    /// See [`crate::hevc_deblock_luma_weak_u16`].
    pub hevc_deblock_luma_weak_u16: unsafe fn(
        plane: *mut u16,
        base: usize,
        xstride: isize,
        ystride: isize,
        tc: i32,
        nd_p: i32,
        nd_q: i32,
        max: i32,
    ),
    /// See [`crate::hevc_deblock_chroma_weak_u16`].
    pub hevc_deblock_chroma_weak_u16: unsafe fn(
        plane: *mut u16,
        base: usize,
        xstride: isize,
        ystride: isize,
        size: usize,
        tc: i32,
        max: i32,
    ),
    /// See [`crate::sad_block_u16`].
    pub sad_block_u16: unsafe fn(
        a: *const u16,
        a_stride: isize,
        b: *const u16,
        b_stride: isize,
        w: usize,
        h: usize,
    ) -> u32,
    /// See [`crate::satd_block_4x4_u16`].
    pub satd_block_4x4_u16:
        unsafe fn(a: *const u16, a_stride: isize, b: *const u16, b_stride: isize) -> u32,
    /// See [`crate::satd_block_8x8_u16`].
    pub satd_block_8x8_u16:
        unsafe fn(a: *const u16, a_stride: isize, b: *const u16, b_stride: isize) -> u32,
    /// See [`crate::sse_block_u16`].
    pub sse_block_u16: unsafe fn(
        a: *const u16,
        a_stride: isize,
        b: *const u16,
        b_stride: isize,
        w: usize,
        h: usize,
    ) -> u64,
    /// See [`crate::prefix_sum_bias_u8_inplace`].
    pub prefix_sum_bias_u8_inplace: unsafe fn(buf: *mut u8, n: usize),
    /// See [`crate::interleave_halves_u8`].
    pub interleave_halves_u8: unsafe fn(t1: *const u8, t2: *const u8, out: *mut u8, n: usize),
}

impl std::fmt::Debug for Dispatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dispatch")
            .field("cpu_level", &arch::cpu_level_name(arch::cpu_level()))
            .finish()
    }
}

static DISPATCH: OnceLock<Dispatch> = OnceLock::new();

pub(crate) fn get() -> &'static Dispatch {
    DISPATCH.get_or_init(Dispatch::detect)
}

impl Dispatch {
    pub fn detect() -> Self {
        let level = arch::cpu_level();
        let mut d = Self::scalar();

        #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
        {
            if level >= arch::X86_SSE2 {
                d.sat_pack_i16_to_u8 = crate::prims_sse2::sat_pack_i16_to_u8;
                d.abs_diff_u8 = crate::prims_sse2::abs_diff_u8;
                d.transpose_4x4_i16 = crate::prims_sse2::transpose_4x4_i16;
                d.transpose_8x8_i16 = crate::prims_sse2::transpose_8x8_i16;
                d.sat_add_i16 = crate::prims_sse2::sat_add_i16;
                d.sat_sub_i16 = crate::prims_sse2::sat_sub_i16;
                d.abs_i16 = crate::prims_sse2::abs_i16;
                d.mul_hi_round_i16 = crate::prims_sse2::mul_hi_round_i16;
                d.round_shift_narrow_i32_to_i16 = crate::prims_sse2::round_shift_narrow_i32_to_i16;
                d.sad_block_u8 = crate::prims_sse2::sad_block_u8;
                d.satd_block_4x4_u8 = crate::prims_sse2::satd_block_4x4_u8;
                d.sse_block_u8 = crate::prims_sse2::sse_block_u8;
                d.mc_avg_u8 = crate::prims_sse2::mc_avg_u8;
                d.variance_block_u8 = crate::prims_sse2::variance_block_u8;
                d.mc_8tap_h_u8 = crate::prims_sse2::mc_8tap_h_u8;
                d.mc_8tap_v_u8 = crate::prims_sse2::mc_8tap_v_u8;
                d.mc_4tap_h_u8 = crate::prims_sse2::mc_4tap_h_u8;
                d.mc_4tap_v_u8 = crate::prims_sse2::mc_4tap_v_u8;
                d.mc_6tap_h_u8 = crate::prims_sse2::mc_6tap_h_u8;
                d.mc_6tap_v_u8 = crate::prims_sse2::mc_6tap_v_u8;
                d.mc_bilinear_h_u8 = crate::prims_sse2::mc_bilinear_h_u8;
                d.mc_bilinear_v_u8 = crate::prims_sse2::mc_bilinear_v_u8;
                d.mc_8tap_h_u8_to_i16 = crate::prims_sse2::mc_8tap_h_u8_to_i16;
                d.mc_8tap_v_i16_to_u8 = crate::prims_sse2::mc_8tap_v_i16_to_u8;
                d.mc_4tap_h_u8_to_i16 = crate::prims_sse2::mc_4tap_h_u8_to_i16;
                d.mc_4tap_v_i16_to_u8 = crate::prims_sse2::mc_4tap_v_i16_to_u8;
                d.mc_6tap_h_u8_to_i16 = crate::prims_sse2::mc_6tap_h_u8_to_i16;
                d.mc_6tap_v_i16_to_u8 = crate::prims_sse2::mc_6tap_v_i16_to_u8;
                d.sad_x4_block_u8 = crate::prims_sse2::sad_x4_block_u8;
                d.dequant_block_i16 = crate::prims_sse2::dequant_block_i16;
                d.weighted_avg_u8 = crate::prims_sse2::weighted_avg_u8;
                d.lincomb3_clip_u8 = crate::prims_sse2::lincomb3_clip_u8;
                d.hadamard_4x4_i16 = crate::prims_sse2::hadamard_4x4_i16;
                d.mc_avg_u16 = crate::prims_sse2::mc_avg_u16;
                d.rct_fwd_i32 = crate::prims_sse2::rct_fwd_i32;
                d.rct_bwd_i32 = crate::prims_sse2::rct_bwd_i32;
                d.mc_8tap_h_i32_to_i32 = crate::prims_sse2::mc_8tap_h_i32_to_i32;
                d.mc_8tap_v_i32_to_i32 = crate::prims_sse2::mc_8tap_v_i32_to_i32;
                d.mc_8tap_v_i16_to_i32 = crate::prims_sse2::mc_8tap_v_i16_to_i32;
                d.mc_4tap_h_i32_to_i32 = crate::prims_sse2::mc_4tap_h_i32_to_i32;
                d.mc_4tap_v_i32_to_i32 = crate::prims_sse2::mc_4tap_v_i32_to_i32;
                d.mc_4tap_v_i16_to_i32 = crate::prims_sse2::mc_4tap_v_i16_to_i32;
                d.mc_8tap_h_u16_to_i32 = crate::prims_sse2::mc_8tap_h_u16_to_i32;
                d.mc_8tap_v_u16_to_i32 = crate::prims_sse2::mc_8tap_v_u16_to_i32;
                d.mc_4tap_h_u16_to_i32 = crate::prims_sse2::mc_4tap_h_u16_to_i32;
                d.mc_4tap_v_u16_to_i32 = crate::prims_sse2::mc_4tap_v_u16_to_i32;
                d.hevc_idct4_i16_inplace = crate::prims_sse2::hevc_idct4_i16_inplace;
                d.hevc_dst4_i16_inplace = crate::prims_sse2::hevc_dst4_i16_inplace;
                d.hevc_idct_dc_i16_inplace = crate::prims_sse2::hevc_idct_dc_i16_inplace;
                d.hevc_idct8_i16_inplace = crate::prims_sse2::hevc_idct8_i16_inplace;
                d.hevc_idct16_i16_inplace = crate::prims_sse2::hevc_idct16_i16_inplace;
                d.hevc_idct32_i16_inplace = crate::prims_sse2::hevc_idct32_i16_inplace;
                d.sao_edge_u16 = crate::prims_sse2::sao_edge_u16;
                d.hevc_intra_planar_u16 = crate::prims_sse2::hevc_intra_planar_u16;
                d.hevc_intra_angular_u16 = crate::prims_sse2::hevc_intra_angular_u16;
                d.hevc_deblock_luma_strong_u16 = crate::prims_sse2::hevc_deblock_luma_strong_u16;
                d.hevc_deblock_luma_weak_u16 = crate::prims_sse2::hevc_deblock_luma_weak_u16;
                d.hevc_deblock_chroma_weak_u16 = crate::prims_sse2::hevc_deblock_chroma_weak_u16;
                d.interleave_halves_u8 = crate::prims_sse2::interleave_halves_u8;
            }
            if level >= arch::X86_SSE41 {
                d.sat_pack_i32_to_u16 = crate::prims_sse41::sat_pack_i32_to_u16;
                d.weighted_avg_u16 = crate::prims_sse41::weighted_avg_u16;
                d.add_clip_to_pixel_u16 = crate::prims_sse41::add_clip_to_pixel_u16;
                d.add_clip_to_pixel_u8 = crate::prims_sse41::add_clip_to_pixel_u8;
                d.clamp_i32_to_i16 = crate::prims_sse41::clamp_i32_to_i16;
                d.mac_widen_i16_to_i32 = crate::prims_sse41::mac_widen_i16_to_i32;
                d.mul_hi_round_i16 = crate::prims_sse41::mul_hi_round_i16;
                d.mc_uni_clip_i32_to_u16 = crate::prims_sse41::mc_uni_clip_i32_to_u16;
                d.mc_uni_w_clip_i32_to_u16 = crate::prims_sse41::mc_uni_w_clip_i32_to_u16;
                d.mc_bi_clip_i32_to_u16 = crate::prims_sse41::mc_bi_clip_i32_to_u16;
                d.mc_bi_w_clip_i32_to_u16 = crate::prims_sse41::mc_bi_w_clip_i32_to_u16;
                d.mc_store_i32_to_i16 = crate::prims_sse41::mc_store_i32_to_i16;
                d.prefix_sum_bias_u8_inplace = crate::prims_sse41::prefix_sum_bias_u8_inplace;
            }
            if level >= arch::X86_AVX2 {
                d.sat_pack_i16_to_u8 = crate::prims_avx2::sat_pack_i16_to_u8;
                d.sat_pack_i32_to_u16 = crate::prims_avx2::sat_pack_i32_to_u16;
                d.add_clip_to_pixel_u16 = crate::prims_avx2::add_clip_to_pixel_u16;
                d.add_clip_to_pixel_u8 = crate::prims_avx2::add_clip_to_pixel_u8;
                d.clamp_i32_to_i16 = crate::prims_avx2::clamp_i32_to_i16;
                d.abs_diff_u8 = crate::prims_avx2::abs_diff_u8;
                d.mac_widen_i16_to_i32 = crate::prims_avx2::mac_widen_i16_to_i32;
                d.sat_add_i16 = crate::prims_avx2::sat_add_i16;
                d.sat_sub_i16 = crate::prims_avx2::sat_sub_i16;
                d.abs_i16 = crate::prims_avx2::abs_i16;
                d.mul_hi_round_i16 = crate::prims_avx2::mul_hi_round_i16;
                d.round_shift_narrow_i32_to_i16 = crate::prims_avx2::round_shift_narrow_i32_to_i16;
                d.sad_block_u8 = crate::prims_avx2::sad_block_u8;
                d.sse_block_u8 = crate::prims_avx2::sse_block_u8;
                d.mc_avg_u8 = crate::prims_avx2::mc_avg_u8;
                d.mc_avg_u16 = crate::prims_avx2::mc_avg_u16;
                d.rct_fwd_i32 = crate::prims_avx2::rct_fwd_i32;
                d.rct_bwd_i32 = crate::prims_avx2::rct_bwd_i32;
                d.variance_block_u8 = crate::prims_avx2::variance_block_u8;
                d.mc_8tap_h_u8 = crate::prims_avx2::mc_8tap_h_u8;
                d.mc_8tap_v_u8 = crate::prims_avx2::mc_8tap_v_u8;
                d.mc_4tap_h_u8 = crate::prims_avx2::mc_4tap_h_u8;
                d.mc_4tap_v_u8 = crate::prims_avx2::mc_4tap_v_u8;
                d.mc_6tap_h_u8 = crate::prims_avx2::mc_6tap_h_u8;
                d.mc_6tap_v_u8 = crate::prims_avx2::mc_6tap_v_u8;
                d.mc_bilinear_h_u8 = crate::prims_avx2::mc_bilinear_h_u8;
                d.mc_bilinear_v_u8 = crate::prims_avx2::mc_bilinear_v_u8;
                d.mc_8tap_h_u8_to_i16 = crate::prims_avx2::mc_8tap_h_u8_to_i16;
                d.mc_8tap_v_i16_to_u8 = crate::prims_avx2::mc_8tap_v_i16_to_u8;
                d.mc_4tap_h_u8_to_i16 = crate::prims_avx2::mc_4tap_h_u8_to_i16;
                d.mc_4tap_v_i16_to_u8 = crate::prims_avx2::mc_4tap_v_i16_to_u8;
                d.mc_6tap_h_u8_to_i16 = crate::prims_avx2::mc_6tap_h_u8_to_i16;
                d.mc_6tap_v_i16_to_u8 = crate::prims_avx2::mc_6tap_v_i16_to_u8;
                d.sad_x4_block_u8 = crate::prims_avx2::sad_x4_block_u8;
                d.dequant_block_i16 = crate::prims_avx2::dequant_block_i16;
                d.weighted_avg_u8 = crate::prims_avx2::weighted_avg_u8;
                d.lincomb3_clip_u8 = crate::prims_avx2::lincomb3_clip_u8;
                d.mc_8tap_h_i32_to_i32 = crate::prims_avx2::mc_8tap_h_i32_to_i32;
                d.mc_8tap_v_i32_to_i32 = crate::prims_avx2::mc_8tap_v_i32_to_i32;
                d.mc_8tap_v_i16_to_i32 = crate::prims_avx2::mc_8tap_v_i16_to_i32;
                d.mc_4tap_h_i32_to_i32 = crate::prims_avx2::mc_4tap_h_i32_to_i32;
                d.mc_4tap_v_i32_to_i32 = crate::prims_avx2::mc_4tap_v_i32_to_i32;
                d.mc_4tap_v_i16_to_i32 = crate::prims_avx2::mc_4tap_v_i16_to_i32;
                d.mc_8tap_h_u16_to_i32 = crate::prims_avx2::mc_8tap_h_u16_to_i32;
                d.mc_8tap_v_u16_to_i32 = crate::prims_avx2::mc_8tap_v_u16_to_i32;
                d.mc_4tap_h_u16_to_i32 = crate::prims_avx2::mc_4tap_h_u16_to_i32;
                d.mc_4tap_v_u16_to_i32 = crate::prims_avx2::mc_4tap_v_u16_to_i32;
                d.mc_uni_clip_i32_to_u16 = crate::prims_avx2::mc_uni_clip_i32_to_u16;
                d.mc_uni_w_clip_i32_to_u16 = crate::prims_avx2::mc_uni_w_clip_i32_to_u16;
                d.mc_bi_clip_i32_to_u16 = crate::prims_avx2::mc_bi_clip_i32_to_u16;
                d.mc_bi_w_clip_i32_to_u16 = crate::prims_avx2::mc_bi_w_clip_i32_to_u16;
                d.mc_store_i32_to_i16 = crate::prims_avx2::mc_store_i32_to_i16;
                d.hevc_idct4_i16_inplace = crate::prims_avx2::hevc_idct4_i16_inplace;
                d.hevc_dst4_i16_inplace = crate::prims_avx2::hevc_dst4_i16_inplace;
                d.hevc_idct_dc_i16_inplace = crate::prims_avx2::hevc_idct_dc_i16_inplace;
                d.hevc_idct8_i16_inplace = crate::prims_avx2::hevc_idct8_i16_inplace;
                d.hevc_idct16_i16_inplace = crate::prims_avx2::hevc_idct16_i16_inplace;
                d.hevc_idct32_i16_inplace = crate::prims_avx2::hevc_idct32_i16_inplace;
                d.sao_edge_u16 = crate::prims_avx2::sao_edge_u16;
                d.hevc_intra_planar_u16 = crate::prims_avx2::hevc_intra_planar_u16;
                d.hevc_intra_angular_u16 = crate::prims_avx2::hevc_intra_angular_u16;
                d.hevc_deblock_luma_strong_u16 = crate::prims_avx2::hevc_deblock_luma_strong_u16;
                d.hevc_deblock_luma_weak_u16 = crate::prims_avx2::hevc_deblock_luma_weak_u16;
                d.hevc_deblock_chroma_weak_u16 = crate::prims_avx2::hevc_deblock_chroma_weak_u16;
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            if level >= arch::ARM_NEON {
                d.sat_pack_i16_to_u8 = crate::prims_neon::sat_pack_i16_to_u8;
                d.sat_pack_i32_to_u16 = crate::prims_neon::sat_pack_i32_to_u16;
                d.add_clip_to_pixel_u16 = crate::prims_neon::add_clip_to_pixel_u16;
                d.add_clip_to_pixel_u8 = crate::prims_neon::add_clip_to_pixel_u8;
                d.clamp_i32_to_i16 = crate::prims_neon::clamp_i32_to_i16;
                d.abs_diff_u8 = crate::prims_neon::abs_diff_u8;
                d.mac_widen_i16_to_i32 = crate::prims_neon::mac_widen_i16_to_i32;
                d.transpose_4x4_i16 = crate::prims_neon::transpose_4x4_i16;
                d.transpose_8x8_i16 = crate::prims_neon::transpose_8x8_i16;
                d.sat_add_i16 = crate::prims_neon::sat_add_i16;
                d.sat_sub_i16 = crate::prims_neon::sat_sub_i16;
                d.abs_i16 = crate::prims_neon::abs_i16;
                d.mul_hi_round_i16 = crate::prims_neon::mul_hi_round_i16;
                d.round_shift_narrow_i32_to_i16 = crate::prims_neon::round_shift_narrow_i32_to_i16;
                d.sad_block_u8 = crate::prims_neon::sad_block_u8;
                d.satd_block_4x4_u8 = crate::prims_neon::satd_block_4x4_u8;
                d.satd_block_8x8_u8 = crate::prims_neon::satd_block_8x8_u8;
                d.sse_block_u8 = crate::prims_neon::sse_block_u8;
                d.mc_avg_u8 = crate::prims_neon::mc_avg_u8;
                d.variance_block_u8 = crate::prims_neon::variance_block_u8;
                d.mc_8tap_h_u8 = crate::prims_neon::mc_8tap_h_u8;
                d.mc_8tap_v_u8 = crate::prims_neon::mc_8tap_v_u8;
                d.mc_4tap_h_u8 = crate::prims_neon::mc_4tap_h_u8;
                d.mc_4tap_v_u8 = crate::prims_neon::mc_4tap_v_u8;
                d.mc_6tap_h_u8 = crate::prims_neon::mc_6tap_h_u8;
                d.mc_6tap_v_u8 = crate::prims_neon::mc_6tap_v_u8;
                d.mc_bilinear_h_u8 = crate::prims_neon::mc_bilinear_h_u8;
                d.mc_bilinear_v_u8 = crate::prims_neon::mc_bilinear_v_u8;
                d.mc_8tap_h_u8_to_i16 = crate::prims_neon::mc_8tap_h_u8_to_i16;
                d.mc_8tap_v_i16_to_u8 = crate::prims_neon::mc_8tap_v_i16_to_u8;
                d.mc_4tap_h_u8_to_i16 = crate::prims_neon::mc_4tap_h_u8_to_i16;
                d.mc_4tap_v_i16_to_u8 = crate::prims_neon::mc_4tap_v_i16_to_u8;
                d.mc_6tap_h_u8_to_i16 = crate::prims_neon::mc_6tap_h_u8_to_i16;
                d.mc_6tap_v_i16_to_u8 = crate::prims_neon::mc_6tap_v_i16_to_u8;
                d.sad_x4_block_u8 = crate::prims_neon::sad_x4_block_u8;
                d.dequant_block_i16 = crate::prims_neon::dequant_block_i16;
                d.weighted_avg_u8 = crate::prims_neon::weighted_avg_u8;
                d.lincomb3_clip_u8 = crate::prims_neon::lincomb3_clip_u8;
                d.hadamard_4x4_i16 = crate::prims_neon::hadamard_4x4_i16;
                d.mc_avg_u16 = crate::prims_neon::mc_avg_u16;
                d.rct_fwd_i32 = crate::prims_neon::rct_fwd_i32;
                d.rct_bwd_i32 = crate::prims_neon::rct_bwd_i32;
                d.prefix_sum_bias_u8_inplace = crate::prims_neon::prefix_sum_bias_u8_inplace;
                d.interleave_halves_u8 = crate::prims_neon::interleave_halves_u8;
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            if level >= arch::WASM_SIMD128 {
                d.sat_pack_i16_to_u8 = crate::prims_wasm::sat_pack_i16_to_u8;
                d.sat_pack_i32_to_u16 = crate::prims_wasm::sat_pack_i32_to_u16;
                d.add_clip_to_pixel_u16 = crate::prims_wasm::add_clip_to_pixel_u16;
                d.add_clip_to_pixel_u8 = crate::prims_wasm::add_clip_to_pixel_u8;
                d.clamp_i32_to_i16 = crate::prims_wasm::clamp_i32_to_i16;
                d.abs_diff_u8 = crate::prims_wasm::abs_diff_u8;
                d.mac_widen_i16_to_i32 = crate::prims_wasm::mac_widen_i16_to_i32;
                d.transpose_4x4_i16 = crate::prims_wasm::transpose_4x4_i16;
                d.transpose_8x8_i16 = crate::prims_wasm::transpose_8x8_i16;
                d.sat_add_i16 = crate::prims_wasm::sat_add_i16;
                d.sat_sub_i16 = crate::prims_wasm::sat_sub_i16;
                d.abs_i16 = crate::prims_wasm::abs_i16;
                d.mul_hi_round_i16 = crate::prims_wasm::mul_hi_round_i16;
                d.sad_block_u8 = crate::prims_wasm::sad_block_u8;
                d.satd_block_4x4_u8 = crate::prims_wasm::satd_block_4x4_u8;
                d.sse_block_u8 = crate::prims_wasm::sse_block_u8;
                d.mc_avg_u8 = crate::prims_wasm::mc_avg_u8;
                d.variance_block_u8 = crate::prims_wasm::variance_block_u8;
                d.mc_8tap_h_u8 = crate::prims_wasm::mc_8tap_h_u8;
                d.mc_8tap_v_u8 = crate::prims_wasm::mc_8tap_v_u8;
                d.mc_4tap_h_u8 = crate::prims_wasm::mc_4tap_h_u8;
                d.mc_4tap_v_u8 = crate::prims_wasm::mc_4tap_v_u8;
                d.mc_6tap_h_u8 = crate::prims_wasm::mc_6tap_h_u8;
                d.mc_6tap_v_u8 = crate::prims_wasm::mc_6tap_v_u8;
                d.mc_bilinear_h_u8 = crate::prims_wasm::mc_bilinear_h_u8;
                d.mc_bilinear_v_u8 = crate::prims_wasm::mc_bilinear_v_u8;
                d.mc_8tap_h_u8_to_i16 = crate::prims_wasm::mc_8tap_h_u8_to_i16;
                d.mc_8tap_v_i16_to_u8 = crate::prims_wasm::mc_8tap_v_i16_to_u8;
                d.mc_4tap_h_u8_to_i16 = crate::prims_wasm::mc_4tap_h_u8_to_i16;
                d.mc_4tap_v_i16_to_u8 = crate::prims_wasm::mc_4tap_v_i16_to_u8;
                d.mc_6tap_h_u8_to_i16 = crate::prims_wasm::mc_6tap_h_u8_to_i16;
                d.mc_6tap_v_i16_to_u8 = crate::prims_wasm::mc_6tap_v_i16_to_u8;
                d.sad_x4_block_u8 = crate::prims_wasm::sad_x4_block_u8;
                d.dequant_block_i16 = crate::prims_wasm::dequant_block_i16;
                d.weighted_avg_u8 = crate::prims_wasm::weighted_avg_u8;
                d.lincomb3_clip_u8 = crate::prims_wasm::lincomb3_clip_u8;
                d.hadamard_4x4_i16 = crate::prims_wasm::hadamard_4x4_i16;
                d.mc_avg_u16 = crate::prims_wasm::mc_avg_u16;
                d.rct_fwd_i32 = crate::prims_wasm::rct_fwd_i32;
                d.rct_bwd_i32 = crate::prims_wasm::rct_bwd_i32;
            }
        }

        let _ = level;
        d
    }

    pub fn scalar() -> Self {
        Self {
            sat_pack_i16_to_u8: crate::prims_scalar::sat_pack_i16_to_u8,
            sat_pack_i32_to_u16: crate::prims_scalar::sat_pack_i32_to_u16,
            add_clip_to_pixel_u16: crate::prims_scalar::add_clip_to_pixel_u16,
            add_clip_to_pixel_u8: crate::prims_scalar::add_clip_to_pixel_u8,
            clamp_i32_to_i16: crate::prims_scalar::clamp_i32_to_i16,
            abs_diff_u8: crate::prims_scalar::abs_diff_u8,
            mac_widen_i16_to_i32: crate::prims_scalar::mac_widen_i16_to_i32,
            transpose_4x4_i16: crate::prims_scalar::transpose_4x4_i16,
            transpose_8x8_i16: crate::prims_scalar::transpose_8x8_i16,
            sat_add_i16: crate::prims_scalar::sat_add_i16,
            sat_sub_i16: crate::prims_scalar::sat_sub_i16,
            abs_i16: crate::prims_scalar::abs_i16,
            mul_hi_round_i16: crate::prims_scalar::mul_hi_round_i16,
            round_shift_narrow_i32_to_i16: crate::prims_scalar::round_shift_narrow_i32_to_i16,
            sad_block_u8: crate::prims_scalar::sad_block_u8,
            satd_block_4x4_u8: crate::prims_scalar::satd_block_4x4_u8,
            satd_block_8x8_u8: crate::prims_scalar::satd_block_8x8_u8,
            sse_block_u8: crate::prims_scalar::sse_block_u8,
            mc_avg_u8: crate::prims_scalar::mc_avg_u8,
            variance_block_u8: crate::prims_scalar::variance_block_u8,
            mc_8tap_h_u8: crate::prims_scalar::mc_8tap_h_u8,
            mc_8tap_v_u8: crate::prims_scalar::mc_8tap_v_u8,
            mc_4tap_h_u8: crate::prims_scalar::mc_4tap_h_u8,
            mc_4tap_v_u8: crate::prims_scalar::mc_4tap_v_u8,
            mc_6tap_h_u8: crate::prims_scalar::mc_6tap_h_u8,
            mc_6tap_v_u8: crate::prims_scalar::mc_6tap_v_u8,
            mc_bilinear_h_u8: crate::prims_scalar::mc_bilinear_h_u8,
            mc_bilinear_v_u8: crate::prims_scalar::mc_bilinear_v_u8,
            mc_8tap_h_u8_to_i16: crate::prims_scalar::mc_8tap_h_u8_to_i16,
            mc_8tap_v_i16_to_u8: crate::prims_scalar::mc_8tap_v_i16_to_u8,
            mc_4tap_h_u8_to_i16: crate::prims_scalar::mc_4tap_h_u8_to_i16,
            mc_4tap_v_i16_to_u8: crate::prims_scalar::mc_4tap_v_i16_to_u8,
            mc_6tap_h_u8_to_i16: crate::prims_scalar::mc_6tap_h_u8_to_i16,
            mc_6tap_v_i16_to_u8: crate::prims_scalar::mc_6tap_v_i16_to_u8,
            sad_x4_block_u8: crate::prims_scalar::sad_x4_block_u8,
            dequant_block_i16: crate::prims_scalar::dequant_block_i16,
            weighted_avg_u8: crate::prims_scalar::weighted_avg_u8,
            lincomb3_clip_u8: crate::prims_scalar::lincomb3_clip_u8,
            hadamard_4x4_i16: crate::prims_scalar::hadamard_4x4_i16,
            mc_avg_u16: crate::prims_scalar::mc_avg_u16,
            weighted_avg_u16: crate::prims_scalar::weighted_avg_u16,
            rct_fwd_i32: crate::prims_scalar::rct_fwd_i32,
            rct_bwd_i32: crate::prims_scalar::rct_bwd_i32,
            // HEVC luma i32→i32 MC (v0.3.22).  NEON/wasm fall through to the
            // scalar reference (bit-exact by construction): a hand-tuned SIMD
            // backfill for those ISAs is deferred rather than shipped untested,
            // per CAVEMAN's documented scalar-fallthrough allowance.
            mc_8tap_h_i32_to_i32: crate::prims_scalar::mc_8tap_h_i32_to_i32,
            mc_8tap_v_i32_to_i32: crate::prims_scalar::mc_8tap_v_i32_to_i32,
            mc_8tap_v_i16_to_i32: crate::prims_scalar::mc_8tap_v_i16_to_i32,
            // HEVC chroma 4-tap i32->i32 MC (v0.3.23).  Same NEON/wasm
            // scalar-fallthrough as the luma i32 family above.
            mc_4tap_h_i32_to_i32: crate::prims_scalar::mc_4tap_h_i32_to_i32,
            mc_4tap_v_i32_to_i32: crate::prims_scalar::mc_4tap_v_i32_to_i32,
            mc_4tap_v_i16_to_i32: crate::prims_scalar::mc_4tap_v_i16_to_i32,
            // HEVC u16-source MC (v0.3.27): the u16 siblings of the i32-source
            // luma/chroma tap filters (2x SIMD lanes).  SSE2 (mullo/mulhi) +
            // AVX2 (`cvtepu16_epi32`) host-tested; NEON/wasm scalar-fallthrough.
            mc_8tap_h_u16_to_i32: crate::prims_scalar::mc_8tap_h_u16_to_i32,
            mc_8tap_v_u16_to_i32: crate::prims_scalar::mc_8tap_v_u16_to_i32,
            mc_4tap_h_u16_to_i32: crate::prims_scalar::mc_4tap_h_u16_to_i32,
            mc_4tap_v_u16_to_i32: crate::prims_scalar::mc_4tap_v_u16_to_i32,
            // HEVC MC final clip/combine (v0.3.26).  SSE4.1 + AVX2 backends
            // (i32->u16 clamp+narrow wants `packus_epi32`/`mullo_epi32`, which
            // are SSE4.1 — same tier as the crate's `add_clip_to_pixel_u16` /
            // `weighted_avg_u16`).  SSE2 / NEON / wasm fall through to scalar.
            mc_uni_clip_i32_to_u16: crate::prims_scalar::mc_uni_clip_i32_to_u16,
            mc_uni_w_clip_i32_to_u16: crate::prims_scalar::mc_uni_w_clip_i32_to_u16,
            mc_bi_clip_i32_to_u16: crate::prims_scalar::mc_bi_clip_i32_to_u16,
            mc_bi_w_clip_i32_to_u16: crate::prims_scalar::mc_bi_w_clip_i32_to_u16,
            mc_store_i32_to_i16: crate::prims_scalar::mc_store_i32_to_i16,
            // HEVC inverse transforms — size 4 (v0.3.28).  Scalar reference +
            // SSE2 (mullo/mulhi widen, across-columns butterfly + transpose) +
            // AVX2 (native mullo_epi32/cvtepi16_epi32/min_epi32).  NEON/wasm
            // fall through to scalar (SIMD backfill deferred, per CAVEMAN).
            hevc_idct4_i16_inplace: crate::prims_scalar::hevc_idct4_i16_inplace,
            hevc_dst4_i16_inplace: crate::prims_scalar::hevc_dst4_i16_inplace,
            hevc_idct_dc_i16_inplace: crate::prims_scalar::hevc_idct_dc_i16_inplace,
            // HEVC inverse transforms — sizes 8/16/32 (v0.3.29).  Scalar port +
            // SSE2 (4-column i32 butterfly) + AVX2 (8-column).  NEON/wasm fall
            // through to scalar (SIMD backfill deferred, per CAVEMAN).
            hevc_idct8_i16_inplace: crate::prims_scalar::hevc_idct8_i16_inplace,
            hevc_idct16_i16_inplace: crate::prims_scalar::hevc_idct16_i16_inplace,
            hevc_idct32_i16_inplace: crate::prims_scalar::hevc_idct32_i16_inplace,
            // HEVC SAO band + edge on the u16 plane (v0.3.30).  Band is a
            // 32-entry per-pixel LUT gather kept scalar on every tier (a SIMD
            // scalar-gather does not beat it); edge gets SSE2/AVX2 i16-lane
            // classify backends.  NEON/wasm fall through to scalar.
            sao_band_u16: crate::prims_scalar::sao_band_u16,
            sao_edge_u16: crate::prims_scalar::sao_edge_u16,
            // HEVC intra prediction — planar + angular FILL kernels on the u16
            // plane (v0.3.31). Both accumulate in i32 (10/12-bit safe); u16
            // narrow only at the store (output ∈ [0, 2^bd)). SSE2 (mullo/mulhi
            // widen) + AVX2 (native mullo_epi32) 8-wide interior + scalar tail;
            // NEON/wasm scalar-fallthrough (the x86 SIMD modules are cfg-gated).
            hevc_intra_planar_u16: crate::prims_scalar::hevc_intra_planar_u16,
            hevc_intra_angular_u16: crate::prims_scalar::hevc_intra_angular_u16,
            // HEVC in-loop deblocking-filter kernels on the u16 plane (v0.3.32).
            // i32-lane arithmetic (one lane per line); orientation folded into
            // the SIMD gather/scatter so ONE kernel serves both vertical and
            // horizontal edges. Scalar reference + SSE2 (emulated min/max) +
            // AVX2 (native min/max); NEON/wasm scalar-fallthrough.
            hevc_deblock_luma_strong_u16: crate::prims_scalar::hevc_deblock_luma_strong_u16,
            hevc_deblock_luma_weak_u16: crate::prims_scalar::hevc_deblock_luma_weak_u16,
            hevc_deblock_chroma_weak_u16: crate::prims_scalar::hevc_deblock_chroma_weak_u16,
            // HEVC encoder-cost u16 siblings (Tier-D scaffold, encoder SAD/SATD on
            // the u16 plane). Scalar on every tier for now: a hand-tuned SIMD
            // backfill is deferred rather than shipped untested (CAVEMAN
            // scalar-fallthrough allowance), so `detect()` leaves them untouched.
            sad_block_u16: crate::prims_scalar::sad_block_u16,
            satd_block_4x4_u16: crate::prims_scalar::satd_block_4x4_u16,
            satd_block_8x8_u16: crate::prims_scalar::satd_block_8x8_u16,
            sse_block_u16: crate::prims_scalar::sse_block_u16,
            // OpenEXR ZIP/RLE predictor decode-side primitives (v0.3.35).
            // SSE4.1-tier (SSSE3 instructions) + NEON backends; wasm falls
            // through to scalar (SIMD backfill deferred, per CAVEMAN).
            prefix_sum_bias_u8_inplace: crate::prims_scalar::prefix_sum_bias_u8_inplace,
            interleave_halves_u8: crate::prims_scalar::interleave_halves_u8,
        }
    }
}
