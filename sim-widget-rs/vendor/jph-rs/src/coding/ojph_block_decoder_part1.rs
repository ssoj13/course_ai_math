//! JPEG 2000 **Part-1** (legacy, non-HT) EBCOT block decoder — the three bit-plane coding passes
//! (significance propagation, magnitude refinement, cleanup) driven by the [`MqDecoder`] arithmetic
//! coder. OpenJPH is HT-only and ships no Part-1 decoder, so this is a fresh port of the clean
//! OpenHTJ2K reference (`source/core/coding/block_decoding.cpp`, function `j2k_decode`).
//!
//! ## Output contract (why this reuses the HT downstream verbatim)
//! Each decoded coefficient is written into the codeblock buffer in the SAME layout the HT decoder
//! ([`crate::coding::ojph_block_decoder32`]) produces, so the existing dequant / `tx_from_cb32` /
//! inverse-DWT path consumes it unchanged:
//! - bit 31 = sign (1 ⇒ negative);
//! - magnitude bit decoded on bit-plane `p` is stored at absolute bit position `p`, the MSB plane
//!   being `pmsb = 30 − missing_msbs`. Decoding the full depth places the magnitude LSB at
//!   `31 − k_max`, exactly the shift `gen_rev_tx_from_cb32` divides out (`>> (31 − k_max)`), so no
//!   re-normalisation is needed here.
//! - For irreversible (9/7) blocks a half-step reconstruction offset is OR-ed in at `p_last − 1`
//!   (the analogue of the HT `+2` bias); it is harmless for reversible blocks (it would be shifted
//!   out) but is gated on `!reversible` to keep the lossless path provably bit-exact.
//!
//! ## Scope
//! All six Part-1 styles: RESET, CAUSAL, SEGMARK, ERTERM (no decoder effect), and the segmenting
//! BYPASS / RESTART, whose per-pass segment lengths come from the packet parsers (segment model:
//! [`crate::coding::ojph_cmodes::seg_end`]). Inconsistent input is **rejected loudly** (`false`).
//!
//! Port target: <https://github.com/osamu620/OpenHTJ2K> `source/core/coding/block_decoding.cpp`.

use crate::coding::ojph_mq_decoder::MqDecoder;
use crate::coding::ojph_t1_luts::{CTX_RL, CTX_UNI, SIGN_LUT, SIG_LUT};
use crate::openjph::ojph_defs::{Ui32, Ui8};

use crate::coding::ojph_cmodes::{BYPASS, CAUSAL, RESET, SEGMARK, SEG_MODES, seg_end};

// Per-sample `block_states` bits (OpenHTJ2K `coding_local.hpp`: SHIFT_SIGMA=0, SHIFT_SIGMA_=1,
// SHIFT_PI_=2). We never store the decoded-bitplane index (SHIFT_P) — it is only used by
// OpenHTJ2K's own dequant, which we bypass.
const SIGMA: u8 = 1 << 0; // current significance
const SIGMA_: u8 = 1 << 1; // has been refined at least once (magref state)
const PI: u8 = 1 << 2; // visited during this bit-plane's sigprop pass
                       // Bits 3..=7 hold the last bit-plane index a pass coded this sample at (OpenHTJ2K SHIFT_P=3). The
                       // irreversible reconstruction midpoint is `1 << (plane - 1)` PER SAMPLE, where this plane differs
                       // between samples last touched by magnitude refinement vs the final cleanup.
const SHIFT_P: u8 = 3;

/// EBCOT decode state for one codeblock. `samples` is the borderless coefficient grid that becomes
/// the output (sign at bit 31, magnitude at absolute bit-plane positions); `states` is the σ/σ̲/π
/// significance grid with a 1-sample zero border on every side (so neighbour reads never branch).
struct Ebcot<'a> {
    samples: Vec<i32>,
    states: Vec<u8>,
    w: usize,
    h: usize,
    /// `states` row stride (`w + 2`, for the 1-sample border).
    sw: usize,
    /// Subband orientation 0=LL,1=HL,2=LH,3=HH — selects the zero-coding context LUT.
    orient: usize,
    causal: bool,
    mq: MqDecoder<'a>,
}

impl<'a> Ebcot<'a> {
    fn new(coded: &'a [u8], w: usize, h: usize, orient: usize, causal: bool) -> Self {
        let sw = w + 2;
        Self {
            samples: vec![0i32; w * h],
            states: vec![0u8; sw * (h + 2)],
            w,
            h,
            sw,
            orient,
            causal,
            mq: MqDecoder::new(coded),
        }
    }

    /// 8-bit zero-coding neighbour-significance index, packed as
    /// `TL=0 TR=1 BL=2 BR=3 T=4 B=5 L=6 R=7` (the layout [`SIG_LUT`] is indexed by).
    #[inline]
    fn zc_idx(&self, j1: usize, j2: usize) -> usize {
        let b = (j1 + 1) * self.sw + (j2 + 1);
        let s = &self.states;
        let g = |off: usize| (s[off] & SIGMA) as usize;
        let up = b - self.sw;
        let dn = b + self.sw;
        g(up - 1)
            | (g(up + 1) << 1)
            | (g(dn - 1) << 2)
            | (g(dn + 1) << 3)
            | (g(up) << 4)
            | (g(dn) << 5)
            | (g(b - 1) << 6)
            | (g(b + 1) << 7)
    }

    /// Zero-coding context label (0..=8). Under CAUSAL coding the bottom-row samples of a stripe
    /// (`j1 % 4 == 3`) ignore the "row below" neighbours BL/B/BR (mask `0xD3`), matching
    /// `get_context_label_sig`.
    #[inline]
    fn zc_label(&self, j1: usize, j2: usize) -> u8 {
        let mut idx = self.zc_idx(j1, j2);
        if self.causal && j1 % 4 == 3 {
            idx &= 0xD3;
        }
        SIG_LUT[self.orient][idx]
    }

    /// Sign bit (0/1) of a sample (bit 31 of its coefficient word).
    #[inline]
    fn sgn(&self, j1: usize, j2: usize) -> usize {
        ((self.samples[j1 * self.w + j2] as u32) >> 31) as usize
    }

    /// 8-bit sign-coding index, packed as `T-σ=0 B-σ=1 L-σ=2 R-σ=3 T-sgn=4 B-sgn=5 L-sgn=6 R-sgn=7`
    /// (the layout [`SIGN_LUT`] is indexed by; see `get_signLUT_index`).
    #[inline]
    fn sign_idx(&self, j1: usize, j2: usize) -> usize {
        let b = (j1 + 1) * self.sw + (j2 + 1);
        let s = &self.states;
        let mut idx = (s[b - self.sw] & SIGMA) as usize; // top σ
        idx |= ((s[b + self.sw] & SIGMA) as usize) << 1; // bottom σ
        idx |= ((s[b - 1] & SIGMA) as usize) << 2; // left σ
        idx |= ((s[b + 1] & SIGMA) as usize) << 3; // right σ
        if j1 > 0 {
            idx |= self.sgn(j1 - 1, j2) << 4;
        }
        if j1 + 1 < self.h {
            idx |= self.sgn(j1 + 1, j2) << 5;
        }
        if j2 > 0 {
            idx |= self.sgn(j1, j2 - 1) << 6;
        }
        if j2 + 1 < self.w {
            idx |= self.sgn(j1, j2 + 1) << 7;
        }
        idx
    }

    /// Decode and store the sign of a freshly-significant sample. Normally the sign is context-coded
    /// (label + prediction XOR bit from [`SIGN_LUT`]); in a RAW (arithmetic-bypass) pass it is a
    /// single un-predicted raw bit (`decode_j2k_sign_raw`).
    #[inline]
    fn decode_sign(&mut self, j1: usize, j2: usize, raw: bool) {
        let bit = if raw {
            self.mq.get_raw_symbol() & 1
        } else {
            let mut idx = self.sign_idx(j1, j2);
            if self.causal && j1 % 4 == 3 {
                idx &= 0xDD; // CAUSAL: drop bottom σ (bit1) and bottom sign (bit5)
            }
            (self.mq.decode(SIGN_LUT[0][idx]) ^ SIGN_LUT[1][idx]) & 1
        };
        self.samples[j1 * self.w + j2] |= (bit as i32) << 31;
    }

    /// One sample of the significance-propagation pass (extracted so both the full-stripe and the
    /// partial-bottom-stripe loops share it without duplicating the body).
    #[inline]
    fn sigprop_sample(&mut self, p: i32, j1: usize, j2: usize, raw: bool) {
        let sidx = (j1 + 1) * self.sw + (j2 + 1);
        let coded = (self.states[sidx] & SIGMA) == 0 && {
            let label = self.zc_label(j1, j2);
            if label > 0 {
                self.states[sidx] = (self.states[sidx] & 0x7) | ((p as u8) << SHIFT_P);
                // RAW (bypass) passes read an un-coded bit; otherwise context-code with `label`.
                let symbol = if raw {
                    self.mq.get_raw_symbol()
                } else {
                    self.mq.decode(label)
                };
                if symbol != 0 {
                    self.samples[j1 * self.w + j2] |= 1 << p;
                    self.states[sidx] |= SIGMA;
                    self.decode_sign(j1, j2, raw);
                }
                true
            } else {
                false
            }
        };
        if coded {
            self.states[sidx] |= PI;
        } else {
            self.states[sidx] &= !PI;
        }
    }

    /// Visit every sample in EBCOT 4-row stripe scan order (stripe -> column -> row within the
    /// stripe; a partial bottom stripe is just a shorter stripe). The single scan shared by the
    /// sigprop and magref passes.
    fn scan(&mut self, mut f: impl FnMut(&mut Self, usize, usize)) {
        for j1_start in (0..self.h).step_by(4) {
            let end = (j1_start + 4).min(self.h);
            for j2 in 0..self.w {
                for j1 in j1_start..end {
                    f(self, j1, j2);
                }
            }
        }
    }

    /// Significance-propagation pass for bit-plane `p`: code every still-insignificant sample that
    /// has at least one significant neighbour. Iterated in EBCOT 4-row **stripe** scan order
    /// (stripe → column → row-within-stripe) — the order the MQ symbol stream was produced in, so
    /// any other nesting would consume the arithmetic bits in the wrong sample order.
    fn sigprop(&mut self, p: i32, raw: bool) {
        self.scan(|e, j1, j2| e.sigprop_sample(p, j1, j2, raw));
    }

    /// One sample of the magnitude-refinement pass.
    #[inline]
    fn magref_sample(&mut self, p: i32, j1: usize, j2: usize, raw: bool) {
        const MMM: [u8; 4] = [14, 15, 16, 16];
        let sidx = (j1 + 1) * self.sw + (j2 + 1);
        let st = self.states[sidx];
        if (st & SIGMA) != 0 && (st & PI) == 0 {
            self.states[sidx] = (st & 0x7) | ((p as u8) << SHIFT_P);
            // RAW (bypass) passes read an un-coded bit; otherwise context-code with the MR label.
            let symbol = if raw {
                self.mq.get_raw_symbol()
            } else {
                let refined = (st & SIGMA_) != 0;
                let label_sig = self.zc_label(j1, j2);
                let mi = (usize::from(refined) << 1) | usize::from(label_sig > 0);
                self.mq.decode(MMM[mi])
            };
            self.samples[j1 * self.w + j2] |= (symbol as i32) << p;
            self.states[sidx] |= SIGMA_;
        }
    }

    /// Magnitude-refinement pass for bit-plane `p`: code one refinement bit for every sample that
    /// is already significant but was not visited by this plane's sigprop pass. Same 4-row stripe
    /// scan order as [`Self::sigprop`].
    fn magref(&mut self, p: i32, raw: bool) {
        self.scan(|e, j1, j2| e.magref_sample(p, j1, j2, raw));
    }

    /// True when the aligned 4-sample column at `(j1_start, j2)` qualifies for run-length coding:
    /// every one of the four samples has an all-insignificant neighbourhood (ZC label 0). Because
    /// the four samples are themselves insignificant in the cleanup pass, this is equivalent to the
    /// reference's packed-context mask test.
    #[inline]
    fn col_runnable(&self, j1_start: usize, j2: usize) -> bool {
        (0..4).all(|d| self.zc_label(j1_start + d, j2) == 0)
    }

    /// Cleanup pass for bit-plane `p`, including the run-length primitive for columns of four
    /// fully-insignificant samples (contexts RL=17, UNI=18).
    fn cleanup(&mut self, p: i32) {
        let num_v_stripe = self.h / 4;
        for n in 0..num_v_stripe {
            let j1_start = n * 4;
            for j2 in 0..self.w {
                let mut k: i32 = 4;
                while k > 0 {
                    let mut j1 = j1_start + (4 - k) as usize;
                    let mut r: i32 = -1;
                    if j1.is_multiple_of(4) && j1 + 4 <= self.h && self.col_runnable(j1, j2) {
                        let symbol = self.mq.decode(CTX_RL);
                        if symbol == 0 {
                            r = 4;
                        } else {
                            r = self.mq.decode(CTX_UNI) as i32;
                            r = (r << 1) + self.mq.decode(CTX_UNI) as i32;
                            self.samples[(j1 + r as usize) * self.w + j2] |= (symbol as i32) << p;
                        }
                        k -= r;
                        if k != 0 {
                            j1 = j1_start + (4 - k) as usize;
                        }
                    }
                    let sidx = (j1 + 1) * self.sw + (j2 + 1);
                    if (self.states[sidx] & SIGMA) == 0 && (self.states[sidx] & PI) == 0 {
                        self.states[sidx] = (self.states[sidx] & 0x7) | ((p as u8) << SHIFT_P);
                        // `r >= 0` ⇒ this sample is the run primitive's first-significant one and was
                        // already placed above; otherwise code its significance bit normally.
                        if r < 0 {
                            let label = self.zc_label(j1, j2);
                            let symbol = self.mq.decode(label);
                            self.samples[j1 * self.w + j2] |= (symbol as i32) << p;
                        }
                        if self.samples[j1 * self.w + j2] == (1i32 << p) {
                            self.states[sidx] |= SIGMA;
                            self.decode_sign(j1, j2, false);
                        }
                    }
                    k -= 1;
                }
            }
        }

        // Partial bottom stripe (height % 4): no run-length coding, plain significance coding.
        if !self.h.is_multiple_of(4) {
            let j1_start = num_v_stripe * 4;
            for j2 in 0..self.w {
                for j1 in j1_start..self.h {
                    let sidx = (j1 + 1) * self.sw + (j2 + 1);
                    if (self.states[sidx] & SIGMA) == 0 && (self.states[sidx] & PI) == 0 {
                        self.states[sidx] = (self.states[sidx] & 0x7) | ((p as u8) << SHIFT_P);
                        let label = self.zc_label(j1, j2);
                        let symbol = self.mq.decode(label);
                        self.samples[j1 * self.w + j2] |= (symbol as i32) << p;
                        if symbol != 0 {
                            self.states[sidx] |= SIGMA;
                            self.decode_sign(j1, j2, false);
                        }
                    }
                }
            }
        }
    }
}

/// Decode one Part-1 codeblock into `decoded` (row stride `stride`, in `u32` units). `coded` is the
/// codeblock's full compressed byte segment, `missing_msbs` the leading all-zero bit-plane count,
/// `num_passes` the active (post-placeholder) coding-pass count, `orientation` the subband band
/// number (0..3), `cmodes` the COD/COC coding-style flags, and `reversible` whether the component
/// uses the 5/3 (lossless) transform. `seg_lengths` holds one byte count per pass for
/// segmented styles (RESTART/BYPASS, see [`seg_end`]; a contribution's bytes sit on its last pass),
/// and is empty for every other Part-1 block (which forms a single segment over all of `coded`). `k_max` is the subband's `M_b`: it fixes the quantization
/// LSB plane (`pLSB = 31 - M_b`) that the irreversible reconstruction midpoint never goes below.
///
/// Returns `false` (a loud reject — never a silent partial decode) when the geometry is degenerate,
/// `missing_msbs` is out of range, an unsupported coding mode is requested, the pass schedule would
/// underflow past bit-plane 0, the RESTART per-pass lengths are missing, or a SEGMARK check fails.
#[allow(clippy::too_many_arguments)]
pub(crate) fn decode_codeblock_part1(
    coded: &[u8],
    decoded: &mut [Ui32],
    stride: usize,
    missing_msbs: Ui32,
    num_passes: Ui32,
    width: usize,
    height: usize,
    orientation: Ui8,
    cmodes: u16,
    reversible: bool,
    roishift: u8,
    seg_lengths: &[Ui32],
    k_max: Ui32,
) -> bool {
    if width == 0 || height == 0 || missing_msbs > 30 || orientation > 3 {
        return false;
    }
    // BYPASS (arithmetic-coding bypass / "lazy"): passes 10+ that are SigProp/MagRef are RAW-coded.
    // ERTERM (predictable termination) only changes how the encoder terminates a segment, so it
    // needs no decoder support. When the style splits the block into several segments (see
    // [`seg_end`]) the packet parser supplies one length per pass (the contribution's bytes sit on
    // its last pass, zeros elsewhere); a mismatch is a corrupt/parse-desynced stream.
    let segmented = cmodes & SEG_MODES != 0;
    let bypass = cmodes & BYPASS != 0;
    if segmented && seg_lengths.len() != num_passes as usize {
        return false;
    }

    let mut e = Ebcot::new(
        coded,
        width,
        height,
        orientation as usize,
        cmodes & CAUSAL != 0,
    );

    let pmsb = 30i32 - missing_msbs as i32;
    let mut p = pmsb;
    let mut z = 0u32;
    let mut k = 2u8; // first pass is a cleanup at the MSB plane

    // Unsegmented: the whole block is a single MQ codeword segment — prime once over all bytes.
    // Segmented styles prime a fresh segment at each segment start instead (see the loop body).
    let mut segment_pos = 0u32;
    let mut next_seg = 0u32; // first pass of the next segment
    if !segmented {
        e.mq.init(0, coded.len() as Ui32, false);
    }

    while z < num_passes {
        if k == 3 {
            if p <= 0 {
                return false; // schedule would step below bit-plane 0 — malformed
            }
            k = 0;
            p -= 1;
        }
        // BYPASS: from pass index 10 on, SigProp (k=0) and MagRef (k=1) are arithmetic-bypass (RAW);
        // Cleanup (k=2) and the first 10 passes stay MQ-coded. Port of OpenHTJ2K `j2k_decode`'s
        // `bypass_threshold = 10` schedule.
        let is_bypass = bypass && z >= 10 && k != 2;
        // RESTART: begin this pass's own codeword segment before (re)initialising contexts. Port of
        // OpenHTJ2K `j2k_decode`: segment init (RAW or MQ per `is_bypass`), then state init, then the
        // pass. Context STATES are still only reset at z==0 (or every pass under RESET=0x02).
        if segmented && z == next_seg {
            next_seg = seg_end(cmodes, z);
            let last = next_seg.min(num_passes) as usize;
            let len = seg_lengths[z as usize..last].iter().sum::<Ui32>();
            e.mq.init(segment_pos, len, is_bypass);
            segment_pos += len;
        }
        if z == 0 || cmodes & RESET != 0 {
            e.mq.init_states_for_all_contexts();
        }
        match k {
            0 => e.sigprop(p, is_bypass),
            1 => e.magref(p, is_bypass),
            _ => {
                e.cleanup(p);
                if cmodes & SEGMARK != 0 {
                    let mut marker = 0i32;
                    for _ in 0..4 {
                        marker = (marker << 1) + e.mq.decode(CTX_UNI) as i32;
                    }
                    if marker != 0b1010 {
                        return false; // SEGMARK mismatch ⇒ corrupt segment
                    }
                }
            }
        }
        z += 1;
        k += 1;
    }

    // Irreversible reconstruction midpoint (T.800 dequant; port of OpenHTJ2K `j2k_dequant`): each
    // significant sample gets a half-step `1 << (plane - 1)` at ITS OWN last-coded bit-plane, read
    // from the per-sample SHIFT_P state. (A global plane is wrong: samples last touched by magref
    // sit one plane above those resolved in the final cleanup, and at coarse 12-bit quantisation
    // that one-plane error is hundreds of levels.) Skipped for reversible blocks (must stay
    // bit-exact; the `gen_rev_tx_from_cb32` shift would discard it anyway).
    //
    // Under ROI max-shift (`roishift != 0`) the offset is NOT per-sample: OpenHTJ2K forces
    // `N_b = M_b`, i.e. a FIXED `1 << (pLSB - 1)` added to all non-zero samples AFTER the
    // background up-shift. That happens in `Codeblock::apply_roi` instead, so we must leave the
    // magnitude reconstruction-free here when ROI is active.
    //
    // OpenHTJ2K clamps the plane to the quantization LSB `pLSB = 31 - M_b` (`N_b > M_b` still adds
    // `1 << (pLSB - 1)`, half a quantization step): blocks inside HT MIXED streams are routinely
    // coded BELOW that plane, and without the clamp the tiny own-plane offset left lossy output off
    // by hundreds of levels (conformance ds0_hm_06).
    if !reversible && roishift == 0 {
        let plsb = 31 - k_max as i32;
        for j1 in 0..height {
            for j2 in 0..width {
                let st = e.states[(j1 + 1) * e.sw + (j2 + 1)];
                if st & SIGMA != 0 {
                    let plane = ((st >> SHIFT_P) as i32).max(plsb);
                    if plane >= 1 {
                        e.samples[j1 * width + j2] |= 1 << (plane - 1);
                    }
                }
            }
        }
    }

    for j1 in 0..height {
        for j2 in 0..width {
            decoded[j1 * stride + j2] = e.samples[j1 * width + j2] as Ui32;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Structural smoke test: a small block decodes without panicking and is deterministic. (Bit
    /// exactness is covered end-to-end by the `.pgx` conformance comparison.)
    #[test]
    fn decode_runs_and_is_deterministic() {
        let coded: [u8; 12] = [
            0xA3, 0x5C, 0x00, 0xFF, 0x91, 0x7E, 0x42, 0x18, 0x0D, 0xB6, 0x24, 0x99,
        ];
        let run = || {
            let mut out = vec![0u32; 8 * 8];
            let ok = decode_codeblock_part1(&coded, &mut out, 8, 0, 4, 8, 8, 3, 0, true, 0, &[], 9);
            (ok, out)
        };
        let a = run();
        let b = run();
        assert!(a.0, "decode should succeed on well-formed-length input");
        assert_eq!(a.1, b.1, "Part-1 decode must be deterministic");
    }

    /// Missing segment lengths and bad geometry must reject loudly, not decode silently.
    #[test]
    fn rejects_unsupported_and_degenerate() {
        use crate::coding::ojph_cmodes::RESTART;
        let coded = [0u8; 4];
        let mut out = vec![0u32; 16];
        // RESTART with NO per-pass lengths supplied must reject (len mismatch).
        assert!(!decode_codeblock_part1(
            &coded,
            &mut out,
            4,
            0,
            1,
            4,
            4,
            0,
            RESTART,
            true,
            0,
            &[],
            9,
        ));
        assert!(!decode_codeblock_part1(
            &coded,
            &mut out,
            4,
            0,
            1,
            4,
            4,
            0,
            BYPASS,
            true,
            0,
            &[],
            9,
        ));
        assert!(!decode_codeblock_part1(
            &coded,
            &mut out,
            4,
            31,
            1,
            4,
            4,
            0,
            0,
            true,
            0,
            &[],
            9,
        ));
        assert!(!decode_codeblock_part1(
            &coded,
            &mut out,
            4,
            0,
            1,
            0,
            4,
            0,
            0,
            true,
            0,
            &[],
            9,
        ));
    }
}
