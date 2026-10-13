//! JPEG 2000 **Part-1** MQ arithmetic decoder — the entropy coder used by the legacy (non-HT)
//! EBCOT bit-plane passes. OpenJPH is HT-only and has no MQ decoder, so this is a fresh port of the
//! clean OpenHTJ2K reference (`source/core/coding/mq_decoder.cpp`), whose internal representation
//! keeps the interval register `A` and the code register `C` left-shifted by `MQMINOFF = 8` bits
//! (`Areg_min = 1 << 23`); the "active" byte of `C` is `(C >> 8) & 0xFFFF`. The decoded bit-plane
//! passes (see `ojph_block_decoder_part1`) drive it via [`MqDecoder::decode`] (context-coded) and
//! [`MqDecoder::get_raw_symbol`] (RAW / arithmetic-bypass).
//!
//! Port target: <https://github.com/osamu620/OpenHTJ2K> `source/core/coding/mq_decoder.cpp`.

use crate::openjph::ojph_defs::{Ui16, Ui32, Ui8};

/// `Areg_min = 1 << (15 + MQMINOFF)` with `MQMINOFF = 8`: the renormalisation threshold for `A` in
/// the shifted representation (the standard `0x8000` left-shifted by 8).
const AREG_MIN: i32 = 1 << 23;

/// Packed MQ state-transition table (47 states). Each `u32` is
/// `Xs(1) << 31 | Sigma_mps(6) << 24 | Sigma_lps(6) << 16 | Qe(16)`, i.e. the next-state indices
/// for MPS/LPS, the MPS-exchange flag `Xs`, and the LPS probability estimate `Qe`. Verbatim from
/// the reference so the arithmetic matches bit-for-bit.
#[rustfmt::skip]
const STATIC_TABLE: [u32; 47] = [
    0x81015601, 0x02063401, 0x03091801, 0x040c0ac1, 0x051d0521, 0x26210221, 0x87065601, 0x080e5401,
    0x090e4801, 0x0a0e3801, 0x0b113001, 0x0c122401, 0x0d141c01, 0x1d151601, 0x8f0e5601, 0x100e5401,
    0x110f5101, 0x12104801, 0x13113801, 0x14123401, 0x15133001, 0x16132801, 0x17142401, 0x18152201,
    0x19161c01, 0x1a171801, 0x1b181601, 0x1c191401, 0x1d1a1201, 0x1e1b1101, 0x1f1c0ac1, 0x201d09c1,
    0x211e08a1, 0x221f0521, 0x23200441, 0x242102a1, 0x25220221, 0x26230141, 0x27240111, 0x28250085,
    0x29260049, 0x2a270025, 0x2b280015, 0x2c290009, 0x2d2a0005, 0x2d2b0001, 0x2e2e5601,
];

/// The 19 JPEG 2000 context labels share this decoder. `dynamic_table[0]` holds each context's
/// current state index (into [`STATIC_TABLE`]); `dynamic_table[1]` holds its MPS symbol (0/1).
pub struct MqDecoder<'a> {
    /// Interval register `A` (left-shifted by 8).
    a: i32,
    /// Count of fresh bits available in `C` before the next byte must be fetched.
    t: i32,
    /// Code register `C` (active byte at bits 8..24).
    c: i32,
    /// Temporary byte register `T` (the last byte fetched, for 0xFF bit-stuffing).
    big_t: i32,
    /// Read cursor into `byte_buffer` (absolute).
    l: Ui32,
    /// Codeword-segment boundary (`buf_pos + segment_length`).
    lmax: Ui32,
    /// Per-context `[state_index, mps]` tables (19 contexts).
    dynamic_table: [[Ui16; 19]; 2],
    byte_buffer: &'a [Ui8],
}

impl<'a> MqDecoder<'a> {
    /// Bind the decoder to a code-block's compressed byte buffer (not yet positioned; call
    /// [`Self::init`]).
    pub fn new(buf: &'a [Ui8]) -> Self {
        Self {
            a: 0,
            t: 0,
            c: 0,
            big_t: 0,
            l: 0,
            lmax: 0,
            dynamic_table: [[0; 19]; 2],
            byte_buffer: buf,
        }
    }

    /// Begin a codeword segment at `buf_pos` of length `segment_length`. `is_bypass` starts a RAW
    /// (arithmetic-bypass) segment, which only primes the byte register; an MQ segment primes the
    /// `A`/`C` registers per T.800 (INITDEC).
    pub fn init(&mut self, buf_pos: Ui32, segment_length: Ui32, is_bypass: bool) {
        self.lmax = buf_pos.saturating_add(segment_length);
        self.l = buf_pos;
        self.big_t = 0;
        if is_bypass {
            self.t = 0;
        } else {
            self.a = AREG_MIN;
            self.c = 0;
            self.fill_lsbs();
            self.c <<= self.t;
            self.fill_lsbs();
            self.c <<= 7;
            self.t -= 7;
        }
    }

    /// Reset all 19 contexts to the standard initial states (T.800 Table D.7): context 0 (the
    /// zero-coding "uniform" start) → state 4, context 17 (RUN) → state 3, context 18 (UNI) →
    /// state 46; all MPS = 0.
    pub fn init_states_for_all_contexts(&mut self) {
        self.dynamic_table = [[0; 19]; 2];
        self.dynamic_table[0][0] = 4;
        self.dynamic_table[0][17] = 3;
        self.dynamic_table[0][18] = 46;
    }

    /// Fetch the next byte into the low bits of `C`, honouring 0xFF bit-stuffing and the
    /// end-of-segment "fill with 1s" rule.
    fn fill_lsbs(&mut self) {
        self.t = 8;
        let next = if self.l < self.lmax {
            self.byte_buffer
                .get(self.l as usize)
                .copied()
                .unwrap_or(0xFF)
        } else {
            0xFF
        };
        if self.l == self.lmax || (self.big_t == 0xFF && next > 0x8F) {
            // Codeword exhausted: feed 1-bits from here on.
            self.c += 0xFF;
        } else {
            if self.big_t == 0xFF {
                self.t = 7;
            }
            self.big_t = next as i32;
            self.l += 1;
            self.c += self.big_t << (8 - self.t);
        }
    }

    fn renormalize(&mut self) {
        let mut n = (self.a as u32).leading_zeros() as i32 - 8;
        if self.t == 0 {
            self.fill_lsbs();
        }
        if n <= self.t {
            self.a <<= n;
            self.c <<= n;
            self.t -= n;
        } else {
            self.a <<= self.t;
            self.c <<= self.t;
            n -= self.t;
            self.t = 0;
            loop {
                self.fill_lsbs();
                let shift = if n <= self.t { n } else { self.t };
                self.a <<= shift;
                self.c <<= shift;
                self.t -= shift;
                n -= shift;
                if n <= 0 {
                    break;
                }
            }
        }
    }

    /// Decode one context-coded binary symbol for context `label` (0..18), updating that context's
    /// adaptive state. Faithful port of the reference MQ decode procedure (JPEG 2000 book pp.646).
    pub fn decode(&mut self, label: Ui8) -> Ui8 {
        let li = label as usize;
        let mut sigma_k = self.dynamic_table[0][li];
        let val = STATIC_TABLE[sigma_k as usize];
        let sigma_mps = ((val >> 24) & 0x3F) as Ui16;
        let sigma_lps = ((val >> 16) & 0x3F) as Ui16;
        let xs = (val >> 31) as Ui16;
        // p_bar (from the static table) << 8.
        let p_shifted = ((val & 0xFFFF) << 8) as i32;
        let mut sk = self.dynamic_table[1][li];

        let mut x = sk as i32; // assume MPS (most likely)
        self.a -= p_shifted;
        if self.c >= p_shifted {
            // Upper sub-interval selected.
            self.c -= p_shifted;
            if self.a < AREG_MIN {
                let is_lps = i32::from(self.a < p_shifted);
                x = (sk as i32) ^ is_lps;
                sk ^= xs & (-is_lps) as Ui16;
                sigma_k = if is_lps != 0 { sigma_lps } else { sigma_mps };
                self.renormalize();
            }
        } else {
            // Lower sub-interval selected; renormalisation inevitable.
            let is_lps = i32::from(self.a >= p_shifted);
            x = (sk as i32) ^ is_lps;
            sk ^= xs & (-is_lps) as Ui16;
            sigma_k = if is_lps != 0 { sigma_lps } else { sigma_mps };
            self.a = p_shifted;
            self.renormalize();
        }
        self.dynamic_table[0][li] = sigma_k;
        self.dynamic_table[1][li] = sk;
        x as Ui8
    }

    /// Read one RAW (arithmetic-bypass) bit, MSB-first, with 0xFF bit-stuffing.
    pub fn get_raw_symbol(&mut self) -> Ui8 {
        if self.t == 0 {
            self.t = 8;
            if self.l == self.lmax {
                self.big_t = 0xFF;
            } else {
                if self.big_t == 0xFF {
                    self.t = 7;
                }
                self.big_t = self
                    .byte_buffer
                    .get(self.l as usize)
                    .copied()
                    .unwrap_or(0xFF) as i32;
                self.l += 1;
            }
        }
        self.t -= 1;
        ((self.big_t >> self.t) & 1) as Ui8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The packed state table must hold all 47 MQ states with sane Qe values (non-zero, <= 0x5601).
    #[test]
    fn static_table_shape() {
        assert_eq!(STATIC_TABLE.len(), 47);
        for &v in &STATIC_TABLE {
            let qe = v & 0xFFFF;
            assert!(qe != 0 && qe <= 0x5601, "Qe {qe:#x} out of range");
            assert!((v >> 24) & 0x3F < 47, "Sigma_mps out of range");
            assert!((v >> 16) & 0x3F < 47, "Sigma_lps out of range");
        }
    }

    /// Structural / determinism check: a context-coded then RAW read over a fixed buffer must not
    /// panic and must be reproducible. (Bit-exactness is covered end-to-end by the .pgx conformance
    /// comparison once the EBCOT passes drive this decoder.)
    #[test]
    fn decode_is_deterministic_and_panic_free() {
        let buf: [u8; 8] = [0xA3, 0x5C, 0x00, 0xFF, 0x91, 0x7E, 0x42, 0x18];
        let run = || -> (Vec<u8>, Vec<u8>) {
            let mut mq = MqDecoder::new(&buf);
            mq.init(0, buf.len() as Ui32, false);
            mq.init_states_for_all_contexts();
            let ctx: Vec<u8> = (0..40).map(|i| mq.decode((i % 19) as Ui8)).collect();
            let raw: Vec<u8> = (0..16).map(|_| mq.get_raw_symbol()).collect();
            (ctx, raw)
        };
        let a = run();
        let b = run();
        assert_eq!(a, b, "MQ decode must be deterministic for identical input");
        assert!(a.0.iter().all(|&s| s <= 1) && a.1.iter().all(|&s| s <= 1));
    }
}
