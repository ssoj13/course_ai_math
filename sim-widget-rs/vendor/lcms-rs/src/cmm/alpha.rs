//! Extra-channel (alpha) copy — a direct port of `cmsalpha.c`.
//!
//! When a transform carries `cmsFLAGS_COPY_ALPHA`, the "extra" channels (those beyond the
//! colour channels — typically an alpha) are copied straight from source to destination,
//! converting between bit depths as needed. This module ports the 6×6 converter matrix
//! (axes `{8, 16, 16SE, HLF, FLT, DBL}`), the per-channel byte-increment computation
//! (chunky and planar, honouring `T_DOSWAP`/`T_SWAPFIRST`), and the copy driver.
//!
//! Endianness: like the C (`*(cmsUInt16Number*)ptr`), the non-`SE` 16-bit paths use the
//! host byte order via [`u16::from_ne_bytes`]/[`to_ne_bytes`]; the `16SE` paths byte-swap
//! (`CHANGE_ENDIAN`). This matches lcms bit-for-bit on the same platform, which is exactly
//! why `TYPE_*_16_SE` exists.

use super::Stride;
use crate::MAX_CHANNELS;
use crate::types::format::PixelFormat;
use crate::types::num::{from_8_to_16, from_16_to_8, quick_saturate_byte, quick_saturate_word};

/// An alpha-to-alpha converter: reads the source sample at `src[0..]`, writes the
/// converted sample to `dst[0..]` (`cmsFormatterAlphaFn`).
type AlphaFn = fn(&mut [u8], &[u8]);

/// `trueBytesSize` — a format's channel size in bytes; `T_BYTES == 0` means a `double`.
#[inline]
fn true_bytes_size(fmt: PixelFormat) -> usize {
    let b = fmt.bytes() as usize;
    if b == 0 { 8 } else { b }
}

#[inline]
fn rd_u16(src: &[u8]) -> u16 {
    u16::from_ne_bytes([src[0], src[1]])
}
#[inline]
fn wr_u16(dst: &mut [u8], v: u16) {
    dst[..2].copy_from_slice(&v.to_ne_bytes());
}
#[inline]
fn rd_f32(src: &[u8]) -> f32 {
    f32::from_ne_bytes([src[0], src[1], src[2], src[3]])
}
#[inline]
fn wr_f32(dst: &mut [u8], v: f32) {
    dst[..4].copy_from_slice(&v.to_ne_bytes());
}
#[inline]
fn rd_f64(src: &[u8]) -> f64 {
    f64::from_ne_bytes([src[0], src[1], src[2], src[3], src[4], src[5], src[6], src[7]])
}
#[inline]
fn wr_f64(dst: &mut [u8], v: f64) {
    dst[..8].copy_from_slice(&v.to_ne_bytes());
}

// --- from 8 ---
fn copy8(dst: &mut [u8], src: &[u8]) {
    dst[0] = src[0];
}
fn from8to16(dst: &mut [u8], src: &[u8]) {
    wr_u16(dst, from_8_to_16(src[0]));
}
fn from8to16se(dst: &mut [u8], src: &[u8]) {
    wr_u16(dst, from_8_to_16(src[0]).swap_bytes());
}
fn from8toflt(dst: &mut [u8], src: &[u8]) {
    wr_f32(dst, src[0] as f32 / 255.0);
}
fn from8todbl(dst: &mut [u8], src: &[u8]) {
    wr_f64(dst, src[0] as f64 / 255.0);
}
fn from8tohlf(dst: &mut [u8], src: &[u8]) {
    #[cfg(feature = "half-tables")]
    wr_u16(dst, crate::math::half::float_to_half(src[0] as f32 / 255.0));
    #[cfg(not(feature = "half-tables"))]
    let _ = (dst, src);
}

// --- from 16 ---
fn from16to8(dst: &mut [u8], src: &[u8]) {
    dst[0] = from_16_to_8(rd_u16(src));
}
fn from16seto8(dst: &mut [u8], src: &[u8]) {
    dst[0] = from_16_to_8(rd_u16(src).swap_bytes());
}
fn copy16(dst: &mut [u8], src: &[u8]) {
    dst[..2].copy_from_slice(&src[..2]);
}
fn from16to16(dst: &mut [u8], src: &[u8]) {
    wr_u16(dst, rd_u16(src).swap_bytes());
}
fn from16toflt(dst: &mut [u8], src: &[u8]) {
    wr_f32(dst, rd_u16(src) as f32 / 65535.0);
}
fn from16setoflt(dst: &mut [u8], src: &[u8]) {
    wr_f32(dst, rd_u16(src).swap_bytes() as f32 / 65535.0);
}
fn from16todbl(dst: &mut [u8], src: &[u8]) {
    wr_f64(dst, rd_u16(src) as f64 / 65535.0);
}
fn from16setodbl(dst: &mut [u8], src: &[u8]) {
    wr_f64(dst, rd_u16(src).swap_bytes() as f64 / 65535.0);
}
fn from16tohlf(dst: &mut [u8], src: &[u8]) {
    #[cfg(feature = "half-tables")]
    wr_u16(dst, crate::math::half::float_to_half(rd_u16(src) as f32 / 65535.0));
    #[cfg(not(feature = "half-tables"))]
    let _ = (dst, src);
}
fn from16setohlf(dst: &mut [u8], src: &[u8]) {
    #[cfg(feature = "half-tables")]
    wr_u16(dst, crate::math::half::float_to_half(rd_u16(src).swap_bytes() as f32 / 65535.0));
    #[cfg(not(feature = "half-tables"))]
    let _ = (dst, src);
}

// --- from float ---
fn fromfltto8(dst: &mut [u8], src: &[u8]) {
    dst[0] = quick_saturate_byte(rd_f32(src) as f64 * 255.0);
}
fn fromfltto16(dst: &mut [u8], src: &[u8]) {
    wr_u16(dst, quick_saturate_word(rd_f32(src) as f64 * 65535.0));
}
fn fromfltto16se(dst: &mut [u8], src: &[u8]) {
    wr_u16(dst, quick_saturate_word(rd_f32(src) as f64 * 65535.0).swap_bytes());
}
fn copy32(dst: &mut [u8], src: &[u8]) {
    dst[..4].copy_from_slice(&src[..4]);
}
fn fromflttodbl(dst: &mut [u8], src: &[u8]) {
    wr_f64(dst, rd_f32(src) as f64);
}
fn fromflttohlf(dst: &mut [u8], src: &[u8]) {
    #[cfg(feature = "half-tables")]
    wr_u16(dst, crate::math::half::float_to_half(rd_f32(src)));
    #[cfg(not(feature = "half-tables"))]
    let _ = (dst, src);
}

// --- from half ---
fn fromhlfto8(dst: &mut [u8], src: &[u8]) {
    #[cfg(feature = "half-tables")]
    {
        dst[0] = quick_saturate_byte(crate::math::half::half_to_float(rd_u16(src)) as f64 * 255.0);
    }
    #[cfg(not(feature = "half-tables"))]
    let _ = (dst, src);
}
fn fromhlfto16(dst: &mut [u8], src: &[u8]) {
    #[cfg(feature = "half-tables")]
    wr_u16(dst, quick_saturate_word(crate::math::half::half_to_float(rd_u16(src)) as f64 * 65535.0));
    #[cfg(not(feature = "half-tables"))]
    let _ = (dst, src);
}
fn fromhlfto16se(dst: &mut [u8], src: &[u8]) {
    #[cfg(feature = "half-tables")]
    wr_u16(
        dst,
        quick_saturate_word(crate::math::half::half_to_float(rd_u16(src)) as f64 * 65535.0)
            .swap_bytes(),
    );
    #[cfg(not(feature = "half-tables"))]
    let _ = (dst, src);
}
fn fromhlftoflt(dst: &mut [u8], src: &[u8]) {
    #[cfg(feature = "half-tables")]
    wr_f32(dst, crate::math::half::half_to_float(rd_u16(src)));
    #[cfg(not(feature = "half-tables"))]
    let _ = (dst, src);
}
fn fromhlftodbl(dst: &mut [u8], src: &[u8]) {
    #[cfg(feature = "half-tables")]
    wr_f64(dst, crate::math::half::half_to_float(rd_u16(src)) as f64);
    #[cfg(not(feature = "half-tables"))]
    let _ = (dst, src);
}

// --- from double ---
fn fromdblto8(dst: &mut [u8], src: &[u8]) {
    dst[0] = quick_saturate_byte(rd_f64(src) * 255.0);
}
fn fromdblto16(dst: &mut [u8], src: &[u8]) {
    wr_u16(dst, quick_saturate_word(rd_f64(src) * 65535.0));
}
fn fromdblto16se(dst: &mut [u8], src: &[u8]) {
    wr_u16(dst, quick_saturate_word(rd_f64(src) * 65535.0).swap_bytes());
}
fn fromdbltoflt(dst: &mut [u8], src: &[u8]) {
    wr_f32(dst, rd_f64(src) as f32);
}
fn fromdbltohlf(dst: &mut [u8], src: &[u8]) {
    #[cfg(feature = "half-tables")]
    wr_u16(dst, crate::math::half::float_to_half(rd_f64(src) as f32));
    #[cfg(not(feature = "half-tables"))]
    let _ = (dst, src);
}
fn copy64(dst: &mut [u8], src: &[u8]) {
    dst[..8].copy_from_slice(&src[..8]);
}

/// The 6×6 converter table (`FormattersAlpha`), rows = source kind, cols = dest kind, in
/// the order `{8, 16, 16SE, HLF, FLT, DBL}`.
#[rustfmt::skip]
static FORMATTERS_ALPHA: [[AlphaFn; 6]; 6] = [
    /* from 8   */ [copy8,      from8to16,   from8to16se,   from8tohlf,   from8toflt,   from8todbl  ],
    /* from 16  */ [from16to8,  copy16,      from16to16,    from16tohlf,  from16toflt,  from16todbl ],
    /* from 16SE*/ [from16seto8,from16to16,  copy16,        from16setohlf,from16setoflt,from16setodbl],
    /* from HLF */ [fromhlfto8, fromhlfto16, fromhlfto16se, copy16,       fromhlftoflt, fromhlftodbl],
    /* from FLT */ [fromfltto8, fromfltto16, fromfltto16se, fromflttohlf, copy32,       fromflttodbl],
    /* from DBL */ [fromdblto8, fromdblto16, fromdblto16se, fromdbltohlf, fromdbltoflt, copy64      ],
];

/// `FormatterPos` — the row/column index of a format in [`FORMATTERS_ALPHA`], or `None`.
fn formatter_pos(fmt: PixelFormat) -> Option<usize> {
    let b = fmt.bytes();
    if b == 0 && fmt.float() {
        return Some(5); // DBL
    }
    #[cfg(feature = "half-tables")]
    if b == 2 && fmt.float() {
        return Some(3); // HLF
    }
    if b == 4 && fmt.float() {
        return Some(4); // FLT
    }
    if b == 2 && !fmt.float() {
        return Some(if fmt.endian16() { 2 } else { 1 }); // 16SE / 16
    }
    if b == 1 && !fmt.float() {
        return Some(0); // 8
    }
    None
}

/// `_cmsGetFormatterAlpha` — pick the converter for `in_fmt` → `out_fmt`.
fn get_formatter_alpha(in_fmt: PixelFormat, out_fmt: PixelFormat) -> Option<AlphaFn> {
    let i = formatter_pos(in_fmt)?;
    let o = formatter_pos(out_fmt)?;
    Some(FORMATTERS_ALPHA[i][o])
}

/// The per-extra-channel starting byte offsets + increments computed for a format.
struct Increments {
    starting_order: [u32; MAX_CHANNELS],
    pointer_increments: [u32; MAX_CHANNELS],
}

/// `ComputeIncrementsForChunky`.
fn compute_increments_chunky(fmt: PixelFormat) -> Option<Increments> {
    let extra = fmt.extra() as usize;
    let nchannels = fmt.channels() as usize;
    let total_chans = nchannels + extra;
    let channel_size = true_bytes_size(fmt);
    let pixel_size = channel_size * total_chans;

    // Sanity check (C: total_chans <= 0 || >= cmsMAXCHANNELS).
    if total_chans == 0 || total_chans >= MAX_CHANNELS {
        return None;
    }

    let mut channels = [0u32; MAX_CHANNELS];
    let mut increments = [0u32; MAX_CHANNELS];
    for inc in increments.iter_mut().take(extra) {
        *inc = pixel_size as u32;
    }
    for (i, ch) in channels.iter_mut().enumerate().take(total_chans) {
        *ch = if fmt.doswap() { (total_chans - i - 1) as u32 } else { i as u32 };
    }
    // Swap first (ROL of positions) — chunky guards `total_chans > 1`.
    if fmt.swapfirst() && total_chans > 1 {
        let tmp = channels[0];
        for i in 0..total_chans - 1 {
            channels[i] = channels[i + 1];
        }
        channels[total_chans - 1] = tmp;
    }
    if channel_size > 1 {
        for ch in channels.iter_mut().take(total_chans) {
            *ch *= channel_size as u32;
        }
    }
    let mut starting = [0u32; MAX_CHANNELS];
    starting[..extra].copy_from_slice(&channels[nchannels..nchannels + extra]);
    Some(Increments { starting_order: starting, pointer_increments: increments })
}

/// `ComputeIncrementsForPlanar`.
fn compute_increments_planar(fmt: PixelFormat, bytes_per_plane: usize) -> Option<Increments> {
    let extra = fmt.extra() as usize;
    let nchannels = fmt.channels() as usize;
    let total_chans = nchannels + extra;
    let channel_size = true_bytes_size(fmt);

    if total_chans == 0 || total_chans >= MAX_CHANNELS {
        return None;
    }

    let mut channels = [0u32; MAX_CHANNELS];
    let mut increments = [0u32; MAX_CHANNELS];
    for inc in increments.iter_mut().take(extra) {
        *inc = channel_size as u32;
    }
    for (i, ch) in channels.iter_mut().enumerate().take(total_chans) {
        *ch = if fmt.doswap() { (total_chans - i - 1) as u32 } else { i as u32 };
    }
    // Swap first — note the C asymmetry: planar guards `total_chans > 0` (chunky uses > 1).
    if fmt.swapfirst() && total_chans > 0 {
        let tmp = channels[0];
        for i in 0..total_chans - 1 {
            channels[i] = channels[i + 1];
        }
        channels[total_chans - 1] = tmp;
    }
    for ch in channels.iter_mut().take(total_chans) {
        *ch *= bytes_per_plane as u32;
    }
    let mut starting = [0u32; MAX_CHANNELS];
    starting[..extra].copy_from_slice(&channels[nchannels..nchannels + extra]);
    Some(Increments { starting_order: starting, pointer_increments: increments })
}

/// `ComputeComponentIncrements` — dispatch on planar vs chunky.
fn compute_component_increments(fmt: PixelFormat, bytes_per_plane: usize) -> Option<Increments> {
    if fmt.planar() {
        compute_increments_planar(fmt, bytes_per_plane)
    } else {
        compute_increments_chunky(fmt)
    }
}

/// Copy the extra (alpha) channels from `src` to `dst` (`_cmsHandleExtraChannels`).
///
/// `copy_alpha` mirrors the transform's `cmsFLAGS_COPY_ALPHA`. The C in-place shortcut
/// (`InputFormat == OutputFormat && in == out`) is the caller's responsibility here, since
/// `src`/`dst` are distinct borrows; the format-mismatch and zero-extra early-outs are
/// preserved.
#[allow(clippy::too_many_arguments)]
pub fn handle_extra_channels(
    in_fmt: PixelFormat,
    out_fmt: PixelFormat,
    copy_alpha: bool,
    src: &[u8],
    dst: &mut [u8],
    pixels_per_line: usize,
    line_count: usize,
    stride: Stride,
) {
    if !copy_alpha {
        return;
    }
    let n_extra = in_fmt.extra() as usize;
    if n_extra != out_fmt.extra() as usize || n_extra == 0 {
        return;
    }
    let Some(src_inc) = compute_component_increments(in_fmt, stride.bytes_per_plane_in) else {
        return;
    };
    let Some(dst_inc) = compute_component_increments(out_fmt, stride.bytes_per_plane_out) else {
        return;
    };
    let Some(copy_fn) = get_formatter_alpha(in_fmt, out_fmt) else {
        return;
    };

    if n_extra == 1 {
        // Optimized single-extra-channel path.
        let mut src_stride = 0usize;
        let mut dst_stride = 0usize;
        for _ in 0..line_count {
            let mut soff = src_inc.starting_order[0] as usize + src_stride;
            let mut doff = dst_inc.starting_order[0] as usize + dst_stride;
            for _ in 0..pixels_per_line {
                copy_fn(&mut dst[doff..], &src[soff..]);
                soff += src_inc.pointer_increments[0] as usize;
                doff += dst_inc.pointer_increments[0] as usize;
            }
            src_stride += stride.bytes_per_line_in;
            dst_stride += stride.bytes_per_line_out;
        }
    } else {
        // General N-extra-channel path.
        let mut src_stride = [0usize; MAX_CHANNELS];
        let mut dst_stride = [0usize; MAX_CHANNELS];
        for _ in 0..line_count {
            let mut soff = [0usize; MAX_CHANNELS];
            let mut doff = [0usize; MAX_CHANNELS];
            for j in 0..n_extra {
                soff[j] = src_inc.starting_order[j] as usize + src_stride[j];
                doff[j] = dst_inc.starting_order[j] as usize + dst_stride[j];
            }
            for _ in 0..pixels_per_line {
                for k in 0..n_extra {
                    copy_fn(&mut dst[doff[k]..], &src[soff[k]..]);
                    soff[k] += src_inc.pointer_increments[k] as usize;
                    doff[k] += dst_inc.pointer_increments[k] as usize;
                }
            }
            for j in 0..n_extra {
                src_stride[j] += stride.bytes_per_line_in;
                dst_stride[j] += stride.bytes_per_line_out;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::format::FormatBuilder;

    // TYPE_*_8 with n colour channels + `extra` alpha channels, chunky, no swaps.
    fn fmt8(channels: u32, extra: u32) -> PixelFormat {
        FormatBuilder::new().channels(channels).extra(extra).bytes(1).build()
    }

    #[test]
    fn scaling_matches_c_macros() {
        // 8<->16 replication/rounding must match FROM_8_TO_16 / FROM_16_TO_8 exactly.
        for n in 0u16..=255 {
            let n = n as u8;
            assert_eq!(from_8_to_16(n), ((n as u16) << 8) | n as u16);
        }
        assert_eq!(from_16_to_8(0), 0);
        assert_eq!(from_16_to_8(0xFFFF), 0xFF);
        assert_eq!(from_16_to_8(0x8080), 0x80);
    }

    #[test]
    fn copy_single_alpha_8bit() {
        // RGBA_8: 3 colour + 1 alpha, one pixel per line, two lines.
        let f = fmt8(3, 1);
        let src = vec![10, 20, 30, 99, 40, 50, 60, 77]; // two RGBA pixels
        let mut dst = vec![0u8; 8];
        let stride = Stride {
            bytes_per_line_in: 4,
            bytes_per_line_out: 4,
            ..Default::default()
        };
        handle_extra_channels(f, f, true, &src, &mut dst, 1, 2, stride);
        // Alpha (byte index 3 of each pixel) copied; colour bytes untouched (stay 0).
        assert_eq!(dst[3], 99);
        assert_eq!(dst[7], 77);
        assert_eq!(&dst[0..3], &[0, 0, 0]);
    }

    #[test]
    fn copy_alpha_8_to_16_scales() {
        // in RGBA_8 (extra=1), out RGBA_16 (extra=1): alpha 0x99 -> from_8_to_16(0x99).
        let fin = fmt8(3, 1);
        let fout = FormatBuilder::new().channels(3).extra(1).bytes(2).build();
        let src = vec![0, 0, 0, 0x99];
        let mut dst = vec![0u8; 8]; // 4 channels * 2 bytes
        let stride = Stride { bytes_per_line_in: 4, bytes_per_line_out: 8, ..Default::default() };
        handle_extra_channels(fin, fout, true, &src, &mut dst, 1, 1, stride);
        // Alpha is channel index 3 -> byte offset 6 in the 16-bit dest.
        let a = u16::from_ne_bytes([dst[6], dst[7]]);
        assert_eq!(a, from_8_to_16(0x99));
    }

    #[test]
    fn copy_alpha_disabled_is_noop() {
        let f = fmt8(3, 1);
        let src = vec![1, 2, 3, 4];
        let mut dst = vec![0u8; 4];
        handle_extra_channels(f, f, false, &src, &mut dst, 1, 1, Stride::default());
        assert_eq!(dst, vec![0, 0, 0, 0]);
    }

    #[test]
    fn no_extra_is_noop() {
        let f = fmt8(3, 0);
        let src = vec![1, 2, 3];
        let mut dst = vec![0u8; 3];
        handle_extra_channels(f, f, true, &src, &mut dst, 1, 1, Stride::default());
        assert_eq!(dst, vec![0, 0, 0]);
    }

    #[test]
    fn doswap_moves_extra_position() {
        // With DOSWAP, channel order reverses: for RGBA (4 total), the extra ends up first.
        let f = FormatBuilder::new().channels(3).extra(1).bytes(1).doswap(true).build();
        let inc = compute_increments_chunky(f).unwrap();
        // extra channel (index 3 logically) maps to reversed position 0.
        assert_eq!(inc.starting_order[0], 0);
        assert_eq!(inc.pointer_increments[0], 4); // pixel size
    }
}
