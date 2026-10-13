//! Packing formatters (`Pack*` in `cmspack.c`): working `wOut[]` → pixel buffer.
//!
//! See the [module docs](super) for the generic-vs-specialised strategy. Each function is a
//! direct port of the like-named C routine; the C name is given in each doc comment.

use super::{
    fom_lab_v4_to_v2, is_ink_space, pixel_size, rd_u16, wr_f32, wr_f64, wr_u16,
    MAX_ENCODEABLE_XYZ,
};
use crate::color::pcs::{
    cms_float_to_lab_encoded, cms_float_to_lab_encoded_v2, cms_lab_encoded_to_float,
    cms_xyz_encoded_to_float,
};
use crate::types::color::CIELab;
use crate::types::format::PixelFormat;
use crate::types::num::{from_16_to_8, from_8_to_16, quick_saturate_word, to_fixed_domain};

// ============================================================================================
// Generic 16-bit packers (chunky + planar), 8/16-bit samples
// ============================================================================================

/// `PackChunkyBytes` — the generic byte packer (doswap/flavor/swapfirst/extra/premul).
pub fn pack_chunky_bytes(fmt: PixelFormat, w_out: &[u16], dst: &mut [u8], _stride: usize) -> usize {
    let n_chan = fmt.channels() as usize;
    let do_swap = fmt.doswap();
    let reverse = fmt.flavor();
    let extra = fmt.extra() as usize;
    let swap_first = fmt.swapfirst();
    let premul = fmt.premul();
    let extra_first = do_swap ^ swap_first;

    let mut pos = 0usize;
    let mut alpha_factor: u32 = 0;
    if extra_first {
        if premul && extra > 0 {
            alpha_factor = to_fixed_domain(from_8_to_16(dst[0]) as i32) as u32;
        }
        pos += extra;
    } else if premul && extra > 0 {
        alpha_factor = to_fixed_domain(from_8_to_16(dst[n_chan]) as i32) as u32;
    }

    let mut last_v: u16 = 0;
    for i in 0..n_chan {
        let index = if do_swap { n_chan - i - 1 } else { i };
        let mut v = w_out[index];
        if reverse {
            v = 0xffff - v;
        }
        if premul {
            v = (((v as u32) * alpha_factor + 0x8000) >> 16) as u16;
        }
        dst[pos] = from_16_to_8(v);
        last_v = v;
        pos += 1;
    }

    if !extra_first {
        pos += extra;
    }

    if extra == 0 && swap_first {
        // memmove(swap1+1, swap1, nChan-1); *swap1 = FROM_16_TO_8(v)
        dst.copy_within(0..n_chan - 1, 1);
        dst[0] = from_16_to_8(last_v);
    }

    pos
}

/// `PackChunkyWords` — the generic word packer (endian16/doswap/flavor/swapfirst/extra/premul).
pub fn pack_chunky_words(fmt: PixelFormat, w_out: &[u16], dst: &mut [u8], _stride: usize) -> usize {
    let n_chan = fmt.channels() as usize;
    let swap_endian = fmt.endian16();
    let do_swap = fmt.doswap();
    let reverse = fmt.flavor();
    let extra = fmt.extra() as usize;
    let swap_first = fmt.swapfirst();
    let premul = fmt.premul();
    let extra_first = do_swap ^ swap_first;

    let mut pos = 0usize;
    let mut alpha_factor: u32 = 0;
    if extra_first {
        if premul && extra > 0 {
            alpha_factor = to_fixed_domain(rd_u16(dst, 0) as i32) as u32;
        }
        pos += extra * 2;
    } else if premul && extra > 0 {
        alpha_factor = to_fixed_domain(rd_u16(dst, n_chan * 2) as i32) as u32;
    }

    let mut last_v: u16 = 0;
    for i in 0..n_chan {
        let index = if do_swap { n_chan - i - 1 } else { i };
        let mut v = w_out[index];
        if swap_endian {
            v = v.swap_bytes();
        }
        if reverse {
            v = 0xffff - v;
        }
        if premul {
            v = (((v as u32) * alpha_factor + 0x8000) >> 16) as u16;
        }
        wr_u16(dst, pos, v);
        last_v = v;
        pos += 2;
    }

    if !extra_first {
        pos += extra * 2;
    }

    if extra == 0 && swap_first {
        dst.copy_within(0..(n_chan - 1) * 2, 2);
        wr_u16(dst, 0, last_v);
    }

    pos
}

/// `PackPlanarBytes` — generic byte packer for planar layouts. Returns one sample (1).
pub fn pack_planar_bytes(fmt: PixelFormat, w_out: &[u16], dst: &mut [u8], stride: usize) -> usize {
    let n_chan = fmt.channels() as usize;
    let do_swap = fmt.doswap();
    let swap_first = fmt.swapfirst();
    let reverse = fmt.flavor();
    let extra = fmt.extra() as usize;
    let premul = fmt.premul();
    let extra_first = do_swap ^ swap_first;

    let mut base = 0usize;
    let mut alpha_factor: u32 = 0;
    if extra_first {
        if premul && extra > 0 {
            alpha_factor = to_fixed_domain(from_8_to_16(dst[0]) as i32) as u32;
        }
        base += extra * stride;
    } else if premul && extra > 0 {
        alpha_factor = to_fixed_domain(from_8_to_16(dst[n_chan * stride]) as i32) as u32;
    }

    for i in 0..n_chan {
        let index = if do_swap { n_chan - i - 1 } else { i };
        let mut v = w_out[index];
        if reverse {
            v = 0xffff - v;
        }
        if premul {
            v = (((v as u32) * alpha_factor + 0x8000) >> 16) as u16;
        }
        dst[base] = from_16_to_8(v);
        base += stride;
    }

    1
}

/// `PackPlanarWords` — generic planar word packer. Returns one sample (2).
pub fn pack_planar_words(fmt: PixelFormat, w_out: &[u16], dst: &mut [u8], stride: usize) -> usize {
    let n_chan = fmt.channels() as usize;
    let do_swap = fmt.doswap();
    let swap_first = fmt.swapfirst();
    let reverse = fmt.flavor();
    let extra = fmt.extra() as usize;
    let premul = fmt.premul();
    let swap_endian = fmt.endian16();
    let extra_first = do_swap ^ swap_first;

    let mut base = 0usize;
    let mut alpha_factor: u32 = 0;
    if extra_first {
        if premul && extra > 0 {
            alpha_factor = to_fixed_domain(rd_u16(dst, 0) as i32) as u32;
        }
        base += extra * stride;
    } else if premul && extra > 0 {
        // C: `((u16*)output)[nChan*Stride]` — u16 index nChan*Stride → byte 2*nChan*Stride.
        alpha_factor = to_fixed_domain(rd_u16(dst, 2 * n_chan * stride) as i32) as u32;
    }

    for i in 0..n_chan {
        let index = if do_swap { n_chan - i - 1 } else { i };
        let mut v = w_out[index];
        if swap_endian {
            v = v.swap_bytes();
        }
        if reverse {
            v = 0xffff - v;
        }
        if premul {
            v = (((v as u32) * alpha_factor + 0x8000) >> 16) as u16;
        }
        wr_u16(dst, base, v);
        base += stride;
    }

    2
}

// ============================================================================================
// LabV2 8/16-bit packers (FomLabV4ToLabV2)
// ============================================================================================

/// `PackLabV2_8` — V4 working Lab → V2-encoded 8-bit Lab.
pub fn pack_lab_v2_8(_fmt: PixelFormat, w_out: &[u16], dst: &mut [u8], _stride: usize) -> usize {
    dst[0] = from_16_to_8(fom_lab_v4_to_v2(w_out[0]));
    dst[1] = from_16_to_8(fom_lab_v4_to_v2(w_out[1]));
    dst[2] = from_16_to_8(fom_lab_v4_to_v2(w_out[2]));
    3
}

/// `PackALabV2_8` — as [`pack_lab_v2_8`] with a leading (unwritten) alpha byte.
pub fn pack_a_lab_v2_8(_fmt: PixelFormat, w_out: &[u16], dst: &mut [u8], _stride: usize) -> usize {
    dst[1] = from_16_to_8(fom_lab_v4_to_v2(w_out[0]));
    dst[2] = from_16_to_8(fom_lab_v4_to_v2(w_out[1]));
    dst[3] = from_16_to_8(fom_lab_v4_to_v2(w_out[2]));
    4
}

/// `PackLabV2_16` — V4 working Lab → V2-encoded 16-bit Lab.
pub fn pack_lab_v2_16(_fmt: PixelFormat, w_out: &[u16], dst: &mut [u8], _stride: usize) -> usize {
    wr_u16(dst, 0, fom_lab_v4_to_v2(w_out[0]));
    wr_u16(dst, 2, fom_lab_v4_to_v2(w_out[1]));
    wr_u16(dst, 4, fom_lab_v4_to_v2(w_out[2]));
    6
}

// ============================================================================================
// "Optimized" byte packers (low-byte truncation). See module docs: never selected in the
// stock path (OPTIMIZED is masked off before matching), retained for table fidelity.
// ============================================================================================

/// `Pack3BytesOptimized` — low-byte truncation of three channels.
pub fn pack_3_bytes_optimized(
    _fmt: PixelFormat,
    w_out: &[u16],
    dst: &mut [u8],
    _stride: usize,
) -> usize {
    dst[0] = (w_out[0] & 0xFF) as u8;
    dst[1] = (w_out[1] & 0xFF) as u8;
    dst[2] = (w_out[2] & 0xFF) as u8;
    3
}

/// `Pack3BytesSwapOptimized` — reversed [`pack_3_bytes_optimized`].
pub fn pack_3_bytes_swap_optimized(
    _fmt: PixelFormat,
    w_out: &[u16],
    dst: &mut [u8],
    _stride: usize,
) -> usize {
    dst[0] = (w_out[2] & 0xFF) as u8;
    dst[1] = (w_out[1] & 0xFF) as u8;
    dst[2] = (w_out[0] & 0xFF) as u8;
    3
}

/// `Pack3BytesAndSkip1Optimized` — [`pack_3_bytes_optimized`] then skip one byte.
pub fn pack_3_bytes_and_skip1_optimized(
    _fmt: PixelFormat,
    w_out: &[u16],
    dst: &mut [u8],
    _stride: usize,
) -> usize {
    dst[0] = (w_out[0] & 0xFF) as u8;
    dst[1] = (w_out[1] & 0xFF) as u8;
    dst[2] = (w_out[2] & 0xFF) as u8;
    4
}

/// `Pack3BytesAndSkip1SwapFirstOptimized` — leading skip, then three low bytes.
pub fn pack_3_bytes_and_skip1_swap_first_optimized(
    _fmt: PixelFormat,
    w_out: &[u16],
    dst: &mut [u8],
    _stride: usize,
) -> usize {
    dst[1] = (w_out[0] & 0xFF) as u8;
    dst[2] = (w_out[1] & 0xFF) as u8;
    dst[3] = (w_out[2] & 0xFF) as u8;
    4
}

/// `Pack3BytesAndSkip1SwapOptimized` — leading skip, then three reversed low bytes.
pub fn pack_3_bytes_and_skip1_swap_optimized(
    _fmt: PixelFormat,
    w_out: &[u16],
    dst: &mut [u8],
    _stride: usize,
) -> usize {
    dst[1] = (w_out[2] & 0xFF) as u8;
    dst[2] = (w_out[1] & 0xFF) as u8;
    dst[3] = (w_out[0] & 0xFF) as u8;
    4
}

/// `Pack3BytesAndSkip1SwapSwapFirstOptimized` — three reversed low bytes, then skip one.
pub fn pack_3_bytes_and_skip1_swap_swap_first_optimized(
    _fmt: PixelFormat,
    w_out: &[u16],
    dst: &mut [u8],
    _stride: usize,
) -> usize {
    dst[0] = (w_out[2] & 0xFF) as u8;
    dst[1] = (w_out[1] & 0xFF) as u8;
    dst[2] = (w_out[0] & 0xFF) as u8;
    4
}

// ============================================================================================
// 16-bit → float/double/half packers (generic, ink-aware)
// ============================================================================================

/// `PackDoubleFrom16` — `u16` working samples → `f64` (ink-aware, planar/chunky).
pub fn pack_double_from_16(fmt: PixelFormat, w_out: &[u16], dst: &mut [u8], stride: usize) -> usize {
    let n_chan = fmt.channels() as usize;
    let do_swap = fmt.doswap();
    let reverse = fmt.flavor();
    let extra = fmt.extra() as usize;
    let swap_first = fmt.swapfirst();
    let planar = fmt.planar();
    let extra_first = do_swap ^ swap_first;
    let maximum = if is_ink_space(fmt) { 655.35 } else { 65535.0 };
    let stride_el = stride / pixel_size(fmt);
    let start = if extra_first { extra } else { 0 };

    let mut last_v: f64 = 0.0;
    for i in 0..n_chan {
        let index = if do_swap { n_chan - i - 1 } else { i };
        let mut v = w_out[index] as f64 / maximum;
        if reverse {
            v = maximum - v;
        }
        let off = if planar { (i + start) * stride_el } else { i + start };
        wr_f64(dst, off * 8, v);
        last_v = v;
    }

    if extra == 0 && swap_first {
        dst.copy_within(0..(n_chan - 1) * 8, 8);
        wr_f64(dst, 0, last_v);
    }

    if planar { 8 } else { (n_chan + extra) * 8 }
}

/// `PackFloatFrom16` — `u16` working samples → `f32` (ink-aware, planar/chunky).
pub fn pack_float_from_16(fmt: PixelFormat, w_out: &[u16], dst: &mut [u8], stride: usize) -> usize {
    let n_chan = fmt.channels() as usize;
    let do_swap = fmt.doswap();
    let reverse = fmt.flavor();
    let extra = fmt.extra() as usize;
    let swap_first = fmt.swapfirst();
    let planar = fmt.planar();
    let extra_first = do_swap ^ swap_first;
    let maximum = if is_ink_space(fmt) { 655.35 } else { 65535.0 };
    let stride_el = stride / pixel_size(fmt);
    let start = if extra_first { extra } else { 0 };

    let mut last_v: f64 = 0.0;
    for i in 0..n_chan {
        let index = if do_swap { n_chan - i - 1 } else { i };
        let mut v = w_out[index] as f64 / maximum;
        if reverse {
            v = maximum - v;
        }
        let off = if planar { (i + start) * stride_el } else { i + start };
        wr_f32(dst, off * 4, v as f32);
        last_v = v;
    }

    if extra == 0 && swap_first {
        dst.copy_within(0..(n_chan - 1) * 4, 4);
        wr_f32(dst, 0, last_v as f32);
    }

    if planar { 4 } else { (n_chan + extra) * 4 }
}

/// `PackHalfFrom16` — `u16` working samples → IEEE-754 half (ink-aware, planar/chunky).
#[cfg(feature = "half-tables")]
pub fn pack_half_from_16(fmt: PixelFormat, w_out: &[u16], dst: &mut [u8], stride: usize) -> usize {
    use crate::math::half::float_to_half;

    let n_chan = fmt.channels() as usize;
    let do_swap = fmt.doswap();
    let reverse = fmt.flavor();
    let extra = fmt.extra() as usize;
    let swap_first = fmt.swapfirst();
    let planar = fmt.planar();
    let extra_first = do_swap ^ swap_first;
    let maximum: f32 = if is_ink_space(fmt) { 655.35 } else { 65535.0 };
    let stride_el = stride / pixel_size(fmt);
    let start = if extra_first { extra } else { 0 };

    let mut last_v: f32 = 0.0;
    for i in 0..n_chan {
        let index = if do_swap { n_chan - i - 1 } else { i };
        let mut v = w_out[index] as f32 / maximum;
        if reverse {
            v = maximum - v;
        }
        let off = if planar { (i + start) * stride_el } else { i + start };
        wr_u16(dst, off * 2, float_to_half(v));
        last_v = v;
    }

    if extra == 0 && swap_first {
        dst.copy_within(0..(n_chan - 1) * 2, 2);
        wr_u16(dst, 0, float_to_half(last_v));
    }

    if planar { 2 } else { (n_chan + extra) * 2 }
}

// ============================================================================================
// Lab/XYZ double/float packers from 16-bit working samples (PCS decodings)
// ============================================================================================

/// `PackLabDoubleFrom16` — V4 working Lab → `cmsCIELab` `f64`.
///
/// NOTE: the C planar branch does **not** divide `Stride` by `PixelSize` (unlike the XYZ /
/// Lab-float packers), so the a/b planes land at element index `Stride` (byte `Stride*8`).
/// This lcms inconsistency is reproduced verbatim.
pub fn pack_lab_double_from_16(
    fmt: PixelFormat,
    w_out: &[u16],
    dst: &mut [u8],
    stride: usize,
) -> usize {
    let lab = cms_lab_encoded_to_float(&[w_out[0], w_out[1], w_out[2]]);
    if fmt.planar() {
        wr_f64(dst, 0, lab.l);
        wr_f64(dst, stride * 8, lab.a);
        wr_f64(dst, stride * 2 * 8, lab.b);
        8
    } else {
        wr_f64(dst, 0, lab.l);
        wr_f64(dst, 8, lab.a);
        wr_f64(dst, 16, lab.b);
        24 + fmt.extra() as usize * 8
    }
}

/// `PackLabFloatFrom16` — V4 working Lab → `cmsCIELab` `f32`.
pub fn pack_lab_float_from_16(
    fmt: PixelFormat,
    w_out: &[u16],
    dst: &mut [u8],
    stride: usize,
) -> usize {
    let lab = cms_lab_encoded_to_float(&[w_out[0], w_out[1], w_out[2]]);
    if fmt.planar() {
        let stride_el = stride / pixel_size(fmt);
        wr_f32(dst, 0, lab.l as f32);
        wr_f32(dst, stride_el * 4, lab.a as f32);
        wr_f32(dst, stride_el * 2 * 4, lab.b as f32);
        4
    } else {
        wr_f32(dst, 0, lab.l as f32);
        wr_f32(dst, 4, lab.a as f32);
        wr_f32(dst, 8, lab.b as f32);
        (3 + fmt.extra() as usize) * 4
    }
}

/// `PackXYZDoubleFrom16` — 1.15-fixed working XYZ → `cmsCIEXYZ` `f64`.
pub fn pack_xyz_double_from_16(
    fmt: PixelFormat,
    w_out: &[u16],
    dst: &mut [u8],
    stride: usize,
) -> usize {
    let xyz = cms_xyz_encoded_to_float(&[w_out[0], w_out[1], w_out[2]]);
    if fmt.planar() {
        let stride_el = stride / pixel_size(fmt);
        wr_f64(dst, 0, xyz.x);
        wr_f64(dst, stride_el * 8, xyz.y);
        wr_f64(dst, stride_el * 2 * 8, xyz.z);
        8
    } else {
        wr_f64(dst, 0, xyz.x);
        wr_f64(dst, 8, xyz.y);
        wr_f64(dst, 16, xyz.z);
        24 + fmt.extra() as usize * 8
    }
}

/// `PackXYZFloatFrom16` — 1.15-fixed working XYZ → `cmsCIEXYZ` `f32`.
pub fn pack_xyz_float_from_16(
    fmt: PixelFormat,
    w_out: &[u16],
    dst: &mut [u8],
    stride: usize,
) -> usize {
    let xyz = cms_xyz_encoded_to_float(&[w_out[0], w_out[1], w_out[2]]);
    if fmt.planar() {
        let stride_el = stride / pixel_size(fmt);
        wr_f32(dst, 0, xyz.x as f32);
        wr_f32(dst, stride_el * 4, xyz.y as f32);
        wr_f32(dst, stride_el * 2 * 4, xyz.z as f32);
        4
    } else {
        wr_f32(dst, 0, xyz.x as f32);
        wr_f32(dst, 4, xyz.y as f32);
        wr_f32(dst, 8, xyz.z as f32);
        3 * 4 + fmt.extra() as usize * 4
    }
}

// ============================================================================================
// Float-path packers (working f32 → pixel buffer)
// ============================================================================================

/// `PackBytesFromFloat` — working `f32` → 8-bit samples. (No `Stride`/`PixelSize` scaling.)
pub fn pack_bytes_from_float(
    fmt: PixelFormat,
    w_out: &[f32],
    dst: &mut [u8],
    stride: usize,
) -> usize {
    let n_chan = fmt.channels() as usize;
    let do_swap = fmt.doswap();
    let reverse = fmt.flavor();
    let extra = fmt.extra() as usize;
    let swap_first = fmt.swapfirst();
    let planar = fmt.planar();
    let extra_first = do_swap ^ swap_first;
    let start = if extra_first { extra } else { 0 };

    let mut last_vv: u8 = 0;
    for i in 0..n_chan {
        let index = if do_swap { n_chan - i - 1 } else { i };
        let mut v = w_out[index] as f64 * 65535.0;
        if reverse {
            v = 65535.0 - v;
        }
        let vv = from_16_to_8(quick_saturate_word(v));
        let off = if planar { (i + start) * stride } else { i + start };
        dst[off] = vv;
        last_vv = vv;
    }

    if extra == 0 && swap_first {
        dst.copy_within(0..n_chan - 1, 1);
        dst[0] = last_vv;
    }

    if planar { 1 } else { n_chan + extra }
}

/// `PackWordsFromFloat` — working `f32` → 16-bit samples.
pub fn pack_words_from_float(
    fmt: PixelFormat,
    w_out: &[f32],
    dst: &mut [u8],
    stride: usize,
) -> usize {
    let n_chan = fmt.channels() as usize;
    let do_swap = fmt.doswap();
    let reverse = fmt.flavor();
    let extra = fmt.extra() as usize;
    let swap_first = fmt.swapfirst();
    let planar = fmt.planar();
    let extra_first = do_swap ^ swap_first;
    let stride_el = stride / 2; // C: `Stride /= 2;`
    let start = if extra_first { extra } else { 0 };

    let mut last_vv: u16 = 0;
    for i in 0..n_chan {
        let index = if do_swap { n_chan - i - 1 } else { i };
        let mut v = w_out[index] as f64 * 65535.0;
        if reverse {
            v = 65535.0 - v;
        }
        let vv = quick_saturate_word(v);
        let off = if planar { (i + start) * stride_el } else { i + start };
        wr_u16(dst, off * 2, vv);
        last_vv = vv;
    }

    if extra == 0 && swap_first {
        dst.copy_within(0..(n_chan - 1) * 2, 2);
        wr_u16(dst, 0, last_vv);
    }

    if planar { 2 } else { (n_chan + extra) * 2 }
}

/// `PackFloatsFromFloat` — working `f32` → `f32` samples (ink-aware).
pub fn pack_floats_from_float(
    fmt: PixelFormat,
    w_out: &[f32],
    dst: &mut [u8],
    stride: usize,
) -> usize {
    let n_chan = fmt.channels() as usize;
    let do_swap = fmt.doswap();
    let reverse = fmt.flavor();
    let extra = fmt.extra() as usize;
    let swap_first = fmt.swapfirst();
    let planar = fmt.planar();
    let extra_first = do_swap ^ swap_first;
    let maximum = if is_ink_space(fmt) { 100.0 } else { 1.0 };
    let stride_el = stride / pixel_size(fmt);
    let start = if extra_first { extra } else { 0 };

    let mut last_v: f64 = 0.0;
    for i in 0..n_chan {
        let index = if do_swap { n_chan - i - 1 } else { i };
        let mut v = w_out[index] as f64 * maximum;
        if reverse {
            v = maximum - v;
        }
        let off = if planar { (i + start) * stride_el } else { i + start };
        wr_f32(dst, off * 4, v as f32);
        last_v = v;
    }

    if extra == 0 && swap_first {
        dst.copy_within(0..(n_chan - 1) * 4, 4);
        wr_f32(dst, 0, last_v as f32);
    }

    if planar { 4 } else { (n_chan + extra) * 4 }
}

/// `PackDoublesFromFloat` — working `f32` → `f64` samples (ink-aware).
pub fn pack_doubles_from_float(
    fmt: PixelFormat,
    w_out: &[f32],
    dst: &mut [u8],
    stride: usize,
) -> usize {
    let n_chan = fmt.channels() as usize;
    let do_swap = fmt.doswap();
    let reverse = fmt.flavor();
    let extra = fmt.extra() as usize;
    let swap_first = fmt.swapfirst();
    let planar = fmt.planar();
    let extra_first = do_swap ^ swap_first;
    let maximum = if is_ink_space(fmt) { 100.0 } else { 1.0 };
    let stride_el = stride / pixel_size(fmt);
    let start = if extra_first { extra } else { 0 };

    let mut last_v: f64 = 0.0;
    for i in 0..n_chan {
        let index = if do_swap { n_chan - i - 1 } else { i };
        let mut v = w_out[index] as f64 * maximum;
        if reverse {
            v = maximum - v;
        }
        let off = if planar { (i + start) * stride_el } else { i + start };
        wr_f64(dst, off * 8, v);
        last_v = v;
    }

    if extra == 0 && swap_first {
        dst.copy_within(0..(n_chan - 1) * 8, 8);
        wr_f64(dst, 0, last_v);
    }

    if planar { 8 } else { (n_chan + extra) * 8 }
}

/// `PackHalfFromFloat` — working `f32` → IEEE-754 half samples (ink-aware).
#[cfg(feature = "half-tables")]
pub fn pack_half_from_float(
    fmt: PixelFormat,
    w_out: &[f32],
    dst: &mut [u8],
    stride: usize,
) -> usize {
    use crate::math::half::float_to_half;

    let n_chan = fmt.channels() as usize;
    let do_swap = fmt.doswap();
    let reverse = fmt.flavor();
    let extra = fmt.extra() as usize;
    let swap_first = fmt.swapfirst();
    let planar = fmt.planar();
    let extra_first = do_swap ^ swap_first;
    let maximum: f32 = if is_ink_space(fmt) { 100.0 } else { 1.0 };
    let stride_el = stride / pixel_size(fmt);
    let start = if extra_first { extra } else { 0 };

    let mut last_v: f32 = 0.0;
    for i in 0..n_chan {
        let index = if do_swap { n_chan - i - 1 } else { i };
        let mut v = w_out[index] * maximum;
        if reverse {
            v = maximum - v;
        }
        let off = if planar { (i + start) * stride_el } else { i + start };
        wr_u16(dst, off * 2, float_to_half(v));
        last_v = v;
    }

    if extra == 0 && swap_first {
        dst.copy_within(0..(n_chan - 1) * 2, 2);
        wr_u16(dst, 0, float_to_half(last_v));
    }

    if planar { 2 } else { (n_chan + extra) * 2 }
}

// ============================================================================================
// Lab/XYZ float/double packers from working f32 (PCS encodings)
// ============================================================================================

/// `PackLabFloatFromFloat` — normalised `0..1` working Lab → `cmsCIELab` `f32`.
pub fn pack_lab_float_from_float(
    fmt: PixelFormat,
    w_out: &[f32],
    dst: &mut [u8],
    stride: usize,
) -> usize {
    if fmt.planar() {
        let stride_el = stride / pixel_size(fmt);
        wr_f32(dst, 0, (w_out[0] as f64 * 100.0) as f32);
        wr_f32(dst, stride_el * 4, (w_out[1] as f64 * 255.0 - 128.0) as f32);
        wr_f32(dst, stride_el * 2 * 4, (w_out[2] as f64 * 255.0 - 128.0) as f32);
        4
    } else {
        wr_f32(dst, 0, (w_out[0] as f64 * 100.0) as f32);
        wr_f32(dst, 4, (w_out[1] as f64 * 255.0 - 128.0) as f32);
        wr_f32(dst, 8, (w_out[2] as f64 * 255.0 - 128.0) as f32);
        3 * 4 + fmt.extra() as usize * 4
    }
}

/// `PackLabDoubleFromFloat` — normalised `0..1` working Lab → `cmsCIELab` `f64`.
pub fn pack_lab_double_from_float(
    fmt: PixelFormat,
    w_out: &[f32],
    dst: &mut [u8],
    stride: usize,
) -> usize {
    if fmt.planar() {
        let stride_el = stride / pixel_size(fmt);
        wr_f64(dst, 0, w_out[0] as f64 * 100.0);
        wr_f64(dst, stride_el * 8, w_out[1] as f64 * 255.0 - 128.0);
        wr_f64(dst, stride_el * 2 * 8, w_out[2] as f64 * 255.0 - 128.0);
        8
    } else {
        wr_f64(dst, 0, w_out[0] as f64 * 100.0);
        wr_f64(dst, 8, w_out[1] as f64 * 255.0 - 128.0);
        wr_f64(dst, 16, w_out[2] as f64 * 255.0 - 128.0);
        8 * 3 + fmt.extra() as usize * 8
    }
}

/// `PackEncodedBytesLabV2FromFloat` — working Lab → V4-encoded Lab, high byte written.
///
/// NOTE: despite the `V2` name, the C source calls `cmsFloat2LabEncoded` (V4) here (only
/// [`pack_encoded_words_lab_v2_from_float`] uses the V2 encoder). Reproduced verbatim.
pub fn pack_encoded_bytes_lab_v2_from_float(
    fmt: PixelFormat,
    w_out: &[f32],
    dst: &mut [u8],
    stride: usize,
) -> usize {
    let lab = CIELab {
        l: w_out[0] as f64 * 100.0,
        a: w_out[1] as f64 * 255.0 - 128.0,
        b: w_out[2] as f64 * 255.0 - 128.0,
    };
    let wlab = cms_float_to_lab_encoded(&lab);
    if fmt.planar() {
        let stride_el = stride / pixel_size(fmt);
        dst[0] = (wlab[0] >> 8) as u8;
        dst[stride_el] = (wlab[1] >> 8) as u8;
        dst[stride_el * 2] = (wlab[2] >> 8) as u8;
        1
    } else {
        dst[0] = (wlab[0] >> 8) as u8;
        dst[1] = (wlab[1] >> 8) as u8;
        dst[2] = (wlab[2] >> 8) as u8;
        3 + fmt.extra() as usize
    }
}

/// `PackEncodedWordsLabV2FromFloat` — working Lab → V2-encoded 16-bit Lab.
pub fn pack_encoded_words_lab_v2_from_float(
    fmt: PixelFormat,
    w_out: &[f32],
    dst: &mut [u8],
    stride: usize,
) -> usize {
    let lab = CIELab {
        l: w_out[0] as f64 * 100.0,
        a: w_out[1] as f64 * 255.0 - 128.0,
        b: w_out[2] as f64 * 255.0 - 128.0,
    };
    let wlab = cms_float_to_lab_encoded_v2(&lab);
    if fmt.planar() {
        let stride_el = stride / pixel_size(fmt);
        wr_u16(dst, 0, wlab[0]);
        wr_u16(dst, stride_el * 2, wlab[1]);
        wr_u16(dst, stride_el * 2 * 2, wlab[2]);
        2
    } else {
        wr_u16(dst, 0, wlab[0]);
        wr_u16(dst, 2, wlab[1]);
        wr_u16(dst, 4, wlab[2]);
        (3 + fmt.extra() as usize) * 2
    }
}

/// `PackXYZFloatFromFloat` — normalised `0..1` working XYZ → `cmsCIEXYZ` `f32`.
pub fn pack_xyz_float_from_float(
    fmt: PixelFormat,
    w_out: &[f32],
    dst: &mut [u8],
    stride: usize,
) -> usize {
    if fmt.planar() {
        let stride_el = stride / pixel_size(fmt);
        wr_f32(dst, 0, (w_out[0] as f64 * MAX_ENCODEABLE_XYZ) as f32);
        wr_f32(dst, stride_el * 4, (w_out[1] as f64 * MAX_ENCODEABLE_XYZ) as f32);
        wr_f32(dst, stride_el * 2 * 4, (w_out[2] as f64 * MAX_ENCODEABLE_XYZ) as f32);
        4
    } else {
        wr_f32(dst, 0, (w_out[0] as f64 * MAX_ENCODEABLE_XYZ) as f32);
        wr_f32(dst, 4, (w_out[1] as f64 * MAX_ENCODEABLE_XYZ) as f32);
        wr_f32(dst, 8, (w_out[2] as f64 * MAX_ENCODEABLE_XYZ) as f32);
        3 * 4 + fmt.extra() as usize * 4
    }
}

/// `PackXYZDoubleFromFloat` — normalised `0..1` working XYZ → `cmsCIEXYZ` `f64`.
pub fn pack_xyz_double_from_float(
    fmt: PixelFormat,
    w_out: &[f32],
    dst: &mut [u8],
    stride: usize,
) -> usize {
    if fmt.planar() {
        let stride_el = stride / pixel_size(fmt);
        wr_f64(dst, 0, w_out[0] as f64 * MAX_ENCODEABLE_XYZ);
        wr_f64(dst, stride_el * 8, w_out[1] as f64 * MAX_ENCODEABLE_XYZ);
        wr_f64(dst, stride_el * 2 * 8, w_out[2] as f64 * MAX_ENCODEABLE_XYZ);
        8
    } else {
        wr_f64(dst, 0, w_out[0] as f64 * MAX_ENCODEABLE_XYZ);
        wr_f64(dst, 8, w_out[1] as f64 * MAX_ENCODEABLE_XYZ);
        wr_f64(dst, 16, w_out[2] as f64 * MAX_ENCODEABLE_XYZ);
        8 * 3 + fmt.extra() as usize * 8
    }
}
