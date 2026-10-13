//! Pixel formatters — a pure-Rust, bit-exact port of `src/cmspack.c` from Little-CMS.
//!
//! This module unpacks (`Unroll*`) an interleaved/planar pixel buffer into the internal
//! working representation and packs (`Pack*`) it back out. Two flavours exist, exactly as
//! in the C source:
//!
//! * **16-bit path** — working samples are `u16` (`cmsUInt16Number wIn[]/wOut[]`).
//! * **float path** — working samples are `f32` (`cmsFloat32Number`), used for the
//!   floating-point transform pipeline.
//!
//! # Design vs. the C source
//!
//! lcms ships dozens of hand-specialised formatters (`Unroll3Bytes`, `Unroll4Words`,
//! `Pack6Bytes`, …) purely for speed. The overwhelming majority produce output that is
//! **bit-identical** to the four generic engines — [`input::unroll_chunky_bytes`] /
//! [`input::unroll_planar_bytes`] (and their word / float / double / half analogues) and
//! [`output::pack_chunky_bytes`] / [`output::pack_planar_bytes`] (and analogues). Those
//! generic engines are ported verbatim (per-dimension: `doswap`, `flavor`, `swapfirst`,
//! `extra`, `premul`, `endian16`), and the dispatch tables point the specialised rows at
//! them. The rows are preserved in the *same order and with the same `Type`/`Mask`* as C,
//! so [`table`] selects the same engine for any given format word.
//!
//! A handful of specialised formatters are **not** reducible to the generic engine and are
//! ported individually:
//! * the monochrome unrollers that *duplicate* L into `wIn[0..3]`
//!   ([`input::unroll_1_byte`] etc.) — the generic engine would set only `wIn[0]`;
//! * the `Lab`/`XYZ` encoders/decoders (internal 16-bit Lab/XYZ encodings, V2 variants);
//! * the `LabV2` byte/word (`FomLabV2ToLabV4`) unrollers/packers;
//! * the "optimized" byte packers (low-byte truncation) — kept for table fidelity even
//!   though [`table::get_output_formatter_16`] masks off `OPTIMIZED` before matching, so
//!   they are never selected in the stock path (matching C `_cmsGetStockOutputFormatter`).
//!
//! # Formatter contract
//!
//! Each formatter is a plain `fn` matching one of the four aliases below. `values` is the
//! working `u16`/`f32` scratch (length ≥ [`crate::MAX_CHANNELS`]); `src`/`dst` is the pixel
//! buffer positioned at the current pixel; `stride` is `BytesPerPlane` for **planar**
//! formats (ignored by chunky ones). The return value is the number of **bytes advanced**
//! in `src`/`dst` — for chunky that is `(nChan + Extra) * bytesPerSample`; for planar it is
//! one sample (`bytesPerSample`), matching the C `return Init + sizeof(sample)` convention.

pub mod input;
pub mod output;
pub mod table;

use crate::types::format::PixelFormat;

pub use table::{
    get_input_formatter_16, get_input_formatter_16_thr, get_input_formatter_float,
    get_input_formatter_float_thr, get_output_formatter_16, get_output_formatter_16_thr,
    get_output_formatter_float, get_output_formatter_float_thr,
};

/// Unpacks one pixel from a `u16` buffer. Ports `cmsFormatter16` (input direction).
pub type Formatter16In = fn(fmt: PixelFormat, values: &mut [u16], src: &[u8], stride: usize) -> usize;
/// Packs one pixel into a `u16` buffer. Ports `cmsFormatter16` (output direction).
pub type Formatter16Out = fn(fmt: PixelFormat, values: &[u16], dst: &mut [u8], stride: usize) -> usize;
/// Unpacks one pixel into an `f32` buffer. Ports `cmsFormatterFloat` (input direction).
pub type FormatterFloatIn = fn(fmt: PixelFormat, values: &mut [f32], src: &[u8], stride: usize) -> usize;
/// Packs one pixel from an `f32` buffer. Ports `cmsFormatterFloat` (output direction).
pub type FormatterFloatOut = fn(fmt: PixelFormat, values: &[f32], dst: &mut [u8], stride: usize) -> usize;

/// ICC v4 max encodeable XYZ (`MAX_ENCODEABLE_XYZ`, `lcms2_internal.h`): `1 + 32767/32768`.
pub(crate) const MAX_ENCODEABLE_XYZ: f64 = 1.0 + 32767.0 / 32768.0;

/// lcms2 pixel-type colour-space codes (`PT_*`) used by the formatters, as `u32` for the
/// `COLORSPACE` bit-field. Values verified against `lcms2.h` / [`crate::color::pcs::pt`].
pub(crate) mod pt {
    pub const GRAY: u32 = 3;
    /// `PT_RGB`. Part of the reference set; referenced by the round-trip tests rather than by
    /// the library formatters (which are colour-space-agnostic for RGB).
    #[allow(dead_code)]
    pub const RGB: u32 = 4;
    pub const CMY: u32 = 5;
    pub const CMYK: u32 = 6;
    pub const XYZ: u32 = 9;
    pub const LAB: u32 = 10;
    pub const MCH2: u32 = 16;
    pub const MCH5: u32 = 19;
    pub const MCH15: u32 = 29;
    pub const LAB_V2: u32 = 30;
}

/// Size in bytes of one sample of a format (`PixelSize`).
///
/// `T_BYTES == 0` is lcms's sentinel for a `double` (8 bytes); every other value is the
/// literal byte count.
#[inline]
pub(crate) fn pixel_size(fmt: PixelFormat) -> usize {
    let b = fmt.bytes() as usize;
    if b == 0 { 8 } else { b }
}

/// Whether a colour space carries inks (percent-scaled), i.e. `IsInkSpace`.
///
/// True for `PT_CMY`, `PT_CMYK` and `PT_MCH5..=PT_MCH15` (note: **not** MCH1..4). Inks scale
/// against `655.35`/`100.0` instead of `65535.0`/`1.0`.
#[inline]
pub(crate) fn is_ink_space(fmt: PixelFormat) -> bool {
    let cs = fmt.colorspace();
    cs == pt::CMY || cs == pt::CMYK || (pt::MCH5..=pt::MCH15).contains(&cs)
}

// --- raw sample read/write helpers (native-endian, mirroring C's `*(T*)ptr`) ---
//
// C reads/writes multi-byte samples directly from memory in host byte order
// (`*(cmsUInt16Number*)accum`, `*(cmsFloat64Number*)accum`). `from_ne_bytes`/`to_ne_bytes`
// reproduce that exactly without `unsafe`; explicit byte-swaps handle `endian16`/`CHANGE_ENDIAN`.

#[inline]
pub(crate) fn rd_u16(b: &[u8], o: usize) -> u16 {
    u16::from_ne_bytes([b[o], b[o + 1]])
}
#[inline]
pub(crate) fn wr_u16(b: &mut [u8], o: usize, v: u16) {
    let x = v.to_ne_bytes();
    b[o] = x[0];
    b[o + 1] = x[1];
}
#[inline]
pub(crate) fn rd_f32(b: &[u8], o: usize) -> f32 {
    let mut a = [0u8; 4];
    a.copy_from_slice(&b[o..o + 4]);
    f32::from_ne_bytes(a)
}
#[inline]
pub(crate) fn wr_f32(b: &mut [u8], o: usize, v: f32) {
    b[o..o + 4].copy_from_slice(&v.to_ne_bytes());
}
#[inline]
pub(crate) fn rd_f64(b: &[u8], o: usize) -> f64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[o..o + 8]);
    f64::from_ne_bytes(a)
}
#[inline]
pub(crate) fn wr_f64(b: &mut [u8], o: usize, v: f64) {
    b[o..o + 8].copy_from_slice(&v.to_ne_bytes());
}

/// `FomLabV2ToLabV4`: rescale a V2 16-bit Lab component to V4 (`* 257/256`, clamped).
#[inline]
pub(crate) fn fom_lab_v2_to_v4(x: u16) -> u16 {
    // C: `int a = (x << 8 | x) >> 8;` — computed in 32-bit to avoid u16 overflow.
    let a = (((x as u32) << 8) | x as u32) >> 8;
    if a > 0xffff { 0xffff } else { a as u16 }
}

/// `FomLabV4ToLabV2`: rescale a V4 16-bit Lab component to V2 (`* 256/257`, rounded).
#[inline]
pub(crate) fn fom_lab_v4_to_v2(x: u16) -> u16 {
    // C: `(cmsUInt16Number)(((x << 8) + 0x80) / 257)`.
    ((((x as u32) << 8) + 0x80) / 257) as u16
}

// ---------------------------------------------------------------------------------------
// Formatter-word helpers (`cmsFormatterForColorspaceOfProfile` / `…ForPCSOfProfile`)
// ---------------------------------------------------------------------------------------

/// Build a "fake" formatter word from a colour-space signature and sample depth
/// (`cmsFormatterForColorspaceOfProfile`).
///
/// The C entry point reads `cmsGetColorSpace(hProfile)` and forwards the signature here; we
/// take the signature directly so no profile handle is needed. The ICC colour-space 4CC is
/// mapped to its lcms `PT_*` code ([`crate::color::pcs::lcms_colorspace`]) and channel count
/// ([`crate::color::pcs::cms_channels_of_colorspace`]); an unsupported space (negative channel
/// count) yields the all-zero word, exactly like the C `return 0`.
///
/// `n_bytes` is masked to its low 3 bits (`nBytes &= 7`) so a caller passing `8` (double)
/// lands on the `T_BYTES == 0` sentinel, matching C.
pub fn formatter_for_colorspace(
    colorspace: crate::types::sig::Signature,
    n_bytes: u32,
    is_float: bool,
) -> PixelFormat {
    let color_space_bits = crate::color::pcs::lcms_colorspace(colorspace) as u32;
    let n_output_chans = crate::color::pcs::cms_channels_of_colorspace(colorspace);

    // Unsupported colour space?
    if n_output_chans < 0 {
        return PixelFormat::from_raw(0);
    }

    crate::types::format::FormatBuilder::new()
        .float(is_float)
        .colorspace(color_space_bits)
        .bytes(n_bytes & 7)
        .channels(n_output_chans as u32)
        .build()
}

/// [`formatter_for_colorspace`] with C's all-zero sentinel lifted into `Option`.
///
/// C signals "unsupported colour space" by returning the zero word and leaves the caller to
/// test it; several call sites here want the `Option` shape instead. Provided once, so those
/// sites do not each re-derive the sig -> PT mapping and drift apart.
pub fn formatter_for_colorspace_opt(
    colorspace: crate::types::sig::Signature,
    n_bytes: u32,
    is_float: bool,
) -> Option<PixelFormat> {
    let f = formatter_for_colorspace(colorspace, n_bytes, is_float);
    if f.raw() == 0 {
        None
    } else {
        Some(f)
    }
}

/// Build a "fake" formatter word for a profile's PCS (`cmsFormatterForPCSOfProfile`).
///
/// In C this differs from [`formatter_for_colorspace`] only in reading `cmsGetPCS` instead of
/// `cmsGetColorSpace`; once the signature is supplied the computation is identical, so this
/// delegates. `pcs` is the PCS colour-space signature (typically `XYZ`/`Lab`).
pub fn formatter_for_pcs(
    pcs: crate::types::sig::Signature,
    n_bytes: u32,
    is_float: bool,
) -> PixelFormat {
    formatter_for_colorspace(pcs, n_bytes, is_float)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::pcs::{
        cms_float_to_lab_encoded, cms_float_to_xyz_encoded, cms_lab_encoded_to_float,
    };
    use crate::types::color::CIELab;
    use crate::types::format::FormatBuilder;
    use crate::types::num::from_8_to_16;

    // Build a distinct-per-byte source buffer so channel reordering is observable.
    fn ramp(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i as u8).wrapping_mul(37).wrapping_add(11)).collect()
    }

    /// 16-bit path unpack→pack round trip must be byte-identical.
    fn rt16(fmt: PixelFormat) {
        let n = fmt.channels() as usize + fmt.extra() as usize;
        let len = n * pixel_size(fmt);
        let src = ramp(len);
        let inf = get_input_formatter_16(fmt).expect("input16 formatter");
        let outf = get_output_formatter_16(fmt).expect("output16 formatter");
        let mut w = [0u16; crate::MAX_CHANNELS];
        let adv_in = inf(fmt, &mut w, &src, 0);
        // Extra (alpha) channels are not the formatter's responsibility — they are copied by
        // `cmm::alpha`, so `pack` leaves those bytes untouched. Seed `dst` from `src` so the
        // untouched extra bytes match; a wrong colour byte still fails the comparison.
        let mut dst = src.clone();
        let adv_out = outf(fmt, &w, &mut dst, 0);
        assert_eq!(adv_in, len, "in advance");
        assert_eq!(adv_out, len, "out advance");
        assert_eq!(src, dst, "16-bit round trip");
    }

    #[test]
    fn round_trip_16_common_formats() {
        let rgb8 = FormatBuilder::new().colorspace(pt::RGB).channels(3).bytes(1).build();
        let bgr8 =
            FormatBuilder::new().colorspace(pt::RGB).channels(3).bytes(1).doswap(true).build();
        let rgb16 = FormatBuilder::new().colorspace(pt::RGB).channels(3).bytes(2).build();
        let rgb16se =
            FormatBuilder::new().colorspace(pt::RGB).channels(3).bytes(2).endian16(true).build();
        let cmyk8 = FormatBuilder::new().colorspace(pt::CMYK).channels(4).bytes(1).build();
        let gray8 = FormatBuilder::new().colorspace(pt::GRAY).channels(1).bytes(1).build();
        // TYPE_Lab_8 / TYPE_Lab_16 / TYPE_XYZ_16 are plain integer scalers in stock lcms
        // (the ICC encoding is produced by the pipeline, not the formatter).
        let lab8 = FormatBuilder::new().colorspace(pt::LAB).channels(3).bytes(1).build();
        let lab16 = FormatBuilder::new().colorspace(pt::LAB).channels(3).bytes(2).build();
        let xyz16 = FormatBuilder::new().colorspace(pt::XYZ).channels(3).bytes(2).build();
        // Only formats without extra channels round-trip byte-identically: the stock
        // formatters intentionally skip extra/alpha (copied by the separate cmsalpha path).
        for f in [rgb8, bgr8, rgb16, rgb16se, cmyk8, gray8, lab8, lab16, xyz16] {
            rt16(f);
        }
    }

    #[test]
    fn rgba8_round_trips_colour_channels_only() {
        // Extra/alpha is deliberately not touched by the formatter, so only the colour
        // channels round-trip; the alpha byte is left as-is in the destination.
        let rgba8 = FormatBuilder::new().colorspace(pt::RGB).extra(1).channels(3).bytes(1).build();
        let src = ramp(4);
        let mut w = [0u16; crate::MAX_CHANNELS];
        let a_in = get_input_formatter_16(rgba8).unwrap()(rgba8, &mut w, &src, 0);
        let mut dst = vec![0u8; 4];
        let a_out = get_output_formatter_16(rgba8).unwrap()(rgba8, &w, &mut dst, 0);
        assert_eq!(a_in, 4);
        assert_eq!(a_out, 4);
        assert_eq!(&dst[..3], &src[..3], "RGBA_8 colour channels");
    }

    #[test]
    fn planar_rgb8_round_trips() {
        // One planar pixel: three planes one byte apart (stride = 1).
        let fmt = FormatBuilder::new().colorspace(pt::RGB).channels(3).bytes(1).planar(true).build();
        let src = vec![10u8, 20, 30];
        let inf = get_input_formatter_16(fmt).unwrap();
        let outf = get_output_formatter_16(fmt).unwrap();
        let mut w = [0u16; crate::MAX_CHANNELS];
        let a_in = inf(fmt, &mut w, &src, 1);
        assert_eq!(a_in, 1, "planar advances one sample");
        assert_eq!(w[0], from_8_to_16(10));
        assert_eq!(w[1], from_8_to_16(20));
        assert_eq!(w[2], from_8_to_16(30));
        let mut dst = vec![0u8; 3];
        let a_out = outf(fmt, &w, &mut dst, 1);
        assert_eq!(a_out, 1);
        assert_eq!(dst, src);
    }

    #[test]
    fn doswap_reverses_channel_order() {
        let rgb = FormatBuilder::new().colorspace(pt::RGB).channels(3).bytes(1).build();
        let bgr =
            FormatBuilder::new().colorspace(pt::RGB).channels(3).bytes(1).doswap(true).build();
        let src = [10u8, 20, 30];
        let mut w_rgb = [0u16; crate::MAX_CHANNELS];
        let mut w_bgr = [0u16; crate::MAX_CHANNELS];
        get_input_formatter_16(rgb).unwrap()(rgb, &mut w_rgb, &src, 0);
        get_input_formatter_16(bgr).unwrap()(bgr, &mut w_bgr, &src, 0);
        // BGR must land the channels reversed relative to RGB.
        assert_eq!([w_bgr[0], w_bgr[1], w_bgr[2]], [w_rgb[2], w_rgb[1], w_rgb[0]]);
    }

    #[test]
    fn swapfirst_rotates_first_channel_to_last() {
        // KCMY: CMYK with SwapFirst and no extra channels rotates wIn.
        let kcmy =
            FormatBuilder::new().colorspace(pt::CMYK).channels(4).bytes(1).swapfirst(true).build();
        let src = [1u8, 2, 3, 4];
        let mut w = [0u16; crate::MAX_CHANNELS];
        get_input_formatter_16(kcmy).unwrap()(kcmy, &mut w, &src, 0);
        assert_eq!(
            [w[0], w[1], w[2], w[3]],
            [from_8_to_16(2), from_8_to_16(3), from_8_to_16(4), from_8_to_16(1)]
        );
    }

    #[test]
    fn float_rgb_round_trips() {
        let flt = FormatBuilder::new().float(true).colorspace(pt::RGB).channels(3).bytes(4).build();
        let dbl = FormatBuilder::new().float(true).colorspace(pt::RGB).channels(3).bytes(0).build();
        let vals = [0.25f32, 0.5, 0.75]; // exact in f32 and f64

        // RGB_FLT
        let mut src = vec![0u8; 12];
        for (i, &v) in vals.iter().enumerate() {
            wr_f32(&mut src, i * 4, v);
        }
        let mut w = [0f32; crate::MAX_CHANNELS];
        let a_in = get_input_formatter_float(flt).unwrap()(flt, &mut w, &src, 0);
        assert_eq!(a_in, 12);
        assert_eq!(&w[..3], &vals);
        let mut dst = vec![0u8; 12];
        let a_out = get_output_formatter_float(flt).unwrap()(flt, &w, &mut dst, 0);
        assert_eq!(a_out, 12);
        assert_eq!(src, dst, "RGB_FLT round trip");

        // RGB_DBL
        let mut src = vec![0u8; 24];
        for (i, &v) in vals.iter().enumerate() {
            wr_f64(&mut src, i * 8, v as f64);
        }
        let mut w = [0f32; crate::MAX_CHANNELS];
        let a_in = get_input_formatter_float(dbl).unwrap()(dbl, &mut w, &src, 0);
        assert_eq!(a_in, 24);
        assert_eq!(&w[..3], &vals);
        let mut dst = vec![0u8; 24];
        let a_out = get_output_formatter_float(dbl).unwrap()(dbl, &w, &mut dst, 0);
        assert_eq!(a_out, 24);
        assert_eq!(src, dst, "RGB_DBL round trip");
    }

    #[test]
    fn lab_double_uses_pcs_v4_encoding() {
        // TYPE_Lab_DBL selects the PCS Lab encoder on the 16-bit input path.
        let fmt = FormatBuilder::new().float(true).colorspace(pt::LAB).channels(3).bytes(0).build();
        let lab = CIELab { l: 42.0, a: -12.0, b: 33.0 };
        let mut src = vec![0u8; 24];
        wr_f64(&mut src, 0, lab.l);
        wr_f64(&mut src, 8, lab.a);
        wr_f64(&mut src, 16, lab.b);
        let mut w = [0u16; crate::MAX_CHANNELS];
        get_input_formatter_16(fmt).unwrap()(fmt, &mut w, &src, 0);
        let expect = cms_float_to_lab_encoded(&lab);
        assert_eq!(&w[..3], &expect, "Lab_DBL must encode via cmsFloat2LabEncoded");

        // Pack back and confirm it decodes to the same Lab within quantisation.
        let mut dst = vec![0u8; 24];
        get_output_formatter_16(fmt).unwrap()(fmt, &w, &mut dst, 0);
        let dec = CIELab { l: rd_f64(&dst, 0), a: rd_f64(&dst, 8), b: rd_f64(&dst, 16) };
        let ref_dec = cms_lab_encoded_to_float(&expect);
        assert!((dec.l - ref_dec.l).abs() < 1e-9);
        assert!((dec.a - ref_dec.a).abs() < 1e-9);
        assert!((dec.b - ref_dec.b).abs() < 1e-9);
    }

    #[test]
    fn xyz_double_uses_pcs_fixed_encoding() {
        use crate::types::color::CIEXYZ;
        let fmt = FormatBuilder::new().float(true).colorspace(pt::XYZ).channels(3).bytes(0).build();
        let xyz = CIEXYZ { x: 0.5, y: 0.55, z: 0.6 };
        let mut src = vec![0u8; 24];
        wr_f64(&mut src, 0, xyz.x);
        wr_f64(&mut src, 8, xyz.y);
        wr_f64(&mut src, 16, xyz.z);
        let mut w = [0u16; crate::MAX_CHANNELS];
        get_input_formatter_16(fmt).unwrap()(fmt, &mut w, &src, 0);
        let expect = cms_float_to_xyz_encoded(&xyz);
        assert_eq!(&w[..3], &expect, "XYZ_DBL must encode via cmsFloat2XYZEncoded");
    }

    #[test]
    fn labv2_16_round_trips_through_v2_rescale() {
        // TYPE_LabV2_16 rescales V2<->V4 (FomLabV2ToLabV4 / FomLabV4ToLabV2).
        let fmt = FormatBuilder::new().colorspace(pt::LAB_V2).channels(3).bytes(2).build();
        let src_words = [0x8000u16, 0x4000, 0xC000];
        let mut src = vec![0u8; 6];
        for (i, &sw) in src_words.iter().enumerate() {
            wr_u16(&mut src, i * 2, sw);
        }
        let mut w = [0u16; crate::MAX_CHANNELS];
        get_input_formatter_16(fmt).unwrap()(fmt, &mut w, &src, 0);
        for (i, &sw) in src_words.iter().enumerate() {
            assert_eq!(w[i], fom_lab_v2_to_v4(sw));
        }
        let mut dst = vec![0u8; 6];
        get_output_formatter_16(fmt).unwrap()(fmt, &w, &mut dst, 0);
        for (i, &wv) in w[..3].iter().enumerate() {
            assert_eq!(rd_u16(&dst, i * 2), fom_lab_v4_to_v2(wv));
        }
    }

    #[test]
    fn every_named_format_selects_a_formatter() {
        let f16 = [
            FormatBuilder::new().colorspace(pt::RGB).channels(3).bytes(1).build(),
            FormatBuilder::new().colorspace(pt::RGB).extra(1).channels(3).bytes(1).build(),
            FormatBuilder::new().colorspace(pt::RGB).channels(3).bytes(2).endian16(true).build(),
            FormatBuilder::new().colorspace(pt::CMYK).channels(4).bytes(2).build(),
            FormatBuilder::new().colorspace(pt::GRAY).channels(1).bytes(2).build(),
        ];
        for f in f16 {
            assert!(get_input_formatter_16(f).is_some());
            assert!(get_output_formatter_16(f).is_some());
        }
        let ffloat = [
            FormatBuilder::new().float(true).colorspace(pt::RGB).channels(3).bytes(4).build(),
            FormatBuilder::new().float(true).colorspace(pt::CMYK).channels(4).bytes(0).build(),
        ];
        for f in ffloat {
            assert!(get_input_formatter_float(f).is_some());
            assert!(get_output_formatter_float(f).is_some());
        }
    }

    #[test]
    fn formatter_for_colorspace_words() {
        use crate::types::sig::colorspace;

        // RGB, 1 byte, integer == TYPE_RGB_8: PT_RGB(4), 3 channels, 1 byte, not float.
        let rgb8 = formatter_for_colorspace(colorspace::RGB, 1, false);
        assert_eq!(rgb8.colorspace(), pt::RGB);
        assert_eq!(rgb8.channels(), 3);
        assert_eq!(rgb8.bytes(), 1);
        assert!(!rgb8.float());
        assert_eq!(
            rgb8,
            FormatBuilder::new().colorspace(pt::RGB).channels(3).bytes(1).build()
        );

        // CMYK, 2 bytes, integer.
        let cmyk16 = formatter_for_colorspace(colorspace::CMYK, 2, false);
        assert_eq!(cmyk16.colorspace(), pt::CMYK);
        assert_eq!(cmyk16.channels(), 4);
        assert_eq!(cmyk16.bytes(), 2);
        assert!(!cmyk16.float());

        // Lab, float.
        let lab_f = formatter_for_colorspace(colorspace::LAB, 4, true);
        assert_eq!(lab_f.colorspace(), pt::LAB);
        assert_eq!(lab_f.channels(), 3);
        assert_eq!(lab_f.bytes(), 4);
        assert!(lab_f.float());

        // Gray, double: n_bytes 8 masks to the T_BYTES==0 sentinel.
        let gray_d = formatter_for_colorspace(colorspace::GRAY, 8, true);
        assert_eq!(gray_d.channels(), 1);
        assert_eq!(gray_d.bytes(), 0);
        assert!(gray_d.float());

        // Unsupported colour space → all-zero word (C `return 0`).
        assert_eq!(
            formatter_for_colorspace(crate::types::sig::Signature::new(b"zzzz"), 1, false),
            PixelFormat::from_raw(0)
        );
    }

    #[test]
    fn formatter_for_pcs_words() {
        use crate::types::sig::colorspace;

        // PCS XYZ, float.
        let xyz = formatter_for_pcs(colorspace::XYZ, 4, true);
        assert_eq!(xyz.colorspace(), pt::XYZ);
        assert_eq!(xyz.channels(), 3);
        assert_eq!(xyz.bytes(), 4);
        assert!(xyz.float());

        // PCS Lab, 2-byte integer; identical to the colorspace variant for the same sig.
        let lab = formatter_for_pcs(colorspace::LAB, 2, false);
        assert_eq!(lab, formatter_for_colorspace(colorspace::LAB, 2, false));
        assert_eq!(lab.colorspace(), pt::LAB);
        assert_eq!(lab.channels(), 3);
        assert_eq!(lab.bytes(), 2);
    }
}
