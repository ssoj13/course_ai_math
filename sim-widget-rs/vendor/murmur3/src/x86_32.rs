//! MurmurHash3 **x86_32** - the 32-bit variant used by Cryptomatte
//! (`MurmurHash3_x86_32` / mmh3 `hash`).
//!
//! Hashes raw bytes (Cryptomatte: UTF-8 name bytes, seed `0`). Returns an
//! unsigned 32-bit digest. Float ID conversion (exponent clamp) belongs in
//! Cryptomatte code, not here.

/// Compute MurmurHash3 x86_32.
///
/// Matches Austin Appleby's `MurmurHash3_x86_32` and Psyop Cryptomatte /
/// `mmh3.hash` (unsigned). Seed `0` is the Cryptomatte default.
#[inline]
pub fn hash(data: &[u8], seed: u32) -> u32 {
    const C1: u32 = 0xcc9e2d51;
    const C2: u32 = 0x1b873593;

    let len = data.len();
    let nblocks = len / 4;
    let mut h1 = seed;

    // Body: 4-byte blocks, little-endian.
    for i in 0..nblocks {
        let offset = i * 4;
        let mut k1 = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());

        k1 = k1.wrapping_mul(C1);
        k1 = k1.rotate_left(15);
        k1 = k1.wrapping_mul(C2);

        h1 ^= k1;
        h1 = h1.rotate_left(13);
        h1 = h1.wrapping_mul(5).wrapping_add(0xe6546b64);
    }

    // Tail
    let tail = &data[nblocks * 4..];
    let mut k1: u32 = 0;
    match tail.len() {
        3 => {
            k1 ^= (tail[2] as u32) << 16;
            k1 ^= (tail[1] as u32) << 8;
            k1 ^= tail[0] as u32;
            k1 = k1.wrapping_mul(C1);
            k1 = k1.rotate_left(15);
            k1 = k1.wrapping_mul(C2);
            h1 ^= k1;
        }
        2 => {
            k1 ^= (tail[1] as u32) << 8;
            k1 ^= tail[0] as u32;
            k1 = k1.wrapping_mul(C1);
            k1 = k1.rotate_left(15);
            k1 = k1.wrapping_mul(C2);
            h1 ^= k1;
        }
        1 => {
            k1 ^= tail[0] as u32;
            k1 = k1.wrapping_mul(C1);
            k1 = k1.rotate_left(15);
            k1 = k1.wrapping_mul(C2);
            h1 ^= k1;
        }
        0 => {}
        _ => unreachable!(),
    }

    fmix32(h1 ^ (len as u32))
}

/// Same hash as [`hash`], returned as 4 little-endian bytes.
#[inline]
pub fn hash_bytes(data: &[u8], seed: u32) -> [u8; 4] {
    hash(data, seed).to_le_bytes()
}

#[inline]
fn fmix32(mut h: u32) -> u32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x85ebca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2ae35);
    h ^= h >> 16;
    h
}

/// Cryptomatte float-ID conversion: bit-copy with exponent clamped out of
/// {0, 255}. Spec / Psyop `mm3hash_float`. Kept here as a test helper only.
#[cfg(test)]
fn cryptomatte_id_float(name: &[u8]) -> f32 {
    let mut h = hash(name, 0);
    let exp = (h >> 23) & 255;
    if exp == 0 || exp == 255 {
        h ^= 1 << 23;
    }
    f32::from_bits(h)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unsigned digests from Psyop `pymmh3` / mmh3 (seed 0).
    #[test]
    fn cryptomatte_u32_goldens() {
        let cases: &[(&[u8], u32)] = &[
            (b"", 0x0000_0000),
            (b"a", 0x3c25_69b2),
            (b"abc", 0xb3dd_93fa),
            (b"hello", 0x248b_fa47),
            (b"cube", 0xd968_2f08),
            (b"sphere", 0x591e_9a8d),
            (b"plane", 0x2e21_36b8),
            // UTF-8 "\u0440\u0430\u0432\u043d\u0438\u043d\u0430" (Bulgarian "plane")
            (
                b"\xd1\x80\xd0\xb0\xd0\xb2\xd0\xbd\xd0\xb8\xd0\xbd\xd0\xb0",
                0x9623_511e,
            ),
            // UTF-8 "m\u00e4dchen"
            (b"m\xc3\xa4dchen", 0x6a4e_5613),
        ];
        for &(name, expected) in cases {
            assert_eq!(hash(name, 0), expected, "hash({name:?})");
        }
    }

    #[test]
    fn hash_bytes_le() {
        assert_eq!(hash_bytes(b"hello", 0), 0x248b_fa47u32.to_le_bytes());
    }

    /// Psyop `CryptoHashing.mm3hash_float_values` (after single-precision).
    #[test]
    fn cryptomatte_float_goldens() {
        let cases: &[(&[u8], f32)] = &[
            (b"hello", 6.0705627102400005616e-17),
            (b"cube", -4.08461912519e15),
            (b"sphere", 2.79018604383e15),
            (b"plane", 3.66557617593e-11),
            (
                b"\xd1\x80\xd0\xb0\xd0\xb2\xd0\xbd\xd0\xb8\xd0\xbd\xd0\xb0",
                -1.3192631212399999468e-25,
            ),
            (b"m\xc3\xa4dchen", 6.2361298211599995797e25),
        ];
        for &(name, expected) in cases {
            let got = cryptomatte_id_float(name);
            // Match via bits after f32 narrowing of the Python literal.
            assert_eq!(got.to_bits(), expected.to_bits(), "float id for {name:?}");
        }
    }
}
