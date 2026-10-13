//! Code-block coding-style flags: the single source of truth shared by the COD/COC parser, both
//! packet-header parsers and the Part-1 block decoder. Values follow the SPcod/SPcoc code-block
//! style byte (T.800 Table A.19, T.814 for the HT bits) widened to `u16` so the parsers' internal
//! `HT_PHLD` flag fits next to them.

/// Selective arithmetic-coding bypass ("lazy"): from pass 10 on, SigProp/MagRef are RAW-coded.
pub const BYPASS: u16 = 0x001;
/// Reset the MQ context states after every coding pass.
pub const RESET: u16 = 0x002;
/// Terminate the codeword on every coding pass (each pass is its own segment).
pub const RESTART: u16 = 0x004;
/// Vertically causal context formation (also valid for HT blocks).
pub const CAUSAL: u16 = 0x008;
/// Predictable termination: changes only how the ENCODER terminates a segment.
pub const ERTERM: u16 = 0x010;
/// Segmentation symbols at the end of every cleanup pass.
pub const SEGMARK: u16 = 0x020;
/// HT (HTJ2K, Part 15) block coding.
pub const HT: u16 = 0x040;
/// With [`HT`]: HT and original (Part-1) code-blocks may be mixed in one component.
pub const HT_MIXED: u16 = 0x080;
/// Parser-internal (never signalled): the block has not yet produced its HT cleanup pass.
pub const HT_PHLD: u16 = 0x100;

/// The six Part-1 flags.
pub const PART1_MASK: u16 = 0x3F;
/// Styles that split a block into several codeword segments (T.800 Table D.9); the packet parsers
/// key the per-segment length model off these. RESET/CAUSAL/ERTERM/SEGMARK keep one segment.
pub const SEG_MODES: u16 = BYPASS | RESTART;

/// Part-1 flags of a raw SPcod/SPcoc style byte.
#[inline]
pub const fn part1(block_style: u8) -> u16 {
    block_style as u16 & PART1_MASK
}

/// Whether `block_style` (SPcod/SPcoc byte) is decodable: HT blocks take vertical-causal and the
/// MIXED flag; Part-1 blocks take the six T.800 flags; nothing else.
pub const fn style_ok(block_style: u8) -> bool {
    let bs = block_style as u16;
    if bs & HT != 0 {
        bs & !(HT | HT_MIXED | CAUSAL) == 0
    } else {
        bs & !PART1_MASK == 0
    }
}

/// Exclusive end pass index of the codeword segment that contains coding pass `p` (T.800 D.6/D.9).
/// Pass 0 is the first cleanup; passes then cycle SigProp, MagRef, Cleanup. Single source of truth
/// for the packet-header length model (single- and multi-layer) and the block decoder:
/// * RESTART — every pass terminates: `p + 1`;
/// * BYPASS — passes `0..10` share one MQ segment, afterwards SigProp+MagRef form one RAW segment
///   and each Cleanup its own MQ segment (`2, 1, 2, 1, …`);
/// * otherwise the whole block is a single segment (`u32::MAX`).
pub fn seg_end(cmodes: u16, p: u32) -> u32 {
    if cmodes & RESTART != 0 {
        p + 1
    } else if cmodes & BYPASS != 0 {
        if p < 10 {
            10
        } else {
            match (p - 10) % 3 {
                2 => p + 1,
                r => p - r + 2,
            }
        }
    } else {
        u32::MAX
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Segment boundaries per T.800 Table D.9: BYPASS = 10 passes, then (SP+MR), CU, (SP+MR), CU, …;
    /// RESTART = one pass each; other styles never split.
    #[test]
    fn seg_end_follows_bypass_and_restart_rules() {
        let ends = |m: u16| {
            let (mut p, mut v) = (0u32, vec![]);
            while p < 18 {
                p = seg_end(m, p);
                v.push(p);
            }
            v
        };
        assert_eq!(ends(BYPASS), [10, 12, 13, 15, 16, 18]);
        assert_eq!(ends(RESTART), (1..=18).collect::<Vec<_>>());
        assert_eq!(ends(BYPASS | RESTART), (1..=18).collect::<Vec<_>>());
        assert_eq!(ends(ERTERM | SEGMARK | CAUSAL | RESET), [u32::MAX]);
        // Mid-segment lookups (a layer contribution continuing an open segment).
        assert_eq!(seg_end(BYPASS, 4), 10);
        assert_eq!(seg_end(BYPASS, 11), 12);
        assert_eq!(seg_end(BYPASS, 14), 15);
    }

    #[test]
    fn style_rules() {
        assert!(style_ok(0x40) && style_ok(0x40 | 0x80 | 0x08) && style_ok(0x3F));
        assert!(!style_ok(0x40 | 0x04) && !style_ok(0x80) && !style_ok(0x40 | 0x01));
        assert_eq!(part1(0xFF), 0x3F);
    }
}
