//! Port target:
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/coding/ojph_block_common.h`
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/coding/ojph_block_common.cpp`

use std::sync::LazyLock;

use crate::openjph::ojph_defs::{Ui16, Ui32, Ui8};

use super::{table0::TABLE0, table1::TABLE1};

#[derive(Copy, Clone)]
pub(crate) struct VlcSrcTable {
    pub(crate) c_q: i32,
    pub(crate) rho: i32,
    pub(crate) u_off: i32,
    pub(crate) e_k: i32,
    pub(crate) e_1: i32,
    pub(crate) cwd: i32,
    pub(crate) cwd_len: i32,
}

#[allow(non_upper_case_globals)]
pub static vlc_tbl0: LazyLock<[Ui16; 1024]> = LazyLock::new(|| {
    let debug = false;
    let mut table = [0; 1024];

    if debug {
        table.fill(0);
    }

    for i in 0..1024 {
        let cwd = i & 0x7F;
        let c_q = i >> 7;

        for entry in TABLE0 {
            if entry.c_q == c_q && entry.cwd == (cwd & ((1_i32 << entry.cwd_len) - 1)) {
                if debug {
                    assert_eq!(table[i as usize], 0);
                }

                table[i as usize] = ((entry.rho << 4)
                    | (entry.u_off << 3)
                    | (entry.e_k << 12)
                    | (entry.e_1 << 8)
                    | entry.cwd_len) as Ui16;
            }
        }
    }

    table
});

#[allow(non_upper_case_globals)]
pub static vlc_tbl1: LazyLock<[Ui16; 1024]> = LazyLock::new(|| {
    let debug = false;
    let mut table = [0; 1024];

    if debug {
        table.fill(0);
    }

    for i in 0..1024 {
        let cwd = i & 0x7F;
        let c_q = i >> 7;

        for entry in TABLE1 {
            if entry.c_q == c_q && entry.cwd == (cwd & ((1_i32 << entry.cwd_len) - 1)) {
                if debug {
                    assert_eq!(table[i as usize], 0);
                }

                table[i as usize] = ((entry.rho << 4)
                    | (entry.u_off << 3)
                    | (entry.e_k << 12)
                    | (entry.e_1 << 8)
                    | entry.cwd_len) as Ui16;
            }
        }
    }

    table
});

#[allow(non_upper_case_globals)]
pub static uvlc_tbl0: LazyLock<[Ui16; 256 + 64]> = LazyLock::new(|| {
    let mut table = [0; 256 + 64];

    const DEC: [Ui8; 8] = [
        3 | (5 << 2) | (5 << 5),
        1 | (0 << 2) | (1 << 5),
        2 | (0 << 2) | (2 << 5),
        1 | (0 << 2) | (1 << 5),
        3 | (1 << 2) | (3 << 5),
        1 | (0 << 2) | (1 << 5),
        2 | (0 << 2) | (2 << 5),
        1 | (0 << 2) | (1 << 5),
    ];

    for i in 0..((256 + 64) as Ui32) {
        let mode = i >> 6;
        let mut vlc = i & 0x3F;

        if mode == 0 {
            table[i as usize] = 0;
        } else if mode <= 2 {
            let d = DEC[(vlc & 0x7) as usize] as Ui32;
            let total_prefix = d & 0x3;
            let total_suffix = (d >> 2) & 0x7;
            let u0_suffix_len = if mode == 1 { total_suffix } else { 0 };
            let u0 = if mode == 1 { d >> 5 } else { 0 };
            let u1 = if mode == 1 { 0 } else { d >> 5 };

            table[i as usize] = (total_prefix
                | (total_suffix << 3)
                | (u0_suffix_len << 7)
                | (u0 << 10)
                | (u1 << 13)) as Ui16;
        } else if mode == 3 {
            let d0 = DEC[(vlc & 0x7) as usize] as Ui32;
            vlc >>= d0 & 0x3;
            let d1 = DEC[(vlc & 0x7) as usize] as Ui32;

            let (total_prefix, u0_suffix_len, total_suffix, u0, u1) = if (d0 & 0x3) == 3 {
                let total_prefix = (d0 & 0x3) + 1;
                let u0_suffix_len = (d0 >> 2) & 0x7;
                let total_suffix = u0_suffix_len;
                let u0 = d0 >> 5;
                let u1 = (vlc & 1) + 1;
                (total_prefix, u0_suffix_len, total_suffix, u0, u1)
            } else {
                let total_prefix = (d0 & 0x3) + (d1 & 0x3);
                let u0_suffix_len = (d0 >> 2) & 0x7;
                let total_suffix = u0_suffix_len + ((d1 >> 2) & 0x7);
                let u0 = d0 >> 5;
                let u1 = d1 >> 5;
                (total_prefix, u0_suffix_len, total_suffix, u0, u1)
            };

            table[i as usize] = (total_prefix
                | (total_suffix << 3)
                | (u0_suffix_len << 7)
                | (u0 << 10)
                | (u1 << 13)) as Ui16;
        } else if mode == 4 {
            let d0 = DEC[(vlc & 0x7) as usize] as Ui32;
            vlc >>= d0 & 0x3;
            let d1 = DEC[(vlc & 0x7) as usize] as Ui32;

            let total_prefix = (d0 & 0x3) + (d1 & 0x3);
            let u0_suffix_len = (d0 >> 2) & 0x7;
            let total_suffix = u0_suffix_len + ((d1 >> 2) & 0x7);
            let u0 = (d0 >> 5) + 2;
            let u1 = (d1 >> 5) + 2;

            table[i as usize] = (total_prefix
                | (total_suffix << 3)
                | (u0_suffix_len << 7)
                | (u0 << 10)
                | (u1 << 13)) as Ui16;
        }
    }

    table
});

#[allow(non_upper_case_globals)]
pub static uvlc_tbl1: LazyLock<[Ui16; 256]> = LazyLock::new(|| {
    let mut table = [0; 256];

    const DEC: [Ui8; 8] = [
        3 | (5 << 2) | (5 << 5),
        1 | (0 << 2) | (1 << 5),
        2 | (0 << 2) | (2 << 5),
        1 | (0 << 2) | (1 << 5),
        3 | (1 << 2) | (3 << 5),
        1 | (0 << 2) | (1 << 5),
        2 | (0 << 2) | (2 << 5),
        1 | (0 << 2) | (1 << 5),
    ];

    for i in 0..(256 as Ui32) {
        let mode = i >> 6;
        let mut vlc = i & 0x3F;

        if mode == 0 {
            table[i as usize] = 0;
        } else if mode <= 2 {
            let d = DEC[(vlc & 0x7) as usize] as Ui32;
            let total_prefix = d & 0x3;
            let total_suffix = (d >> 2) & 0x7;
            let u0_suffix_len = if mode == 1 { total_suffix } else { 0 };
            let u0 = if mode == 1 { d >> 5 } else { 0 };
            let u1 = if mode == 1 { 0 } else { d >> 5 };

            table[i as usize] = (total_prefix
                | (total_suffix << 3)
                | (u0_suffix_len << 7)
                | (u0 << 10)
                | (u1 << 13)) as Ui16;
        } else if mode == 3 {
            let d0 = DEC[(vlc & 0x7) as usize] as Ui32;
            vlc >>= d0 & 0x3;
            let d1 = DEC[(vlc & 0x7) as usize] as Ui32;

            let total_prefix = (d0 & 0x3) + (d1 & 0x3);
            let u0_suffix_len = (d0 >> 2) & 0x7;
            let total_suffix = u0_suffix_len + ((d1 >> 2) & 0x7);
            let u0 = d0 >> 5;
            let u1 = d1 >> 5;

            table[i as usize] = (total_prefix
                | (total_suffix << 3)
                | (u0_suffix_len << 7)
                | (u0 << 10)
                | (u1 << 13)) as Ui16;
        }
    }

    table
});

/// Wider UVLC table for non-initial quad rows (0.31.0 `uvlc_tbl1_wide`, OpenJPH
/// `ojph_block_common.cpp`'s `uvlc_init_wide_table`). Consumed only by the AVX2 decoder's
/// cleanup-pass fast path (`decode_step1_vlc_mel_32`'s non-initial-row loop): one lookup at
/// `[(uvlc_mode << 10) | (vlc_val & 0x3FF)]` decodes BOTH quads' U-values in a single table read
/// when `total_bits != 0x1F` (the common case), instead of `uvlc_tbl1`'s narrower per-quad
/// decode (still used as the fallback when `total_bits == 0x1F`, i.e. the combined prefix+suffix
/// length would exceed the 10 index bits this table can address).
///
/// Index = `mode(2 bits) * 1024 + vlc_data(10 bits)`. Entry bits: `[4:0]` = total_bits consumed
/// (`0x1F` = fallback sentinel), `[12:5]` = `u_q0`, `[20:13]` = `u_q1`.
#[allow(non_upper_case_globals)]
pub static uvlc_tbl1_wide: LazyLock<[Ui32; 4096]> = LazyLock::new(|| {
    let mut table = [0u32; 4096];

    const DEC: [Ui8; 8] = [
        3 | (5 << 2) | (5 << 5),
        1 | (0 << 2) | (1 << 5),
        2 | (0 << 2) | (2 << 5),
        1 | (0 << 2) | (1 << 5),
        3 | (1 << 2) | (3 << 5),
        1 | (0 << 2) | (1 << 5),
        2 | (0 << 2) | (2 << 5),
        1 | (0 << 2) | (1 << 5),
    ];

    for idx in 0..4096u32 {
        let mode = idx >> 10;
        let vlc = idx & 0x3FF;

        if mode == 0 {
            table[idx as usize] = 0;
            continue;
        }

        if mode <= 2 {
            // single UVLC (one u_off set)
            let d = DEC[(vlc & 0x7) as usize] as Ui32;
            let prefix_len = d & 0x3;
            let suffix_len = (d >> 2) & 0x7;
            let u_pfx = d >> 5;
            let suffix_val = (vlc >> prefix_len) & ((1u32 << suffix_len) - 1);
            let u_val = u_pfx + suffix_val;
            let total = prefix_len + suffix_len;
            let u_q0 = if mode == 1 { u_val } else { 0 };
            let u_q1 = if mode == 2 { u_val } else { 0 };
            table[idx as usize] = total | (u_q0 << 5) | (u_q1 << 13);
            continue;
        }

        // mode == 3: both u_off set. Bitstream layout: [prefix0][prefix1][suffix0][suffix1].
        let d0 = DEC[(vlc & 0x7) as usize] as Ui32;
        let p0_len = d0 & 0x3;
        let s0_len = (d0 >> 2) & 0x7;
        let u0_pfx = d0 >> 5;

        let vlc1 = vlc >> p0_len; // consume prefix0
        let d1 = DEC[(vlc1 & 0x7) as usize] as Ui32;
        let p1_len = d1 & 0x3;
        let s1_len = (d1 >> 2) & 0x7;
        let u1_pfx = d1 >> 5;

        let total_prefix = p0_len + p1_len;
        let total_suffix = s0_len + s1_len;
        let total = total_prefix + total_suffix;

        if total > 10 {
            table[idx as usize] = 0x1F; // fallback sentinel
            continue;
        }

        // suffixes follow both prefixes in the bitstream
        let suffix_bits = vlc >> total_prefix;
        let s0_val = suffix_bits & ((1u32 << s0_len) - 1);
        let s1_val = (suffix_bits >> s0_len) & ((1u32 << s1_len) - 1);

        let u_q0 = u0_pfx + s0_val;
        let u_q1 = u1_pfx + s1_val;
        table[idx as usize] = total | (u_q0 << 5) | (u_q1 << 13);
    }

    table
});

#[allow(non_upper_case_globals)]
pub static uvlc_bias: LazyLock<[Ui8; 256 + 64]> = LazyLock::new(|| {
    let mut table = [0; 256 + 64];

    const DEC: [Ui8; 8] = [
        3 | (5 << 2) | (5 << 5),
        1 | (0 << 2) | (1 << 5),
        2 | (0 << 2) | (2 << 5),
        1 | (0 << 2) | (1 << 5),
        3 | (1 << 2) | (3 << 5),
        1 | (0 << 2) | (1 << 5),
        2 | (0 << 2) | (2 << 5),
        1 | (0 << 2) | (1 << 5),
    ];

    for i in 0..((256 + 64) as Ui32) {
        let mode = i >> 6;
        let mut vlc = i & 0x3F;

        if mode == 0 {
            table[i as usize] = 0;
        } else if mode <= 2 {
            table[i as usize] = 0;
        } else if mode == 3 {
            let d0 = DEC[(vlc & 0x7) as usize] as Ui32;
            vlc >>= d0 & 0x3;
            let d1 = DEC[(vlc & 0x7) as usize] as Ui32;

            if (d0 & 0x3) == 3 {
                let _ = d1;
                table[i as usize] = 4;
            } else {
                table[i as usize] = 0;
            }
        } else if mode == 4 {
            table[i as usize] = 10;
        }
    }

    table
});

#[cfg(test)]
mod tests {
    use super::{uvlc_bias, uvlc_tbl0, uvlc_tbl1, vlc_tbl0, vlc_tbl1};

    #[test]
    fn vlc_tables_initialize() {
        assert_ne!(vlc_tbl0[1], 0);
        assert_ne!(vlc_tbl1[1], 0);
    }

    #[test]
    fn uvlc_tables_initialize() {
        assert_eq!(uvlc_tbl0[0], 0);
        assert_eq!(uvlc_tbl1[0], 0);
        assert_eq!(uvlc_bias[0], 0);
    }
}
