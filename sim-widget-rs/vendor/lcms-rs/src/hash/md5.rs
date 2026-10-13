//! RFC 1321 MD5 message digest — bit-exact port of `src/cmsmd5.c`.
//!
//! lcms2 uses MD5 solely to compute an ICC profile's `ProfileID` (per section
//! 7.2.18 of the ICC spec 4.4). This module ports the algorithm verbatim so the
//! resulting digest matches both the C reference and every standard MD5
//! implementation.
//!
//! ## Endianness
//! The C code accumulates raw bytes in an `in[64]` buffer and then reinterprets
//! that buffer as sixteen `cmsUInt32Number` words when calling
//! `cmsMD5_Transform`. On a little-endian host this reads each word as
//! `in[k] | in[k+1]<<8 | in[k+2]<<16 | in[k+3]<<24`, i.e. MD5's canonical
//! little-endian word packing, and the `byteReverse` macro is a no-op
//! (`#else #define byteReverse(buf, len)`). On big-endian hosts `byteReverse`
//! swaps each word so the *values* fed to the transform are identical. Because
//! we control the byte layout directly in Rust, we always pack words with
//! [`u32::from_le_bytes`] (and unpack the digest with [`u32::to_le_bytes`]),
//! reproducing the little-endian behaviour on every target — no `unsafe`
//! pointer casts and no host-endianness branch required.

/// MD5 auxiliary function `F1` — C macro `F1(x, y, z) = (z ^ (x & (y ^ z)))`.
#[inline]
fn f1(x: u32, y: u32, z: u32) -> u32 {
    z ^ (x & (y ^ z))
}

/// MD5 auxiliary function `F2` — C macro `F2(x, y, z) = F1(z, x, y)`.
#[inline]
fn f2(x: u32, y: u32, z: u32) -> u32 {
    f1(z, x, y)
}

/// MD5 auxiliary function `F3` — C macro `F3(x, y, z) = (x ^ y ^ z)`.
#[inline]
fn f3(x: u32, y: u32, z: u32) -> u32 {
    x ^ y ^ z
}

/// MD5 auxiliary function `F4` — C macro `F4(x, y, z) = (y ^ (x | ~z))`.
#[inline]
fn f4(x: u32, y: u32, z: u32) -> u32 {
    y ^ (x | !z)
}

/// MD5 state — port of the C `_cmsMD5` struct (minus `cmsContext`, unused here).
///
/// - `buf`  — the running 128-bit digest (`buf[4]`).
/// - `bits` — 64-bit message length in bits, low word first (`bits[2]`).
/// - `input` — partial-block byte buffer (`in[64]`).
pub struct Md5 {
    buf: [u32; 4],
    bits: [u32; 2],
    input: [u8; 64],
}

impl Md5 {
    /// Allocate and initialise MD5 state — port of `cmsMD5alloc`.
    ///
    /// The four `buf` words are RFC 1321's fixed initialisation constants.
    pub fn new() -> Self {
        Self {
            buf: [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476],
            bits: [0, 0],
            input: [0; 64],
        }
    }

    /// Pack a 64-byte block into sixteen little-endian words.
    ///
    /// This is the pure-Rust equivalent of the C `(cmsUInt32Number *) ctx->in`
    /// reinterpret-cast combined with the (LE-only) `byteReverse` no-op.
    fn words(input: &[u8; 64]) -> [u32; 16] {
        let mut w = [0u32; 16];
        for (word, chunk) in w.iter_mut().zip(input.chunks_exact(4)) {
            *word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        w
    }

    /// The 64-step MD5 compression function — port of `cmsMD5_Transform`.
    ///
    /// All arithmetic wraps modulo 2^32 (`wrapping_add`), and the C rotate
    /// `w = w<<s | w>>(32-s)` maps to [`u32::rotate_left`]. Every constant,
    /// message-word index and shift amount is transcribed verbatim from the C.
    fn transform(buf: &mut [u32; 4], block: &[u32; 16]) {
        // C `STEP(f, w, x, y, z, data, s)`:
        //   ( w += f(x, y, z) + data,  w = w<<s | w>>(32-s),  w += x )
        macro_rules! step {
            ($f:ident, $w:ident, $x:ident, $y:ident, $z:ident, $data:expr, $s:expr) => {{
                $w = $w.wrapping_add($f($x, $y, $z)).wrapping_add($data);
                $w = $w.rotate_left($s);
                $w = $w.wrapping_add($x);
            }};
        }

        let [mut a, mut b, mut c, mut d] = *buf;
        let in_ = block;

        step!(f1, a, b, c, d, in_[0].wrapping_add(0xd76a_a478), 7);
        step!(f1, d, a, b, c, in_[1].wrapping_add(0xe8c7_b756), 12);
        step!(f1, c, d, a, b, in_[2].wrapping_add(0x2420_70db), 17);
        step!(f1, b, c, d, a, in_[3].wrapping_add(0xc1bd_ceee), 22);
        step!(f1, a, b, c, d, in_[4].wrapping_add(0xf57c_0faf), 7);
        step!(f1, d, a, b, c, in_[5].wrapping_add(0x4787_c62a), 12);
        step!(f1, c, d, a, b, in_[6].wrapping_add(0xa830_4613), 17);
        step!(f1, b, c, d, a, in_[7].wrapping_add(0xfd46_9501), 22);
        step!(f1, a, b, c, d, in_[8].wrapping_add(0x6980_98d8), 7);
        step!(f1, d, a, b, c, in_[9].wrapping_add(0x8b44_f7af), 12);
        step!(f1, c, d, a, b, in_[10].wrapping_add(0xffff_5bb1), 17);
        step!(f1, b, c, d, a, in_[11].wrapping_add(0x895c_d7be), 22);
        step!(f1, a, b, c, d, in_[12].wrapping_add(0x6b90_1122), 7);
        step!(f1, d, a, b, c, in_[13].wrapping_add(0xfd98_7193), 12);
        step!(f1, c, d, a, b, in_[14].wrapping_add(0xa679_438e), 17);
        step!(f1, b, c, d, a, in_[15].wrapping_add(0x49b4_0821), 22);

        step!(f2, a, b, c, d, in_[1].wrapping_add(0xf61e_2562), 5);
        step!(f2, d, a, b, c, in_[6].wrapping_add(0xc040_b340), 9);
        step!(f2, c, d, a, b, in_[11].wrapping_add(0x265e_5a51), 14);
        step!(f2, b, c, d, a, in_[0].wrapping_add(0xe9b6_c7aa), 20);
        step!(f2, a, b, c, d, in_[5].wrapping_add(0xd62f_105d), 5);
        step!(f2, d, a, b, c, in_[10].wrapping_add(0x0244_1453), 9);
        step!(f2, c, d, a, b, in_[15].wrapping_add(0xd8a1_e681), 14);
        step!(f2, b, c, d, a, in_[4].wrapping_add(0xe7d3_fbc8), 20);
        step!(f2, a, b, c, d, in_[9].wrapping_add(0x21e1_cde6), 5);
        step!(f2, d, a, b, c, in_[14].wrapping_add(0xc337_07d6), 9);
        step!(f2, c, d, a, b, in_[3].wrapping_add(0xf4d5_0d87), 14);
        step!(f2, b, c, d, a, in_[8].wrapping_add(0x455a_14ed), 20);
        step!(f2, a, b, c, d, in_[13].wrapping_add(0xa9e3_e905), 5);
        step!(f2, d, a, b, c, in_[2].wrapping_add(0xfcef_a3f8), 9);
        step!(f2, c, d, a, b, in_[7].wrapping_add(0x676f_02d9), 14);
        step!(f2, b, c, d, a, in_[12].wrapping_add(0x8d2a_4c8a), 20);

        step!(f3, a, b, c, d, in_[5].wrapping_add(0xfffa_3942), 4);
        step!(f3, d, a, b, c, in_[8].wrapping_add(0x8771_f681), 11);
        step!(f3, c, d, a, b, in_[11].wrapping_add(0x6d9d_6122), 16);
        step!(f3, b, c, d, a, in_[14].wrapping_add(0xfde5_380c), 23);
        step!(f3, a, b, c, d, in_[1].wrapping_add(0xa4be_ea44), 4);
        step!(f3, d, a, b, c, in_[4].wrapping_add(0x4bde_cfa9), 11);
        step!(f3, c, d, a, b, in_[7].wrapping_add(0xf6bb_4b60), 16);
        step!(f3, b, c, d, a, in_[10].wrapping_add(0xbebf_bc70), 23);
        step!(f3, a, b, c, d, in_[13].wrapping_add(0x289b_7ec6), 4);
        step!(f3, d, a, b, c, in_[0].wrapping_add(0xeaa1_27fa), 11);
        step!(f3, c, d, a, b, in_[3].wrapping_add(0xd4ef_3085), 16);
        step!(f3, b, c, d, a, in_[6].wrapping_add(0x0488_1d05), 23);
        step!(f3, a, b, c, d, in_[9].wrapping_add(0xd9d4_d039), 4);
        step!(f3, d, a, b, c, in_[12].wrapping_add(0xe6db_99e5), 11);
        step!(f3, c, d, a, b, in_[15].wrapping_add(0x1fa2_7cf8), 16);
        step!(f3, b, c, d, a, in_[2].wrapping_add(0xc4ac_5665), 23);

        step!(f4, a, b, c, d, in_[0].wrapping_add(0xf429_2244), 6);
        step!(f4, d, a, b, c, in_[7].wrapping_add(0x432a_ff97), 10);
        step!(f4, c, d, a, b, in_[14].wrapping_add(0xab94_23a7), 15);
        step!(f4, b, c, d, a, in_[5].wrapping_add(0xfc93_a039), 21);
        step!(f4, a, b, c, d, in_[12].wrapping_add(0x655b_59c3), 6);
        step!(f4, d, a, b, c, in_[3].wrapping_add(0x8f0c_cc92), 10);
        step!(f4, c, d, a, b, in_[10].wrapping_add(0xffef_f47d), 15);
        step!(f4, b, c, d, a, in_[1].wrapping_add(0x8584_5dd1), 21);
        step!(f4, a, b, c, d, in_[8].wrapping_add(0x6fa8_7e4f), 6);
        step!(f4, d, a, b, c, in_[15].wrapping_add(0xfe2c_e6e0), 10);
        step!(f4, c, d, a, b, in_[6].wrapping_add(0xa301_4314), 15);
        step!(f4, b, c, d, a, in_[13].wrapping_add(0x4e08_11a1), 21);
        step!(f4, a, b, c, d, in_[4].wrapping_add(0xf753_7e82), 6);
        step!(f4, d, a, b, c, in_[11].wrapping_add(0xbd3a_f235), 10);
        step!(f4, c, d, a, b, in_[2].wrapping_add(0x2ad7_d2bb), 15);
        step!(f4, b, c, d, a, in_[9].wrapping_add(0xeb86_d391), 21);

        buf[0] = buf[0].wrapping_add(a);
        buf[1] = buf[1].wrapping_add(b);
        buf[2] = buf[2].wrapping_add(c);
        buf[3] = buf[3].wrapping_add(d);
    }

    /// Feed `data` into the digest — port of `cmsMD5add`.
    ///
    /// The C API takes a `cmsUInt32Number len`; we accept a byte slice and
    /// truncate its length to `u32` to reproduce the exact bit-count arithmetic
    /// (`len << 3`, `len >> 29`) for inputs below 4 GiB — the only sizes lcms
    /// ever hashes (a serialised ICC profile).
    pub fn update(&mut self, data: &[u8]) {
        let mut buf = data;
        let mut len = data.len() as u32;

        // Update the 64-bit bit count, carrying into the high word.
        let t = self.bits[0];
        self.bits[0] = t.wrapping_add(len << 3);
        if self.bits[0] < t {
            self.bits[1] = self.bits[1].wrapping_add(1);
        }
        self.bits[1] = self.bits[1].wrapping_add(len >> 29);

        // Bytes already buffered in `input` (0..=63).
        let t = ((t >> 3) & 0x3f) as usize;

        if t != 0 {
            // Complete the partial block if there is enough new data.
            let space = 64 - t;
            if (len as usize) < space {
                self.input[t..t + len as usize].copy_from_slice(&buf[..len as usize]);
                return;
            }
            self.input[t..64].copy_from_slice(&buf[..space]);
            Self::transform(&mut self.buf, &Self::words(&self.input));
            buf = &buf[space..];
            len -= space as u32;
        }

        // Process full 64-byte blocks straight from the input.
        while len >= 64 {
            self.input.copy_from_slice(&buf[..64]);
            Self::transform(&mut self.buf, &Self::words(&self.input));
            buf = &buf[64..];
            len -= 64;
        }

        // Stash the remaining tail.
        self.input[..len as usize].copy_from_slice(&buf[..len as usize]);
    }

    /// Finalise and return the 16-byte digest — port of `cmsMD5finish`.
    ///
    /// The C version writes the result into a `cmsProfileID*`; here we return
    /// the raw digest bytes and let the caller place them. Consumes `self`
    /// because the state is spent (the C code frees the context).
    pub fn finish(mut self) -> [u8; 16] {
        // Number of bytes currently buffered (0..=63).
        let count = ((self.bits[0] >> 3) & 0x3f) as usize;

        // Append the mandatory 0x80 padding byte.
        self.input[count] = 0x80;
        let p = count + 1;

        // Bytes between the 0x80 and the end of the 64-byte block.
        let remaining = 64 - 1 - count;
        if remaining < 8 {
            // Not enough room for the 8-byte length: pad, transform, then start
            // a fresh all-zero block for the length words.
            for byte in &mut self.input[p..64] {
                *byte = 0;
            }
            Self::transform(&mut self.buf, &Self::words(&self.input));
            for byte in &mut self.input[..56] {
                *byte = 0;
            }
        } else {
            // Zero up to (but not into) the final two length words.
            for byte in &mut self.input[p..p + (remaining - 8)] {
                *byte = 0;
            }
        }

        // Words 0..14 come from the buffer; words 14 and 15 hold the bit count.
        // (C: `byteReverse(ctx->in, 14)` is a LE no-op, then the two direct
        // `((cmsUInt32Number*) ctx->in)[14/15] = ctx->bits[...]` assignments.)
        let mut block = Self::words(&self.input);
        block[14] = self.bits[0];
        block[15] = self.bits[1];
        Self::transform(&mut self.buf, &block);

        // Emit the digest words little-endian (C: LE-no-op `byteReverse` then a
        // 16-byte `memmove` out of `ctx->buf`).
        let mut out = [0u8; 16];
        for (chunk, word) in out.chunks_exact_mut(4).zip(self.buf.iter()) {
            chunk.copy_from_slice(&word.to_le_bytes());
        }
        out
    }
}

impl Default for Md5 {
    fn default() -> Self {
        Self::new()
    }
}

/// One-shot convenience: compute the MD5 digest of `data`.
///
/// Equivalent to `cmsMD5alloc` + `cmsMD5add` + `cmsMD5finish` over a single
/// buffer. This is the reusable primitive the ICC `ProfileID` computation will
/// call once profile serialisation (Save-To-Mem) is available.
pub fn md5(data: &[u8]) -> [u8; 16] {
    let mut ctx = Md5::new();
    ctx.update(data);
    ctx.finish()
}

#[cfg(test)]
mod tests {
    use super::md5;

    /// Parse a 32-char lowercase hex string into a `[u8; 16]` expected digest.
    fn hex16(s: &str) -> [u8; 16] {
        let bytes = s.as_bytes();
        assert_eq!(bytes.len(), 32, "expected 32 hex chars");
        let mut out = [0u8; 16];
        for (i, byte) in out.iter_mut().enumerate() {
            let hi = (bytes[i * 2] as char).to_digit(16).expect("hex") as u8;
            let lo = (bytes[i * 2 + 1] as char).to_digit(16).expect("hex") as u8;
            *byte = (hi << 4) | lo;
        }
        out
    }

    // Canonical RFC 1321 (appendix A.5) MD5 test-suite vectors.
    #[test]
    fn rfc1321_empty() {
        assert_eq!(md5(b""), hex16("d41d8cd98f00b204e9800998ecf8427e"));
    }

    #[test]
    fn rfc1321_abc() {
        assert_eq!(md5(b"abc"), hex16("900150983cd24fb0d6963f7d28e17f72"));
    }

    #[test]
    fn rfc1321_message_digest() {
        assert_eq!(
            md5(b"message digest"),
            hex16("f96b697d7cb7938d525a2f31aaf161d0")
        );
    }

    #[test]
    fn rfc1321_fox() {
        assert_eq!(
            md5(b"The quick brown fox jumps over the lazy dog"),
            hex16("9e107d9d372bb6826bd81d3542a419d6")
        );
    }

    /// Multi-`update` streaming must match a single-shot hash, and crossing the
    /// 64-byte block boundary exercises the partial-buffer path in `update`.
    #[test]
    fn streaming_matches_oneshot() {
        use super::Md5;
        let data: Vec<u8> = (0u8..200).collect();
        let oneshot = md5(&data);

        let mut ctx = Md5::new();
        ctx.update(&data[..1]);
        ctx.update(&data[1..70]);
        ctx.update(&data[70..128]);
        ctx.update(&data[128..]);
        assert_eq!(ctx.finish(), oneshot);
    }
}
