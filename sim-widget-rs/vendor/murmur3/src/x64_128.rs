//! MurmurHash3 **x64_128** - byte-compatible with Alembic's C++ `MurmurHash3_x64_128`.
//!
//! Original algorithm by Austin Appleby. Alembic-compatible on little-endian hosts;
//! on big-endian hosts pass element `pod_size` so per-element byte order matches
//! the reference.

/// Compute MurmurHash3 x64_128.
///
/// - `seed`: optional hash seed (`None` = seedless, like Alembic).
/// - `pod_size`: POD byte size for big-endian byte swapping (C++ `podSize`);
///   `None` = no swap.
///
/// Returns the 128-bit digest as `(h1, h2)`.
#[inline]
pub fn hash(data: &[u8], seed: Option<u32>, pod_size: Option<usize>) -> (u64, u64) {
    let pod_size = pod_size.unwrap_or(1);
    let len = data.len();
    let nblocks = len / 16;

    let seed = seed.unwrap_or(0) as u64;
    let mut h1: u64 = seed;
    let mut h2: u64 = seed;

    const C1: u64 = 0x87c37b91114253d5;
    const C2: u64 = 0x4cf5ad432745937f;

    // Body - process 16-byte blocks.
    for i in 0..nblocks {
        let offset = i * 16;

        let block1 = &data[offset..offset + 8];
        let block2 = &data[offset + 8..offset + 16];

        let mut k1 = if cfg!(target_endian = "big") {
            u64::from_ne_bytes(block1.try_into().unwrap())
        } else {
            u64::from_le_bytes(block1.try_into().unwrap())
        };
        let mut k2 = if cfg!(target_endian = "big") {
            u64::from_ne_bytes(block2.try_into().unwrap())
        } else {
            u64::from_le_bytes(block2.try_into().unwrap())
        };

        if cfg!(target_endian = "big") && pod_size > 1 {
            k1 = swap_pod_u64(k1, pod_size);
            k2 = swap_pod_u64(k2, pod_size);
        }

        // Mix k1
        k1 = k1.wrapping_mul(C1);
        k1 = k1.rotate_left(31);
        k1 = k1.wrapping_mul(C2);
        h1 ^= k1;

        h1 = h1.rotate_left(27);
        h1 = h1.wrapping_add(h2);
        h1 = h1.wrapping_mul(5).wrapping_add(0x52dce729);

        // Mix k2
        k2 = k2.wrapping_mul(C2);
        k2 = k2.rotate_left(33);
        k2 = k2.wrapping_mul(C1);
        h2 ^= k2;

        h2 = h2.rotate_left(31);
        h2 = h2.wrapping_add(h1);
        h2 = h2.wrapping_mul(5).wrapping_add(0x38495ab5);
    }

    // Tail - process remaining bytes.
    let raw_tail = &data[nblocks * 16..];
    let mut tail_buf = [0u8; 16];
    let tail_len = raw_tail.len();
    if cfg!(target_endian = "big") && pod_size > 1 {
        for j in 0..tail_len {
            tail_buf[j] = raw_tail[j ^ (pod_size - 1)];
        }
    } else {
        tail_buf[..tail_len].copy_from_slice(raw_tail);
    }
    let tail = &tail_buf[..tail_len];
    let mut k1: u64 = 0;
    let mut k2: u64 = 0;

    // Fall-through switch emulation.
    match tail.len() {
        15 => {
            k2 ^= (tail[14] as u64) << 48;
            k2 ^= (tail[13] as u64) << 40;
            k2 ^= (tail[12] as u64) << 32;
            k2 ^= (tail[11] as u64) << 24;
            k2 ^= (tail[10] as u64) << 16;
            k2 ^= (tail[9] as u64) << 8;
            k2 ^= tail[8] as u64;
            k2 = k2.wrapping_mul(C2);
            k2 = k2.rotate_left(33);
            k2 = k2.wrapping_mul(C1);
            h2 ^= k2;

            k1 ^= (tail[7] as u64) << 56;
            k1 ^= (tail[6] as u64) << 48;
            k1 ^= (tail[5] as u64) << 40;
            k1 ^= (tail[4] as u64) << 32;
            k1 ^= (tail[3] as u64) << 24;
            k1 ^= (tail[2] as u64) << 16;
            k1 ^= (tail[1] as u64) << 8;
            k1 ^= tail[0] as u64;
            k1 = k1.wrapping_mul(C1);
            k1 = k1.rotate_left(31);
            k1 = k1.wrapping_mul(C2);
            h1 ^= k1;
        }
        14 => {
            k2 ^= (tail[13] as u64) << 40;
            k2 ^= (tail[12] as u64) << 32;
            k2 ^= (tail[11] as u64) << 24;
            k2 ^= (tail[10] as u64) << 16;
            k2 ^= (tail[9] as u64) << 8;
            k2 ^= tail[8] as u64;
            k2 = k2.wrapping_mul(C2);
            k2 = k2.rotate_left(33);
            k2 = k2.wrapping_mul(C1);
            h2 ^= k2;

            k1 ^= (tail[7] as u64) << 56;
            k1 ^= (tail[6] as u64) << 48;
            k1 ^= (tail[5] as u64) << 40;
            k1 ^= (tail[4] as u64) << 32;
            k1 ^= (tail[3] as u64) << 24;
            k1 ^= (tail[2] as u64) << 16;
            k1 ^= (tail[1] as u64) << 8;
            k1 ^= tail[0] as u64;
            k1 = k1.wrapping_mul(C1);
            k1 = k1.rotate_left(31);
            k1 = k1.wrapping_mul(C2);
            h1 ^= k1;
        }
        13 => {
            k2 ^= (tail[12] as u64) << 32;
            k2 ^= (tail[11] as u64) << 24;
            k2 ^= (tail[10] as u64) << 16;
            k2 ^= (tail[9] as u64) << 8;
            k2 ^= tail[8] as u64;
            k2 = k2.wrapping_mul(C2);
            k2 = k2.rotate_left(33);
            k2 = k2.wrapping_mul(C1);
            h2 ^= k2;

            k1 ^= (tail[7] as u64) << 56;
            k1 ^= (tail[6] as u64) << 48;
            k1 ^= (tail[5] as u64) << 40;
            k1 ^= (tail[4] as u64) << 32;
            k1 ^= (tail[3] as u64) << 24;
            k1 ^= (tail[2] as u64) << 16;
            k1 ^= (tail[1] as u64) << 8;
            k1 ^= tail[0] as u64;
            k1 = k1.wrapping_mul(C1);
            k1 = k1.rotate_left(31);
            k1 = k1.wrapping_mul(C2);
            h1 ^= k1;
        }
        12 => {
            k2 ^= (tail[11] as u64) << 24;
            k2 ^= (tail[10] as u64) << 16;
            k2 ^= (tail[9] as u64) << 8;
            k2 ^= tail[8] as u64;
            k2 = k2.wrapping_mul(C2);
            k2 = k2.rotate_left(33);
            k2 = k2.wrapping_mul(C1);
            h2 ^= k2;

            k1 ^= (tail[7] as u64) << 56;
            k1 ^= (tail[6] as u64) << 48;
            k1 ^= (tail[5] as u64) << 40;
            k1 ^= (tail[4] as u64) << 32;
            k1 ^= (tail[3] as u64) << 24;
            k1 ^= (tail[2] as u64) << 16;
            k1 ^= (tail[1] as u64) << 8;
            k1 ^= tail[0] as u64;
            k1 = k1.wrapping_mul(C1);
            k1 = k1.rotate_left(31);
            k1 = k1.wrapping_mul(C2);
            h1 ^= k1;
        }
        11 => {
            k2 ^= (tail[10] as u64) << 16;
            k2 ^= (tail[9] as u64) << 8;
            k2 ^= tail[8] as u64;
            k2 = k2.wrapping_mul(C2);
            k2 = k2.rotate_left(33);
            k2 = k2.wrapping_mul(C1);
            h2 ^= k2;

            k1 ^= (tail[7] as u64) << 56;
            k1 ^= (tail[6] as u64) << 48;
            k1 ^= (tail[5] as u64) << 40;
            k1 ^= (tail[4] as u64) << 32;
            k1 ^= (tail[3] as u64) << 24;
            k1 ^= (tail[2] as u64) << 16;
            k1 ^= (tail[1] as u64) << 8;
            k1 ^= tail[0] as u64;
            k1 = k1.wrapping_mul(C1);
            k1 = k1.rotate_left(31);
            k1 = k1.wrapping_mul(C2);
            h1 ^= k1;
        }
        10 => {
            k2 ^= (tail[9] as u64) << 8;
            k2 ^= tail[8] as u64;
            k2 = k2.wrapping_mul(C2);
            k2 = k2.rotate_left(33);
            k2 = k2.wrapping_mul(C1);
            h2 ^= k2;

            k1 ^= (tail[7] as u64) << 56;
            k1 ^= (tail[6] as u64) << 48;
            k1 ^= (tail[5] as u64) << 40;
            k1 ^= (tail[4] as u64) << 32;
            k1 ^= (tail[3] as u64) << 24;
            k1 ^= (tail[2] as u64) << 16;
            k1 ^= (tail[1] as u64) << 8;
            k1 ^= tail[0] as u64;
            k1 = k1.wrapping_mul(C1);
            k1 = k1.rotate_left(31);
            k1 = k1.wrapping_mul(C2);
            h1 ^= k1;
        }
        9 => {
            k2 ^= tail[8] as u64;
            k2 = k2.wrapping_mul(C2);
            k2 = k2.rotate_left(33);
            k2 = k2.wrapping_mul(C1);
            h2 ^= k2;

            k1 ^= (tail[7] as u64) << 56;
            k1 ^= (tail[6] as u64) << 48;
            k1 ^= (tail[5] as u64) << 40;
            k1 ^= (tail[4] as u64) << 32;
            k1 ^= (tail[3] as u64) << 24;
            k1 ^= (tail[2] as u64) << 16;
            k1 ^= (tail[1] as u64) << 8;
            k1 ^= tail[0] as u64;
            k1 = k1.wrapping_mul(C1);
            k1 = k1.rotate_left(31);
            k1 = k1.wrapping_mul(C2);
            h1 ^= k1;
        }
        8 => {
            k1 ^= (tail[7] as u64) << 56;
            k1 ^= (tail[6] as u64) << 48;
            k1 ^= (tail[5] as u64) << 40;
            k1 ^= (tail[4] as u64) << 32;
            k1 ^= (tail[3] as u64) << 24;
            k1 ^= (tail[2] as u64) << 16;
            k1 ^= (tail[1] as u64) << 8;
            k1 ^= tail[0] as u64;
            k1 = k1.wrapping_mul(C1);
            k1 = k1.rotate_left(31);
            k1 = k1.wrapping_mul(C2);
            h1 ^= k1;
        }
        7 => {
            k1 ^= (tail[6] as u64) << 48;
            k1 ^= (tail[5] as u64) << 40;
            k1 ^= (tail[4] as u64) << 32;
            k1 ^= (tail[3] as u64) << 24;
            k1 ^= (tail[2] as u64) << 16;
            k1 ^= (tail[1] as u64) << 8;
            k1 ^= tail[0] as u64;
            k1 = k1.wrapping_mul(C1);
            k1 = k1.rotate_left(31);
            k1 = k1.wrapping_mul(C2);
            h1 ^= k1;
        }
        6 => {
            k1 ^= (tail[5] as u64) << 40;
            k1 ^= (tail[4] as u64) << 32;
            k1 ^= (tail[3] as u64) << 24;
            k1 ^= (tail[2] as u64) << 16;
            k1 ^= (tail[1] as u64) << 8;
            k1 ^= tail[0] as u64;
            k1 = k1.wrapping_mul(C1);
            k1 = k1.rotate_left(31);
            k1 = k1.wrapping_mul(C2);
            h1 ^= k1;
        }
        5 => {
            k1 ^= (tail[4] as u64) << 32;
            k1 ^= (tail[3] as u64) << 24;
            k1 ^= (tail[2] as u64) << 16;
            k1 ^= (tail[1] as u64) << 8;
            k1 ^= tail[0] as u64;
            k1 = k1.wrapping_mul(C1);
            k1 = k1.rotate_left(31);
            k1 = k1.wrapping_mul(C2);
            h1 ^= k1;
        }
        4 => {
            k1 ^= (tail[3] as u64) << 24;
            k1 ^= (tail[2] as u64) << 16;
            k1 ^= (tail[1] as u64) << 8;
            k1 ^= tail[0] as u64;
            k1 = k1.wrapping_mul(C1);
            k1 = k1.rotate_left(31);
            k1 = k1.wrapping_mul(C2);
            h1 ^= k1;
        }
        3 => {
            k1 ^= (tail[2] as u64) << 16;
            k1 ^= (tail[1] as u64) << 8;
            k1 ^= tail[0] as u64;
            k1 = k1.wrapping_mul(C1);
            k1 = k1.rotate_left(31);
            k1 = k1.wrapping_mul(C2);
            h1 ^= k1;
        }
        2 => {
            k1 ^= (tail[1] as u64) << 8;
            k1 ^= tail[0] as u64;
            k1 = k1.wrapping_mul(C1);
            k1 = k1.rotate_left(31);
            k1 = k1.wrapping_mul(C2);
            h1 ^= k1;
        }
        1 => {
            k1 ^= tail[0] as u64;
            k1 = k1.wrapping_mul(C1);
            k1 = k1.rotate_left(31);
            k1 = k1.wrapping_mul(C2);
            h1 ^= k1;
        }
        0 => {}
        _ => unreachable!(),
    }

    // Finalization
    h1 ^= len as u64;
    h2 ^= len as u64;

    h1 = h1.wrapping_add(h2);
    h2 = h2.wrapping_add(h1);

    h1 = fmix64(h1);
    h2 = fmix64(h2);

    h1 = h1.wrapping_add(h2);
    h2 = h2.wrapping_add(h1);

    (h1, h2)
}

/// Final mix function for 64-bit values.
#[inline]
fn fmix64(mut h: u64) -> u64 {
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51afd7ed558ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ceb9fe1a85ec53);
    h ^= h >> 33;
    h
}

/// Swap bytes within a u64 based on POD size (big-endian handling).
/// Mirrors the C++ byte-swap logic in Alembic's Murmur3 implementation.
#[inline]
fn swap_pod_u64(mut value: u64, pod_size: usize) -> u64 {
    match pod_size {
        8 => {
            value = (value >> 56)
                | ((value << 40) & 0x00FF_0000_0000_0000)
                | ((value << 24) & 0x0000_FF00_0000_0000)
                | ((value << 8) & 0x0000_00FF_0000_0000)
                | ((value >> 8) & 0x0000_0000_FF00_0000)
                | ((value >> 24) & 0x0000_0000_00FF_0000)
                | ((value >> 40) & 0x0000_0000_0000_FF00)
                | (value << 56);
        }
        4 => {
            value = ((value << 24) & 0xFF00_0000_0000_0000)
                | ((value << 8) & 0x00FF_0000_0000_0000)
                | ((value >> 8) & 0x0000_FF00_0000_0000)
                | ((value >> 24) & 0x0000_00FF_0000_0000)
                | ((value << 24) & 0x0000_0000_FF00_0000)
                | ((value << 8) & 0x0000_0000_00FF_0000)
                | ((value >> 8) & 0x0000_0000_0000_FF00)
                | ((value >> 24) & 0x0000_0000_0000_00FF);
        }
        2 => {
            value = ((value << 8) & 0xFF00_0000_0000_0000)
                | ((value >> 8) & 0x00FF_0000_0000_0000)
                | ((value << 8) & 0x0000_FF00_0000_0000)
                | ((value >> 8) & 0x0000_00FF_0000_0000)
                | ((value << 8) & 0x0000_0000_FF00_0000)
                | ((value >> 8) & 0x0000_0000_00FF_0000)
                | ((value << 8) & 0x0000_0000_0000_FF00)
                | ((value >> 8) & 0x0000_0000_0000_00FF);
        }
        _ => {}
    }
    value
}

/// Same hash as [`hash`], returned as Alembic's 16-byte little-endian digest.
#[inline]
pub fn hash_bytes(data: &[u8], seed: Option<u32>, pod_size: Option<usize>) -> [u8; 16] {
    let (h1, h2) = hash(data, seed, pod_size);
    let mut result = [0u8; 16];
    result[0..8].copy_from_slice(&h1.to_le_bytes());
    result[8..16].copy_from_slice(&h2.to_le_bytes());
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty() {
        let (h1, h2) = hash(&[], None, None);
        // Empty input yields deterministic zero hash for seed 0.
        assert_eq!(h1, 0);
        assert_eq!(h2, 0);
    }

    #[test]
    fn short() {
        let (h1, _h2) = hash(b"hello", None, None);
        // Just verify it produces something
        assert_ne!(h1, 0);
    }

    #[test]
    fn longer() {
        let data: Vec<u8> = (0..100).collect();
        let (h1, _h2) = hash(&data, None, None);
        assert_ne!(h1, 0);
    }

    #[test]
    fn block_aligned() {
        // Exactly 16 bytes
        let (h1, _h2) = hash(b"0123456789abcdef", None, None);
        assert_ne!(h1, 0);
    }

    #[test]
    fn bytes_roundtrip() {
        let (h1, h2) = hash(b"test", None, None);
        let bytes = hash_bytes(b"test", None, None);

        let h1_check = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
        let h2_check = u64::from_le_bytes(bytes[8..16].try_into().unwrap());

        assert_eq!(h1, h1_check);
        assert_eq!(h2, h2_check);
    }
}
