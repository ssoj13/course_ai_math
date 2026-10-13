//! The packed pixel-format descriptor.
//!
//! lcms2 encodes a buffer's layout in a single 32-bit word built from the `*_SH` macros
//! and decoded with the `T_*` macros (`lcms2.h`). Rather than scatter bit-twiddling
//! across the formatters, we wrap the word in [`PixelFormat`] and expose the exact same
//! fields as methods. Bit layout (verified against `lcms2.h` lines 713–738):
//!
//! ```text
//!  bit 23    premul        bits 16..=20  colorspace (5)   bit 11  endian16
//!  bit 22    float          bit 14       swapfirst         bit 10  doswap
//!  bit 21    optimized      bit 13       flavor           bits 7..=9  extra (3)
//!                            bit 12       planar           bits 3..=6  channels (4)
//!                                                          bits 0..=2  bytes (3)
//! ```
//!
//! The `PT_*` colour-space codes that go in the `colorspace` field are added with the
//! pixel formatters in Phase 4; here we only provide the encode/decode mechanics.

/// A packed lcms2 pixel format (`cmsUInt32Number` format word).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PixelFormat(pub u32);

impl PixelFormat {
    /// Wrap a raw format word.
    #[inline]
    pub const fn from_raw(v: u32) -> Self {
        PixelFormat(v)
    }

    /// The raw format word.
    #[inline]
    pub const fn raw(self) -> u32 {
        self.0
    }

    // --- decoders (the `T_*` macros) ---

    /// `T_PREMUL` — alpha is premultiplied.
    #[inline]
    pub const fn premul(self) -> bool {
        (self.0 >> 23) & 1 != 0
    }
    /// `T_FLOAT` — samples are floating point (else integer).
    #[inline]
    pub const fn float(self) -> bool {
        (self.0 >> 22) & 1 != 0
    }
    /// `T_OPTIMIZED` — an optimized/pre-linearized format.
    #[inline]
    pub const fn optimized(self) -> bool {
        (self.0 >> 21) & 1 != 0
    }
    /// `T_COLORSPACE` — 5-bit `PT_*` colour-space code.
    #[inline]
    pub const fn colorspace(self) -> u32 {
        (self.0 >> 16) & 31
    }
    /// `T_SWAPFIRST` — rotate so the last channel comes first (e.g. CMYK→KCMY).
    #[inline]
    pub const fn swapfirst(self) -> bool {
        (self.0 >> 14) & 1 != 0
    }
    /// `T_FLAVOR` — 0 = min-is-black, 1 = min-is-white (inks).
    #[inline]
    pub const fn flavor(self) -> bool {
        (self.0 >> 13) & 1 != 0
    }
    /// `T_PLANAR` — planar (else chunky/interleaved).
    #[inline]
    pub const fn planar(self) -> bool {
        (self.0 >> 12) & 1 != 0
    }
    /// `T_ENDIAN16` — 16-bit samples are byte-swapped.
    #[inline]
    pub const fn endian16(self) -> bool {
        (self.0 >> 11) & 1 != 0
    }
    /// `T_DOSWAP` — reverse channel order (e.g. RGB→BGR).
    #[inline]
    pub const fn doswap(self) -> bool {
        (self.0 >> 10) & 1 != 0
    }
    /// `T_EXTRA` — number of extra (e.g. alpha) channels, 0..=7.
    #[inline]
    pub const fn extra(self) -> u32 {
        (self.0 >> 7) & 7
    }
    /// `T_CHANNELS` — number of colour channels, 0..=15.
    #[inline]
    pub const fn channels(self) -> u32 {
        (self.0 >> 3) & 15
    }
    /// `T_BYTES` — bytes per sample; **0 means 8 (a `double`)** in lcms2.
    #[inline]
    pub const fn bytes(self) -> u32 {
        self.0 & 7
    }
}

/// Builder for a [`PixelFormat`], mirroring the `*_SH` OR-together macros.
///
/// ```
/// # use lcms_rs::types::format::FormatBuilder;
/// // TYPE_RGB_8 = COLORSPACE_SH(PT_RGB=4) | CHANNELS_SH(3) | BYTES_SH(1)
/// let rgb8 = FormatBuilder::new().colorspace(4).channels(3).bytes(1).build();
/// assert_eq!(rgb8.channels(), 3);
/// assert_eq!(rgb8.bytes(), 1);
/// assert!(!rgb8.float());
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct FormatBuilder(u32);

impl FormatBuilder {
    #[inline]
    pub const fn new() -> Self {
        FormatBuilder(0)
    }
    #[inline]
    pub const fn premul(mut self, on: bool) -> Self {
        self.0 |= (on as u32) << 23;
        self
    }
    #[inline]
    pub const fn float(mut self, on: bool) -> Self {
        self.0 |= (on as u32) << 22;
        self
    }
    #[inline]
    pub const fn optimized(mut self, on: bool) -> Self {
        self.0 |= (on as u32) << 21;
        self
    }
    #[inline]
    pub const fn colorspace(mut self, pt: u32) -> Self {
        self.0 |= pt << 16;
        self
    }
    #[inline]
    pub const fn swapfirst(mut self, on: bool) -> Self {
        self.0 |= (on as u32) << 14;
        self
    }
    #[inline]
    pub const fn flavor(mut self, on: bool) -> Self {
        self.0 |= (on as u32) << 13;
        self
    }
    #[inline]
    pub const fn planar(mut self, on: bool) -> Self {
        self.0 |= (on as u32) << 12;
        self
    }
    #[inline]
    pub const fn endian16(mut self, on: bool) -> Self {
        self.0 |= (on as u32) << 11;
        self
    }
    #[inline]
    pub const fn doswap(mut self, on: bool) -> Self {
        self.0 |= (on as u32) << 10;
        self
    }
    #[inline]
    pub const fn extra(mut self, n: u32) -> Self {
        self.0 |= n << 7;
        self
    }
    #[inline]
    pub const fn channels(mut self, n: u32) -> Self {
        self.0 |= n << 3;
        self
    }
    #[inline]
    pub const fn bytes(mut self, n: u32) -> Self {
        self.0 |= n;
        self
    }
    #[inline]
    pub const fn build(self) -> PixelFormat {
        PixelFormat(self.0)
    }
}

/// The named `TYPE_*` pixel formats from `lcms2.h`.
///
/// lcms2 ships ~100 of these macros; they are how the API is documented and discussed, so a
/// consumer that has to re-derive `FormatBuilder::new().colorspace(pt::RGB as u32)...` for every
/// call is doing work the reference does not ask of them. [`FormatBuilder`] is `const fn`
/// throughout, so these are real constants with no runtime cost.
///
/// Only the common combinations are listed. Anything else composes from [`FormatBuilder`]; the
/// point here is to cover the formats a consumer reaches for daily, not to mirror all 100.
pub mod formats {
    use super::{FormatBuilder, PixelFormat};
    use crate::color::pcs::pt;

    /// `TYPE_GRAY_8` (`lcms2.h:778`).
    pub const GRAY_8: PixelFormat =
        FormatBuilder::new().colorspace(pt::GRAY as u32).channels(1).bytes(1).build();
    /// `TYPE_GRAY_16` (`lcms2.h:780`).
    pub const GRAY_16: PixelFormat =
        FormatBuilder::new().colorspace(pt::GRAY as u32).channels(1).bytes(2).build();
    /// `TYPE_GRAY_FLT` (`lcms2.h:952`).
    pub const GRAY_FLT: PixelFormat =
        FormatBuilder::new().float(true).colorspace(pt::GRAY as u32).channels(1).bytes(4).build();

    /// `TYPE_RGB_8` (`lcms2.h:791`).
    pub const RGB_8: PixelFormat =
        FormatBuilder::new().colorspace(pt::RGB as u32).channels(3).bytes(1).build();
    /// `TYPE_RGB_16` (`lcms2.h:795`).
    pub const RGB_16: PixelFormat =
        FormatBuilder::new().colorspace(pt::RGB as u32).channels(3).bytes(2).build();
    /// `TYPE_RGB_FLT` (`lcms2.h:955`).
    pub const RGB_FLT: PixelFormat =
        FormatBuilder::new().float(true).colorspace(pt::RGB as u32).channels(3).bytes(4).build();
    /// `TYPE_RGB_DBL` (`lcms2.h:974`) - `bytes(0)` is lcms's `double` sentinel, not "no bytes".
    pub const RGB_DBL: PixelFormat =
        FormatBuilder::new().float(true).colorspace(pt::RGB as u32).channels(3).bytes(0).build();

    /// `TYPE_BGR_8` (`lcms2.h:793`).
    pub const BGR_8: PixelFormat = FormatBuilder::new()
        .colorspace(pt::RGB as u32)
        .channels(3)
        .bytes(1)
        .doswap(true)
        .build();

    /// `TYPE_RGBA_8` (`lcms2.h:802`).
    pub const RGBA_8: PixelFormat = FormatBuilder::new()
        .colorspace(pt::RGB as u32)
        .extra(1)
        .channels(3)
        .bytes(1)
        .build();
    /// `TYPE_RGBA_16` (`lcms2.h:805`).
    pub const RGBA_16: PixelFormat = FormatBuilder::new()
        .colorspace(pt::RGB as u32)
        .extra(1)
        .channels(3)
        .bytes(2)
        .build();
    /// `TYPE_RGBA_FLT` (`lcms2.h:957`).
    pub const RGBA_FLT: PixelFormat = FormatBuilder::new()
        .float(true)
        .colorspace(pt::RGB as u32)
        .extra(1)
        .channels(3)
        .bytes(4)
        .build();
    /// `TYPE_BGRA_8` (`lcms2.h:824`).
    pub const BGRA_8: PixelFormat = FormatBuilder::new()
        .colorspace(pt::RGB as u32)
        .extra(1)
        .channels(3)
        .bytes(1)
        .doswap(true)
        .swapfirst(true)
        .build();

    /// `TYPE_CMYK_8` (`lcms2.h:837`).
    pub const CMYK_8: PixelFormat =
        FormatBuilder::new().colorspace(pt::CMYK as u32).channels(4).bytes(1).build();
    /// `TYPE_CMYK_16` (`lcms2.h:842`).
    pub const CMYK_16: PixelFormat =
        FormatBuilder::new().colorspace(pt::CMYK as u32).channels(4).bytes(2).build();
    /// `TYPE_CMYK_FLT` (`lcms2.h:967`).
    pub const CMYK_FLT: PixelFormat =
        FormatBuilder::new().float(true).colorspace(pt::CMYK as u32).channels(4).bytes(4).build();

    /// `TYPE_Lab_8` (`lcms2.h:908`).
    pub const LAB_8: PixelFormat =
        FormatBuilder::new().colorspace(pt::LAB as u32).channels(3).bytes(1).build();
    /// `TYPE_Lab_16` (`lcms2.h:913`).
    pub const LAB_16: PixelFormat =
        FormatBuilder::new().colorspace(pt::LAB as u32).channels(3).bytes(2).build();
    /// `TYPE_Lab_FLT` (`lcms2.h:950`).
    pub const LAB_FLT: PixelFormat =
        FormatBuilder::new().float(true).colorspace(pt::LAB as u32).channels(3).bytes(4).build();
    /// `TYPE_Lab_DBL` (`lcms2.h:972`).
    pub const LAB_DBL: PixelFormat =
        FormatBuilder::new().float(true).colorspace(pt::LAB as u32).channels(3).bytes(0).build();

    /// `TYPE_XYZ_FLT` (`lcms2.h:949`).
    pub const XYZ_FLT: PixelFormat =
        FormatBuilder::new().float(true).colorspace(pt::XYZ as u32).channels(3).bytes(4).build();
    /// `TYPE_XYZ_DBL` (`lcms2.h:971`).
    pub const XYZ_DBL: PixelFormat =
        FormatBuilder::new().float(true).colorspace(pt::XYZ as u32).channels(3).bytes(0).build();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_known_formats() {
        // TYPE_RGBA_8 = COLORSPACE_SH(4) | EXTRA_SH(1) | CHANNELS_SH(3) | BYTES_SH(1)
        let rgba8 = FormatBuilder::new().colorspace(4).extra(1).channels(3).bytes(1).build();
        assert_eq!(rgba8.colorspace(), 4);
        assert_eq!(rgba8.extra(), 1);
        assert_eq!(rgba8.channels(), 3);
        assert_eq!(rgba8.bytes(), 1);
        assert!(!rgba8.planar() && !rgba8.doswap() && !rgba8.float());

        // TYPE_CMYK_16 with doswap+swapfirst set independently decodes cleanly.
        let f = FormatBuilder::new()
            .colorspace(6)
            .channels(4)
            .bytes(2)
            .doswap(true)
            .swapfirst(true)
            .build();
        assert_eq!(f.channels(), 4);
        assert_eq!(f.bytes(), 2);
        assert!(f.doswap() && f.swapfirst());
        assert!(!f.planar());

        // bytes()==0 is the sentinel for a double (8 bytes) in lcms2.
        let dbl = FormatBuilder::new().float(true).channels(3).bytes(0).build();
        assert_eq!(dbl.bytes(), 0);
        assert!(dbl.float());
    }

    /// Each `formats::*` constant must decompose exactly the way its `lcms2.h` macro composes.
    /// Checked field by field rather than by eye - a wrong `EXTRA_SH`/`DOSWAP_SH` bit picks a
    /// different formatter at runtime and would otherwise surface as mangled pixels.
    #[test]
    fn named_formats_match_the_lcms2_macros() {
        use crate::color::pcs::pt;
        use formats::*;

        // TYPE_RGB_FLT = FLOAT_SH(1)|COLORSPACE_SH(PT_RGB)|CHANNELS_SH(3)|BYTES_SH(4)
        assert!(RGB_FLT.float());
        assert_eq!(RGB_FLT.colorspace(), pt::RGB as u32);
        assert_eq!(RGB_FLT.channels(), 3);
        assert_eq!(RGB_FLT.bytes(), 4);
        assert_eq!(RGB_FLT.extra(), 0);
        assert!(!RGB_FLT.doswap() && !RGB_FLT.swapfirst() && !RGB_FLT.planar());

        // The double variants use BYTES_SH(0) - the 8-byte sentinel, not "no bytes".
        assert!(RGB_DBL.float() && RGB_DBL.bytes() == 0 && RGB_DBL.channels() == 3);
        assert!(LAB_DBL.float() && LAB_DBL.bytes() == 0);
        assert!(XYZ_DBL.float() && XYZ_DBL.bytes() == 0);

        // EXTRA_SH(1) carries the alpha channel; CHANNELS stays 3.
        for f in [RGBA_8, RGBA_16, RGBA_FLT, BGRA_8] {
            assert_eq!(f.extra(), 1);
            assert_eq!(f.channels(), 3);
            assert_eq!(f.colorspace(), pt::RGB as u32);
        }

        // TYPE_BGR_8 is RGB with DOSWAP only; TYPE_BGRA_8 adds SWAPFIRST.
        assert!(BGR_8.doswap() && !BGR_8.swapfirst() && BGR_8.extra() == 0);
        assert!(BGRA_8.doswap() && BGRA_8.swapfirst());

        // Integer formats must not claim to be float.
        for f in [GRAY_8, GRAY_16, RGB_8, RGB_16, BGR_8, RGBA_8, RGBA_16, BGRA_8, CMYK_8,
                  CMYK_16, LAB_8, LAB_16] {
            assert!(!f.float(), "integer format flagged float: {:#x}", f.raw());
        }

        // Channel counts follow the colour space.
        assert_eq!((GRAY_8.channels(), GRAY_16.channels(), GRAY_FLT.channels()), (1, 1, 1));
        assert_eq!((CMYK_8.channels(), CMYK_16.channels(), CMYK_FLT.channels()), (4, 4, 4));
        assert_eq!(CMYK_FLT.colorspace(), pt::CMYK as u32);
        assert_eq!(LAB_FLT.colorspace(), pt::LAB as u32);
        assert_eq!(XYZ_FLT.colorspace(), pt::XYZ as u32);

        // Byte widths.
        assert_eq!((RGB_8.bytes(), RGB_16.bytes(), RGB_FLT.bytes()), (1, 2, 4));
    }
}
