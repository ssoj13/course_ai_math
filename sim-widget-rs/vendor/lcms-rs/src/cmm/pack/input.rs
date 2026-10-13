//! Unpacking formatters (`Unroll*` in `cmspack.c`): pixel buffer → working `wIn[]`.
//!
//! See the [module docs](super) for the generic-vs-specialised strategy. Each function is a
//! direct port of the like-named C routine; the C name is given in each doc comment.

use super::{
    fom_lab_v2_to_v4, is_ink_space, pixel_size, rd_f32, rd_f64, rd_u16, MAX_ENCODEABLE_XYZ,
};
use crate::color::pcs::{cms_float_to_lab_encoded, cms_float_to_xyz_encoded};
use crate::types::color::{CIELab, CIEXYZ};
use crate::types::format::PixelFormat;
use crate::types::num::{from_8_to_16, quick_saturate_word, to_fixed_domain};

// ============================================================================================
// Generic 16-bit unrollers (chunky + planar), 8-bit samples
// ============================================================================================

/// `UnrollChunkyBytes` — the generic byte unpacker (doswap/flavor/swapfirst/extra/premul).
pub fn unroll_chunky_bytes(fmt: PixelFormat, w_in: &mut [u16], src: &[u8], _stride: usize) -> usize {
    let n_chan = fmt.channels() as usize;
    let do_swap = fmt.doswap();
    let reverse = fmt.flavor();
    let swap_first = fmt.swapfirst();
    let extra = fmt.extra() as usize;
    let premul = fmt.premul();
    let extra_first = do_swap ^ swap_first;

    let mut pos = 0usize;
    let mut alpha_factor: u32 = 1;
    if extra_first {
        if premul && extra > 0 {
            alpha_factor = to_fixed_domain(from_8_to_16(src[0]) as i32) as u32;
        }
        pos += extra;
    } else if premul && extra > 0 {
        alpha_factor = to_fixed_domain(from_8_to_16(src[n_chan]) as i32) as u32;
    }

    for i in 0..n_chan {
        let index = if do_swap { n_chan - i - 1 } else { i };
        let mut v: u32 = from_8_to_16(src[pos]) as u32;
        if reverse {
            v = 0xffff - v; // REVERSE_FLAVOR_16
        }
        if premul && alpha_factor > 0 {
            v = (v << 16) / alpha_factor;
            if v > 0xffff {
                v = 0xffff;
            }
        }
        w_in[index] = v as u16;
        pos += 1;
    }

    if !extra_first {
        pos += extra;
    }

    if extra == 0 && swap_first {
        let tmp = w_in[0];
        w_in.copy_within(1..n_chan, 0);
        w_in[n_chan - 1] = tmp;
    }

    pos
}

/// `UnrollPlanarBytes` — generic byte unpacker for planar layouts. Returns one sample (1).
pub fn unroll_planar_bytes(fmt: PixelFormat, w_in: &mut [u16], src: &[u8], stride: usize) -> usize {
    let n_chan = fmt.channels() as usize;
    let do_swap = fmt.doswap();
    let swap_first = fmt.swapfirst();
    let reverse = fmt.flavor();
    let extra = fmt.extra() as usize;
    let premul = fmt.premul();
    let extra_first = do_swap ^ swap_first;

    let mut base = 0usize;
    let mut alpha_factor: u32 = 1;
    if extra_first {
        if premul && extra > 0 {
            alpha_factor = to_fixed_domain(from_8_to_16(src[0]) as i32) as u32;
        }
        base += extra * stride;
    } else if premul && extra > 0 {
        alpha_factor = to_fixed_domain(from_8_to_16(src[n_chan * stride]) as i32) as u32;
    }

    for i in 0..n_chan {
        let index = if do_swap { n_chan - i - 1 } else { i };
        let mut v: u32 = from_8_to_16(src[base]) as u32;
        if reverse {
            v = 0xffff - v;
        }
        if premul && alpha_factor > 0 {
            v = (v << 16) / alpha_factor;
            if v > 0xffff {
                v = 0xffff;
            }
        }
        w_in[index] = v as u16;
        base += stride;
    }

    1
}

// ============================================================================================
// Generic 16-bit unrollers, 16-bit samples
// ============================================================================================

/// `UnrollAnyWords` — generic word unpacker (endian16/doswap/flavor/swapfirst/extra).
pub fn unroll_any_words(fmt: PixelFormat, w_in: &mut [u16], src: &[u8], _stride: usize) -> usize {
    let n_chan = fmt.channels() as usize;
    let swap_endian = fmt.endian16();
    let do_swap = fmt.doswap();
    let reverse = fmt.flavor();
    let swap_first = fmt.swapfirst();
    let extra = fmt.extra() as usize;
    let extra_first = do_swap ^ swap_first;

    let mut pos = 0usize;
    if extra_first {
        pos += extra * 2;
    }

    for i in 0..n_chan {
        let index = if do_swap { n_chan - i - 1 } else { i };
        let mut v = rd_u16(src, pos);
        if swap_endian {
            v = v.swap_bytes(); // CHANGE_ENDIAN
        }
        w_in[index] = if reverse { 0xffff - v } else { v };
        pos += 2;
    }

    if !extra_first {
        pos += extra * 2;
    }

    if extra == 0 && swap_first {
        let tmp = w_in[0];
        w_in.copy_within(1..n_chan, 0);
        w_in[n_chan - 1] = tmp;
    }

    pos
}

/// `UnrollAnyWordsPremul` — as [`unroll_any_words`] with premultiplied-alpha unpacking.
///
/// Matches the C peculiarities exactly: the alpha word is read raw (no endian swap), there
/// is no `swapfirst` tail rotation, and the endian swap is applied to each component before
/// the premultiply divide.
pub fn unroll_any_words_premul(
    fmt: PixelFormat,
    w_in: &mut [u16],
    src: &[u8],
    _stride: usize,
) -> usize {
    let n_chan = fmt.channels() as usize;
    let swap_endian = fmt.endian16();
    let do_swap = fmt.doswap();
    let reverse = fmt.flavor();
    let swap_first = fmt.swapfirst();
    let extra = fmt.extra() as usize;
    let extra_first = do_swap ^ swap_first;

    let alpha = if extra_first { rd_u16(src, 0) } else { rd_u16(src, n_chan * 2) };
    let alpha_factor = to_fixed_domain(alpha as i32) as u32;

    let mut pos = 0usize;
    if extra_first {
        pos += extra * 2;
    }

    for i in 0..n_chan {
        let index = if do_swap { n_chan - i - 1 } else { i };
        let mut v: u32 = rd_u16(src, pos) as u32;
        if swap_endian {
            v = (v as u16).swap_bytes() as u32;
        }
        // C: `if (alpha_factor > 0) v = (v << 16) / alpha_factor;` — `checked_div` is `None`
        // exactly when `alpha_factor == 0`, so this preserves the guard-and-divide behaviour.
        if let Some(d) = (v << 16).checked_div(alpha_factor) {
            v = d;
            if v > 0xffff {
                v = 0xffff;
            }
        }
        let vv = v as u16;
        w_in[index] = if reverse { 0xffff - vv } else { vv };
        pos += 2;
    }

    if !extra_first {
        pos += extra * 2;
    }

    pos
}

/// `UnrollPlanarWords` — generic planar word unpacker. Returns one sample (2).
pub fn unroll_planar_words(fmt: PixelFormat, w_in: &mut [u16], src: &[u8], stride: usize) -> usize {
    let n_chan = fmt.channels() as usize;
    let do_swap = fmt.doswap();
    let reverse = fmt.flavor();
    let swap_endian = fmt.endian16();
    let extra = fmt.extra() as usize;

    let mut base = 0usize;
    if do_swap {
        base += extra * stride;
    }

    for i in 0..n_chan {
        let index = if do_swap { n_chan - i - 1 } else { i };
        let mut v = rd_u16(src, base);
        if swap_endian {
            v = v.swap_bytes();
        }
        w_in[index] = if reverse { 0xffff - v } else { v };
        base += stride;
    }

    2
}

/// `UnrollPlanarWordsPremul` — planar word unpacker with premultiplied alpha. Returns 2.
pub fn unroll_planar_words_premul(
    fmt: PixelFormat,
    w_in: &mut [u16],
    src: &[u8],
    stride: usize,
) -> usize {
    let n_chan = fmt.channels() as usize;
    let do_swap = fmt.doswap();
    let swap_first = fmt.swapfirst();
    let reverse = fmt.flavor();
    let swap_endian = fmt.endian16();
    let extra_first = do_swap ^ swap_first;

    // C: alpha = ExtraFirst ? ((u16*)accum)[0] : ((u16*)accum)[nChan*Stride/2];
    // The `/2` cancels the u16-index → byte-offset scaling, giving byte offset nChan*Stride.
    let alpha = if extra_first { rd_u16(src, 0) } else { rd_u16(src, n_chan * stride) };
    let alpha_factor = to_fixed_domain(alpha as i32) as u32;

    let mut base = 0usize;
    if extra_first {
        base += stride;
    }

    for i in 0..n_chan {
        let index = if do_swap { n_chan - i - 1 } else { i };
        let mut v: u32 = rd_u16(src, base) as u32;
        if swap_endian {
            v = (v as u16).swap_bytes() as u32;
        }
        // C: `if (alpha_factor > 0) v = (v << 16) / alpha_factor;` — `checked_div` is `None`
        // exactly when `alpha_factor == 0`, so this preserves the guard-and-divide behaviour.
        if let Some(d) = (v << 16).checked_div(alpha_factor) {
            v = d;
            if v > 0xffff {
                v = 0xffff;
            }
        }
        let vv = v as u16;
        w_in[index] = if reverse { 0xffff - vv } else { vv };
        base += stride;
    }

    2
}

// ============================================================================================
// Monochrome unrollers that duplicate L into wIn[0..3] (not reducible to the generic engine)
// ============================================================================================

/// `Unroll1Byte` — one grey byte duplicated into `wIn[0..3]` (for null transforms).
pub fn unroll_1_byte(_fmt: PixelFormat, w_in: &mut [u16], src: &[u8], _stride: usize) -> usize {
    let v = from_8_to_16(src[0]);
    w_in[0] = v;
    w_in[1] = v;
    w_in[2] = v;
    1
}

/// `Unroll1ByteSkip1` — as [`unroll_1_byte`] then skip one byte.
pub fn unroll_1_byte_skip1(_fmt: PixelFormat, w_in: &mut [u16], src: &[u8], _stride: usize) -> usize {
    let v = from_8_to_16(src[0]);
    w_in[0] = v;
    w_in[1] = v;
    w_in[2] = v;
    2
}

/// `Unroll1ByteSkip2` — as [`unroll_1_byte`] then skip two bytes.
pub fn unroll_1_byte_skip2(_fmt: PixelFormat, w_in: &mut [u16], src: &[u8], _stride: usize) -> usize {
    let v = from_8_to_16(src[0]);
    w_in[0] = v;
    w_in[1] = v;
    w_in[2] = v;
    3
}

/// `Unroll1ByteReversed` — [`unroll_1_byte`] with a reversed (min-is-white) grey.
pub fn unroll_1_byte_reversed(
    _fmt: PixelFormat,
    w_in: &mut [u16],
    src: &[u8],
    _stride: usize,
) -> usize {
    let v = 0xffff - from_8_to_16(src[0]); // REVERSE_FLAVOR_16(FROM_8_TO_16(x))
    w_in[0] = v;
    w_in[1] = v;
    w_in[2] = v;
    1
}

/// `Unroll1Word` — one grey word duplicated into `wIn[0..3]`.
pub fn unroll_1_word(_fmt: PixelFormat, w_in: &mut [u16], src: &[u8], _stride: usize) -> usize {
    let v = rd_u16(src, 0);
    w_in[0] = v;
    w_in[1] = v;
    w_in[2] = v;
    2
}

/// `Unroll1WordReversed` — [`unroll_1_word`] with a reversed grey.
pub fn unroll_1_word_reversed(
    _fmt: PixelFormat,
    w_in: &mut [u16],
    src: &[u8],
    _stride: usize,
) -> usize {
    let v = 0xffff - rd_u16(src, 0);
    w_in[0] = v;
    w_in[1] = v;
    w_in[2] = v;
    2
}

/// `Unroll1WordSkip3` — one grey word duplicated, then advance 8 bytes.
pub fn unroll_1_word_skip3(_fmt: PixelFormat, w_in: &mut [u16], src: &[u8], _stride: usize) -> usize {
    let v = rd_u16(src, 0);
    w_in[0] = v;
    w_in[1] = v;
    w_in[2] = v;
    8
}

// ============================================================================================
// LabV2 8/16-bit unrollers (FomLabV2ToLabV4)
// ============================================================================================

/// `UnrollLabV2_8` — V2-encoded 8-bit Lab, rescaled to the V4 16-bit working encoding.
pub fn unroll_lab_v2_8(_fmt: PixelFormat, w_in: &mut [u16], src: &[u8], _stride: usize) -> usize {
    w_in[0] = fom_lab_v2_to_v4(from_8_to_16(src[0]));
    w_in[1] = fom_lab_v2_to_v4(from_8_to_16(src[1]));
    w_in[2] = fom_lab_v2_to_v4(from_8_to_16(src[2]));
    3
}

/// `UnrollALabV2_8` — as [`unroll_lab_v2_8`] with a leading alpha byte skipped.
pub fn unroll_a_lab_v2_8(_fmt: PixelFormat, w_in: &mut [u16], src: &[u8], _stride: usize) -> usize {
    w_in[0] = fom_lab_v2_to_v4(from_8_to_16(src[1]));
    w_in[1] = fom_lab_v2_to_v4(from_8_to_16(src[2]));
    w_in[2] = fom_lab_v2_to_v4(from_8_to_16(src[3]));
    4
}

/// `UnrollLabV2_16` — V2-encoded 16-bit Lab, rescaled to the V4 working encoding.
pub fn unroll_lab_v2_16(_fmt: PixelFormat, w_in: &mut [u16], src: &[u8], _stride: usize) -> usize {
    w_in[0] = fom_lab_v2_to_v4(rd_u16(src, 0));
    w_in[1] = fom_lab_v2_to_v4(rd_u16(src, 2));
    w_in[2] = fom_lab_v2_to_v4(rd_u16(src, 4));
    6
}

// ============================================================================================
// Float/double/half → 16-bit unrollers (generic, ink-aware)
// ============================================================================================

/// `UnrollDoubleTo16` — `f64` samples scaled to `u16` (ink-aware, planar/chunky).
pub fn unroll_double_to_16(fmt: PixelFormat, w_in: &mut [u16], src: &[u8], stride: usize) -> usize {
    let n_chan = fmt.channels() as usize;
    let do_swap = fmt.doswap();
    let reverse = fmt.flavor();
    let swap_first = fmt.swapfirst();
    let extra = fmt.extra() as usize;
    let planar = fmt.planar();
    let maximum = if is_ink_space(fmt) { 655.35 } else { 65535.0 };
    let extra_first = do_swap ^ swap_first;
    let stride_el = stride / pixel_size(fmt);
    let start = if extra_first { extra } else { 0 };

    for i in 0..n_chan {
        let index = if do_swap { n_chan - i - 1 } else { i };
        let off = if planar { (i + start) * stride_el } else { i + start };
        // C narrows the f64 to f32 before scaling (`(cmsFloat32Number)(...double...)`).
        let v = (rd_f64(src, off * 8) as f32) as f64;
        let mut vi = quick_saturate_word(v * maximum);
        if reverse {
            vi = 0xffff - vi;
        }
        w_in[index] = vi;
    }

    if extra == 0 && swap_first {
        let tmp = w_in[0];
        w_in.copy_within(1..n_chan, 0);
        w_in[n_chan - 1] = tmp;
    }

    if planar { 8 } else { (n_chan + extra) * 8 }
}

/// `UnrollFloatTo16` — `f32` samples scaled to `u16` (ink-aware, planar/chunky).
pub fn unroll_float_to_16(fmt: PixelFormat, w_in: &mut [u16], src: &[u8], stride: usize) -> usize {
    let n_chan = fmt.channels() as usize;
    let do_swap = fmt.doswap();
    let reverse = fmt.flavor();
    let swap_first = fmt.swapfirst();
    let extra = fmt.extra() as usize;
    let planar = fmt.planar();
    let maximum = if is_ink_space(fmt) { 655.35 } else { 65535.0 };
    let extra_first = do_swap ^ swap_first;
    let stride_el = stride / pixel_size(fmt);
    let start = if extra_first { extra } else { 0 };

    for i in 0..n_chan {
        let index = if do_swap { n_chan - i - 1 } else { i };
        let off = if planar { (i + start) * stride_el } else { i + start };
        let v = rd_f32(src, off * 4);
        let mut vi = quick_saturate_word(v as f64 * maximum);
        if reverse {
            vi = 0xffff - vi;
        }
        w_in[index] = vi;
    }

    if extra == 0 && swap_first {
        let tmp = w_in[0];
        w_in.copy_within(1..n_chan, 0);
        w_in[n_chan - 1] = tmp;
    }

    if planar { 4 } else { (n_chan + extra) * 4 }
}

/// `UnrollHalfTo16` — IEEE-754 half samples scaled to `u16` (ink-aware, planar/chunky).
#[cfg(feature = "half-tables")]
pub fn unroll_half_to_16(fmt: PixelFormat, w_in: &mut [u16], src: &[u8], stride: usize) -> usize {
    use crate::math::half::half_to_float;

    let n_chan = fmt.channels() as usize;
    let do_swap = fmt.doswap();
    let reverse = fmt.flavor();
    let swap_first = fmt.swapfirst();
    let extra = fmt.extra() as usize;
    let planar = fmt.planar();
    let maximum: f32 = if is_ink_space(fmt) { 655.35 } else { 65535.0 };
    let extra_first = do_swap ^ swap_first;
    // NOTE: C divides by `PixelSize(info->OutputFormat)` here (an upstream typo — this is an
    // input formatter). Our API carries a single format, so we use the input pixel size,
    // which is what the caller intends for planar half input.
    let stride_el = stride / pixel_size(fmt);
    let start = if extra_first { extra } else { 0 };

    for i in 0..n_chan {
        let index = if do_swap { n_chan - i - 1 } else { i };
        let off = if planar { (i + start) * stride_el } else { i + start };
        let mut v = half_to_float(rd_u16(src, off * 2));
        if reverse {
            v = maximum - v;
        }
        w_in[index] = quick_saturate_word(v as f64 * maximum as f64);
    }

    if extra == 0 && swap_first {
        let tmp = w_in[0];
        w_in.copy_within(1..n_chan, 0);
        w_in[n_chan - 1] = tmp;
    }

    if planar { 2 } else { (n_chan + extra) * 2 }
}

/// `UnrollDouble1Chan` — a single `f64` ink channel duplicated into `wIn[0..3]`.
pub fn unroll_double_1chan(_fmt: PixelFormat, w_in: &mut [u16], src: &[u8], _stride: usize) -> usize {
    let v = quick_saturate_word(rd_f64(src, 0) * 65535.0);
    w_in[0] = v;
    w_in[1] = v;
    w_in[2] = v;
    8
}

// ============================================================================================
// Lab/XYZ double/float → 16-bit unrollers (PCS encodings)
// ============================================================================================

/// `UnrollLabDoubleTo16` — `cmsCIELab` `f64` → V4 16-bit Lab encoding.
pub fn unroll_lab_double_to_16(
    fmt: PixelFormat,
    w_in: &mut [u16],
    src: &[u8],
    stride: usize,
) -> usize {
    if fmt.planar() {
        // C reads at byte offsets accum, accum+Stride, accum+Stride*2 (no /PixelSize).
        let lab = CIELab {
            l: rd_f64(src, 0),
            a: rd_f64(src, stride),
            b: rd_f64(src, stride * 2),
        };
        let enc = cms_float_to_lab_encoded(&lab);
        w_in[0] = enc[0];
        w_in[1] = enc[1];
        w_in[2] = enc[2];
        8
    } else {
        let lab = CIELab { l: rd_f64(src, 0), a: rd_f64(src, 8), b: rd_f64(src, 16) };
        let enc = cms_float_to_lab_encoded(&lab);
        w_in[0] = enc[0];
        w_in[1] = enc[1];
        w_in[2] = enc[2];
        24 + fmt.extra() as usize * 8
    }
}

/// `UnrollLabFloatTo16` — `cmsCIELab` `f32` → V4 16-bit Lab encoding.
pub fn unroll_lab_float_to_16(
    fmt: PixelFormat,
    w_in: &mut [u16],
    src: &[u8],
    stride: usize,
) -> usize {
    if fmt.planar() {
        // C uses raw byte offsets accum, accum+Stride, accum+Stride*2.
        let lab = CIELab {
            l: rd_f32(src, 0) as f64,
            a: rd_f32(src, stride) as f64,
            b: rd_f32(src, stride * 2) as f64,
        };
        let enc = cms_float_to_lab_encoded(&lab);
        w_in[0] = enc[0];
        w_in[1] = enc[1];
        w_in[2] = enc[2];
        4
    } else {
        let lab = CIELab {
            l: rd_f32(src, 0) as f64,
            a: rd_f32(src, 4) as f64,
            b: rd_f32(src, 8) as f64,
        };
        let enc = cms_float_to_lab_encoded(&lab);
        w_in[0] = enc[0];
        w_in[1] = enc[1];
        w_in[2] = enc[2];
        (3 + fmt.extra() as usize) * 4
    }
}

/// `UnrollXYZDoubleTo16` — `cmsCIEXYZ` `f64` → 1.15 fixed 16-bit XYZ encoding.
pub fn unroll_xyz_double_to_16(
    fmt: PixelFormat,
    w_in: &mut [u16],
    src: &[u8],
    stride: usize,
) -> usize {
    if fmt.planar() {
        let xyz = CIEXYZ {
            x: rd_f64(src, 0),
            y: rd_f64(src, stride),
            z: rd_f64(src, stride * 2),
        };
        let enc = cms_float_to_xyz_encoded(&xyz);
        w_in[0] = enc[0];
        w_in[1] = enc[1];
        w_in[2] = enc[2];
        8
    } else {
        let xyz = CIEXYZ { x: rd_f64(src, 0), y: rd_f64(src, 8), z: rd_f64(src, 16) };
        let enc = cms_float_to_xyz_encoded(&xyz);
        w_in[0] = enc[0];
        w_in[1] = enc[1];
        w_in[2] = enc[2];
        24 + fmt.extra() as usize * 8
    }
}

/// `UnrollXYZFloatTo16` — `cmsCIEXYZ` `f32` → 1.15 fixed 16-bit XYZ encoding.
pub fn unroll_xyz_float_to_16(
    fmt: PixelFormat,
    w_in: &mut [u16],
    src: &[u8],
    stride: usize,
) -> usize {
    if fmt.planar() {
        let xyz = CIEXYZ {
            x: rd_f32(src, 0) as f64,
            y: rd_f32(src, stride) as f64,
            z: rd_f32(src, stride * 2) as f64,
        };
        let enc = cms_float_to_xyz_encoded(&xyz);
        w_in[0] = enc[0];
        w_in[1] = enc[1];
        w_in[2] = enc[2];
        4
    } else {
        let xyz = CIEXYZ {
            x: rd_f32(src, 0) as f64,
            y: rd_f32(src, 4) as f64,
            z: rd_f32(src, 8) as f64,
        };
        let enc = cms_float_to_xyz_encoded(&xyz);
        w_in[0] = enc[0];
        w_in[1] = enc[1];
        w_in[2] = enc[2];
        3 * 4 + fmt.extra() as usize * 4
    }
}

// ============================================================================================
// Float-path unrollers → working f32 buffer
// ============================================================================================

/// `Unroll8ToFloat` — 8-bit samples normalised to `0..1` `f32`.
pub fn unroll_8_to_float(fmt: PixelFormat, w_in: &mut [f32], src: &[u8], stride: usize) -> usize {
    let n_chan = fmt.channels() as usize;
    let do_swap = fmt.doswap();
    let reverse = fmt.flavor();
    let swap_first = fmt.swapfirst();
    let extra = fmt.extra() as usize;
    let planar = fmt.planar();
    let extra_first = do_swap ^ swap_first;
    let stride_el = stride / pixel_size(fmt);
    let start = if extra_first { extra } else { 0 };

    for i in 0..n_chan {
        let index = if do_swap { n_chan - i - 1 } else { i };
        let off = if planar { (i + start) * stride_el } else { i + start };
        let v = src[off] as f32 / 255.0;
        w_in[index] = if reverse { 1.0 - v } else { v };
    }

    if extra == 0 && swap_first {
        let tmp = w_in[0];
        w_in.copy_within(1..n_chan, 0);
        w_in[n_chan - 1] = tmp;
    }

    if planar { 1 } else { n_chan + extra }
}

/// `Unroll16ToFloat` — 16-bit samples normalised to `0..1` `f32`.
pub fn unroll_16_to_float(fmt: PixelFormat, w_in: &mut [f32], src: &[u8], stride: usize) -> usize {
    let n_chan = fmt.channels() as usize;
    let do_swap = fmt.doswap();
    let reverse = fmt.flavor();
    let swap_first = fmt.swapfirst();
    let extra = fmt.extra() as usize;
    let planar = fmt.planar();
    let extra_first = do_swap ^ swap_first;
    let stride_el = stride / pixel_size(fmt);
    let start = if extra_first { extra } else { 0 };

    for i in 0..n_chan {
        let index = if do_swap { n_chan - i - 1 } else { i };
        let off = if planar { (i + start) * stride_el } else { i + start };
        let v = rd_u16(src, off * 2) as f32 / 65535.0;
        w_in[index] = if reverse { 1.0 - v } else { v };
    }

    if extra == 0 && swap_first {
        let tmp = w_in[0];
        w_in.copy_within(1..n_chan, 0);
        w_in[n_chan - 1] = tmp;
    }

    if planar { 2 } else { (n_chan + extra) * 2 }
}

/// `UnrollFloatsToFloat` — `f32` samples (ink-aware, premultiplied-alpha aware).
pub fn unroll_floats_to_float(
    fmt: PixelFormat,
    w_in: &mut [f32],
    src: &[u8],
    stride: usize,
) -> usize {
    let n_chan = fmt.channels() as usize;
    let do_swap = fmt.doswap();
    let reverse = fmt.flavor();
    let swap_first = fmt.swapfirst();
    let extra = fmt.extra() as usize;
    let planar = fmt.planar();
    let premul = fmt.premul();
    let extra_first = do_swap ^ swap_first;
    let maximum: f32 = if is_ink_space(fmt) { 100.0 } else { 1.0 };
    let stride_el = stride / pixel_size(fmt);

    let mut alpha_factor: f32 = 1.0;
    if premul && extra > 0 {
        let a_el = if extra_first {
            0
        } else if planar {
            n_chan * stride_el
        } else {
            n_chan
        };
        alpha_factor = rd_f32(src, a_el * 4) / maximum;
    }

    let start = if extra_first { extra } else { 0 };
    for i in 0..n_chan {
        let index = if do_swap { n_chan - i - 1 } else { i };
        let off = if planar { (i + start) * stride_el } else { i + start };
        let mut v = rd_f32(src, off * 4);
        if premul && alpha_factor > 0.0 {
            v /= alpha_factor;
        }
        v /= maximum;
        w_in[index] = if reverse { 1.0 - v } else { v };
    }

    if extra == 0 && swap_first {
        let tmp = w_in[0];
        w_in.copy_within(1..n_chan, 0);
        w_in[n_chan - 1] = tmp;
    }

    if planar { 4 } else { (n_chan + extra) * 4 }
}

/// `UnrollDoublesToFloat` — `f64` samples down-converted to working `f32` (ink/premul aware).
pub fn unroll_doubles_to_float(
    fmt: PixelFormat,
    w_in: &mut [f32],
    src: &[u8],
    stride: usize,
) -> usize {
    let n_chan = fmt.channels() as usize;
    let do_swap = fmt.doswap();
    let reverse = fmt.flavor();
    let swap_first = fmt.swapfirst();
    let extra = fmt.extra() as usize;
    let planar = fmt.planar();
    let premul = fmt.premul();
    let extra_first = do_swap ^ swap_first;
    let maximum: f64 = if is_ink_space(fmt) { 100.0 } else { 1.0 };
    let stride_el = stride / pixel_size(fmt);

    let mut alpha_factor: f64 = 1.0;
    if premul && extra > 0 {
        let a_el = if extra_first {
            0
        } else if planar {
            n_chan * stride_el
        } else {
            n_chan
        };
        alpha_factor = rd_f64(src, a_el * 8) / maximum;
    }

    let start = if extra_first { extra } else { 0 };
    for i in 0..n_chan {
        let index = if do_swap { n_chan - i - 1 } else { i };
        let off = if planar { (i + start) * stride_el } else { i + start };
        let mut v = rd_f64(src, off * 8);
        if premul && alpha_factor > 0.0 {
            v /= alpha_factor;
        }
        v /= maximum;
        w_in[index] = (if reverse { 1.0 - v } else { v }) as f32;
    }

    if extra == 0 && swap_first {
        let tmp = w_in[0];
        w_in.copy_within(1..n_chan, 0);
        w_in[n_chan - 1] = tmp;
    }

    if planar { 8 } else { (n_chan + extra) * 8 }
}

/// `UnrollHalfToFloat` — IEEE-754 half samples normalised to working `f32` (ink-aware).
#[cfg(feature = "half-tables")]
pub fn unroll_half_to_float(
    fmt: PixelFormat,
    w_in: &mut [f32],
    src: &[u8],
    stride: usize,
) -> usize {
    use crate::math::half::half_to_float;

    let n_chan = fmt.channels() as usize;
    let do_swap = fmt.doswap();
    let reverse = fmt.flavor();
    let swap_first = fmt.swapfirst();
    let extra = fmt.extra() as usize;
    let planar = fmt.planar();
    let extra_first = do_swap ^ swap_first;
    let maximum: f32 = if is_ink_space(fmt) { 100.0 } else { 1.0 };
    let stride_el = stride / pixel_size(fmt);
    let start = if extra_first { extra } else { 0 };

    for i in 0..n_chan {
        let index = if do_swap { n_chan - i - 1 } else { i };
        let off = if planar { (i + start) * stride_el } else { i + start };
        let mut v = half_to_float(rd_u16(src, off * 2));
        v /= maximum;
        w_in[index] = if reverse { 1.0 - v } else { v };
    }

    if extra == 0 && swap_first {
        let tmp = w_in[0];
        w_in.copy_within(1..n_chan, 0);
        w_in[n_chan - 1] = tmp;
    }

    if planar { 2 } else { (n_chan + extra) * 2 }
}

// ============================================================================================
// Lab/XYZ double/float → working f32 unrollers
// ============================================================================================

/// `UnrollLabDoubleToFloat` — `cmsCIELab` `f64` → normalised `0..1` `f32`.
pub fn unroll_lab_double_to_float(
    fmt: PixelFormat,
    w_in: &mut [f32],
    src: &[u8],
    stride: usize,
) -> usize {
    if fmt.planar() {
        let stride_el = stride / pixel_size(fmt);
        w_in[0] = (rd_f64(src, 0) / 100.0) as f32;
        w_in[1] = ((rd_f64(src, stride_el * 8) + 128.0) / 255.0) as f32;
        w_in[2] = ((rd_f64(src, stride_el * 2 * 8) + 128.0) / 255.0) as f32;
        8
    } else {
        w_in[0] = (rd_f64(src, 0) / 100.0) as f32;
        w_in[1] = ((rd_f64(src, 8) + 128.0) / 255.0) as f32;
        w_in[2] = ((rd_f64(src, 16) + 128.0) / 255.0) as f32;
        8 * (3 + fmt.extra() as usize)
    }
}

/// `UnrollLabFloatToFloat` — `cmsCIELab` `f32` → normalised `0..1` `f32`.
pub fn unroll_lab_float_to_float(
    fmt: PixelFormat,
    w_in: &mut [f32],
    src: &[u8],
    stride: usize,
) -> usize {
    if fmt.planar() {
        // `Pt[i] + 128` is evaluated in `float` in C (the int literal converts to float; only
        // the following `/ 255.0` promotes to double), so the sum is rounded to f32 first.
        // Doing the addition in f64 would skip that rounding step - see `lab4_to_float`, which
        // already stays in f32 throughout.
        let stride_el = stride / pixel_size(fmt);
        w_in[0] = (rd_f32(src, 0) as f64 / 100.0) as f32;
        w_in[1] = ((rd_f32(src, stride_el * 4) + 128.0f32) as f64 / 255.0) as f32;
        w_in[2] = ((rd_f32(src, stride_el * 2 * 4) + 128.0f32) as f64 / 255.0) as f32;
        4
    } else {
        w_in[0] = (rd_f32(src, 0) as f64 / 100.0) as f32;
        w_in[1] = ((rd_f32(src, 4) + 128.0f32) as f64 / 255.0) as f32;
        w_in[2] = ((rd_f32(src, 8) + 128.0f32) as f64 / 255.0) as f32;
        4 * (3 + fmt.extra() as usize)
    }
}

/// `UnrollXYZDoubleToFloat` — `cmsCIEXYZ` `f64` → `0..1` `f32` (÷ `MAX_ENCODEABLE_XYZ`).
pub fn unroll_xyz_double_to_float(
    fmt: PixelFormat,
    w_in: &mut [f32],
    src: &[u8],
    stride: usize,
) -> usize {
    if fmt.planar() {
        let stride_el = stride / pixel_size(fmt);
        w_in[0] = (rd_f64(src, 0) / MAX_ENCODEABLE_XYZ) as f32;
        w_in[1] = (rd_f64(src, stride_el * 8) / MAX_ENCODEABLE_XYZ) as f32;
        w_in[2] = (rd_f64(src, stride_el * 2 * 8) / MAX_ENCODEABLE_XYZ) as f32;
        8
    } else {
        w_in[0] = (rd_f64(src, 0) / MAX_ENCODEABLE_XYZ) as f32;
        w_in[1] = (rd_f64(src, 8) / MAX_ENCODEABLE_XYZ) as f32;
        w_in[2] = (rd_f64(src, 16) / MAX_ENCODEABLE_XYZ) as f32;
        8 * (3 + fmt.extra() as usize)
    }
}

/// `UnrollXYZFloatToFloat` — `cmsCIEXYZ` `f32` → `0..1` `f32` (÷ `MAX_ENCODEABLE_XYZ`).
pub fn unroll_xyz_float_to_float(
    fmt: PixelFormat,
    w_in: &mut [f32],
    src: &[u8],
    stride: usize,
) -> usize {
    if fmt.planar() {
        let stride_el = stride / pixel_size(fmt);
        w_in[0] = (rd_f32(src, 0) as f64 / MAX_ENCODEABLE_XYZ) as f32;
        w_in[1] = (rd_f32(src, stride_el * 4) as f64 / MAX_ENCODEABLE_XYZ) as f32;
        w_in[2] = (rd_f32(src, stride_el * 2 * 4) as f64 / MAX_ENCODEABLE_XYZ) as f32;
        4
    } else {
        w_in[0] = (rd_f32(src, 0) as f64 / MAX_ENCODEABLE_XYZ) as f32;
        w_in[1] = (rd_f32(src, 4) as f64 / MAX_ENCODEABLE_XYZ) as f32;
        w_in[2] = (rd_f32(src, 8) as f64 / MAX_ENCODEABLE_XYZ) as f32;
        4 * (3 + fmt.extra() as usize)
    }
}

/// `lab4toFloat` — a V4 16-bit Lab triple → normalised `0..1` `f32` (used by the LabV2
/// float unrollers). All arithmetic is in `f32`, matching the C `F`-suffixed literals.
#[inline]
fn lab4_to_float(w_in: &mut [f32], lab4: [u16; 3]) {
    let l = lab4[0] as f32 / 655.35;
    let a = (lab4[1] as f32 / 257.0) - 128.0;
    let b = (lab4[2] as f32 / 257.0) - 128.0;
    w_in[0] = l / 100.0;
    w_in[1] = (a + 128.0) / 255.0;
    w_in[2] = (b + 128.0) / 255.0;
}

/// `UnrollLabV2_8ToFloat` — V2 8-bit Lab → normalised `0..1` `f32`.
pub fn unroll_lab_v2_8_to_float(
    _fmt: PixelFormat,
    w_in: &mut [f32],
    src: &[u8],
    _stride: usize,
) -> usize {
    let lab4 = [
        fom_lab_v2_to_v4(from_8_to_16(src[0])),
        fom_lab_v2_to_v4(from_8_to_16(src[1])),
        fom_lab_v2_to_v4(from_8_to_16(src[2])),
    ];
    lab4_to_float(w_in, lab4);
    3
}

/// `UnrollALabV2_8ToFloat` — V2 8-bit Lab with a leading alpha byte skipped.
pub fn unroll_a_lab_v2_8_to_float(
    _fmt: PixelFormat,
    w_in: &mut [f32],
    src: &[u8],
    _stride: usize,
) -> usize {
    let lab4 = [
        fom_lab_v2_to_v4(from_8_to_16(src[1])),
        fom_lab_v2_to_v4(from_8_to_16(src[2])),
        fom_lab_v2_to_v4(from_8_to_16(src[3])),
    ];
    lab4_to_float(w_in, lab4);
    4
}

/// `UnrollLabV2_16ToFloat` — V2 16-bit Lab → normalised `0..1` `f32`.
pub fn unroll_lab_v2_16_to_float(
    _fmt: PixelFormat,
    w_in: &mut [f32],
    src: &[u8],
    _stride: usize,
) -> usize {
    let lab4 = [
        fom_lab_v2_to_v4(rd_u16(src, 0)),
        fom_lab_v2_to_v4(rd_u16(src, 2)),
        fom_lab_v2_to_v4(rd_u16(src, 4)),
    ];
    lab4_to_float(w_in, lab4);
    6
}
