//! ICC signatures (4-character codes).
//!
//! ICC uses 4-byte big-endian ASCII "signatures" for colour spaces, profile classes,
//! tags, tag types, platforms, etc. lcms2 models each family as its own C enum of
//! `u32` constants. We use one [`Signature`] newtype plus per-family constant modules;
//! values are derived from the 4CC bytes ([`Signature::new`]), so they are authoritative
//! rather than transcribed magic numbers, and match the `lcms2.h` enum values exactly.
//!
//! Only the families needed by the current phase are populated; the remaining tag /
//! tag-type signatures are added as Phase 3 (profile I/O) needs them.

/// A 4-byte ICC signature, stored as the big-endian `u32` lcms2 uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Signature(pub u32);

impl Signature {
    /// Build a signature from its four ASCII bytes (e.g. `Signature::new(b"RGB ")`).
    #[inline]
    pub const fn new(code: &[u8; 4]) -> Self {
        Signature(u32::from_be_bytes(*code))
    }

    /// The raw big-endian `u32` value.
    #[inline]
    pub const fn as_u32(self) -> u32 {
        self.0
    }

    /// The four ASCII bytes, big-endian order (as written to disk).
    #[inline]
    pub const fn to_bytes(self) -> [u8; 4] {
        self.0.to_be_bytes()
    }
}

impl From<u32> for Signature {
    #[inline]
    fn from(v: u32) -> Self {
        Signature(v)
    }
}

impl core::fmt::Display for Signature {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Render as the 4CC when printable, else as hex.
        let b = self.to_bytes();
        if b.iter().all(|&c| (0x20..=0x7e).contains(&c)) {
            write!(f, "'{}'", b.iter().map(|&c| c as char).collect::<String>())
        } else {
            write!(f, "{:#010x}", self.0)
        }
    }
}

/// Colour-space signatures (`cmsColorSpaceSignature`, `cmsSig*Data`).
pub mod colorspace {
    use super::Signature;

    pub const XYZ: Signature = Signature::new(b"XYZ ");
    pub const LAB: Signature = Signature::new(b"Lab ");
    pub const LUV: Signature = Signature::new(b"Luv ");
    pub const YCBCR: Signature = Signature::new(b"YCbr");
    pub const YXY: Signature = Signature::new(b"Yxy ");
    pub const RGB: Signature = Signature::new(b"RGB ");
    pub const GRAY: Signature = Signature::new(b"GRAY");
    pub const HSV: Signature = Signature::new(b"HSV ");
    pub const HLS: Signature = Signature::new(b"HLS ");
    pub const CMYK: Signature = Signature::new(b"CMYK");
    pub const CMY: Signature = Signature::new(b"CMY ");
    pub const NAMED: Signature = Signature::new(b"nmcl");

    // N-colorant spaces. Note the ICC quirk: 10..=15 use 'A'..='F' as the first byte.
    pub const CLR1: Signature = Signature::new(b"1CLR");
    pub const CLR2: Signature = Signature::new(b"2CLR");
    pub const CLR3: Signature = Signature::new(b"3CLR");
    pub const CLR4: Signature = Signature::new(b"4CLR");
    pub const CLR5: Signature = Signature::new(b"5CLR");
    pub const CLR6: Signature = Signature::new(b"6CLR");
    pub const CLR7: Signature = Signature::new(b"7CLR");
    pub const CLR8: Signature = Signature::new(b"8CLR");
    pub const CLR9: Signature = Signature::new(b"9CLR");
    pub const CLR10: Signature = Signature::new(b"ACLR");
    pub const CLR11: Signature = Signature::new(b"BCLR");
    pub const CLR12: Signature = Signature::new(b"CCLR");
    pub const CLR13: Signature = Signature::new(b"DCLR");
    pub const CLR14: Signature = Signature::new(b"ECLR");
    pub const CLR15: Signature = Signature::new(b"FCLR");
}

/// Profile-class signatures (`cmsProfileClassSignature`, `cmsSig*Class`).
pub mod class {
    use super::Signature;

    pub const INPUT: Signature = Signature::new(b"scnr");
    pub const DISPLAY: Signature = Signature::new(b"mntr");
    pub const OUTPUT: Signature = Signature::new(b"prtr");
    pub const LINK: Signature = Signature::new(b"link");
    pub const ABSTRACT: Signature = Signature::new(b"abst");
    pub const COLORSPACE: Signature = Signature::new(b"spac");
    pub const NAMED_COLOR: Signature = Signature::new(b"nmcl");
}

/// The ICC file magic (`acsp`), present in every profile header.
pub const MAGIC_NUMBER: Signature = Signature::new(b"acsp");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_values_match_lcms_header() {
        // Cross-checked against the numeric literals in lcms2.h.
        assert_eq!(colorspace::XYZ.as_u32(), 0x58595A20);
        assert_eq!(colorspace::RGB.as_u32(), 0x52474220);
        assert_eq!(colorspace::CMYK.as_u32(), 0x434D594B);
        assert_eq!(colorspace::CLR10.as_u32(), 0x41434C52); // 'ACLR'
        assert_eq!(class::DISPLAY.as_u32(), 0x6D6E7472);
        assert_eq!(class::LINK.as_u32(), 0x6C696E6B);
        assert_eq!(MAGIC_NUMBER.as_u32(), 0x61637370); // 'acsp'
    }

    #[test]
    fn roundtrip_bytes() {
        assert_eq!(colorspace::RGB.to_bytes(), *b"RGB ");
        assert_eq!(format!("{}", colorspace::CMYK), "'CMYK'");
    }
}
