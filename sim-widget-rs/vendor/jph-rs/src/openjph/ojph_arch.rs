//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/openjph/ojph_arch.h`

use crate::openjph::ojph_defs::{Si32, Ui32, Ui64};
use std::sync::OnceLock;

pub const X86_CPU_EXT_LEVEL_GENERIC: i32 = 0;
pub const X86_CPU_EXT_LEVEL_MMX: i32 = 1;
pub const X86_CPU_EXT_LEVEL_SSE: i32 = 2;
pub const X86_CPU_EXT_LEVEL_SSE2: i32 = 3;
pub const X86_CPU_EXT_LEVEL_SSE3: i32 = 4;
pub const X86_CPU_EXT_LEVEL_SSSE3: i32 = 5;
pub const X86_CPU_EXT_LEVEL_SSE41: i32 = 6;
pub const X86_CPU_EXT_LEVEL_SSE42: i32 = 7;
pub const X86_CPU_EXT_LEVEL_AVX: i32 = 8;
pub const X86_CPU_EXT_LEVEL_AVX2: i32 = 9;
pub const X86_CPU_EXT_LEVEL_AVX2FMA: i32 = 10;
pub const X86_CPU_EXT_LEVEL_AVX512: i32 = 11;

pub const ARM_CPU_EXT_LEVEL_GENERIC: i32 = 0;
pub const ARM_CPU_EXT_LEVEL_NEON: i32 = 1;
pub const ARM_CPU_EXT_LEVEL_ASIMD: i32 = 1;
pub const ARM_CPU_EXT_LEVEL_SVE: i32 = 2;
pub const ARM_CPU_EXT_LEVEL_SVE2: i32 = 3;

pub const BYTE_ALIGNMENT: Ui32 = if cfg!(target_arch = "wasm32") { 16 } else { 64 };
pub const LOG_BYTE_ALIGNMENT: Ui32 = 31 - count_leading_zeros_u32(BYTE_ALIGNMENT);
pub const OBJECT_ALIGNMENT: Ui32 = 8;

static CPU_EXT_LEVEL: OnceLock<i32> = OnceLock::new();

/// Benchmark/diagnostic: force scalar HT block encode at AVX2 tier (`generic|avx2`).
pub const ENV_FORCE_ENCODE: &str = "OJPH_FORCE_ENCODE";
/// Benchmark/diagnostic: force reversible wavelet tier (`sse2|avx2`).
pub const ENV_FORCE_WAVELET_REV: &str = "OJPH_FORCE_WAVELET_REV";
/// Benchmark/diagnostic: force tile→codeblock transform tier (`sse2|avx2`).
pub const ENV_FORCE_TX: &str = "OJPH_FORCE_TX";

fn env_flag_is(name: &str, values: &[&str]) -> bool {
    let Ok(raw) = std::env::var(name) else {
        return false;
    };
    let v = raw.trim().to_ascii_lowercase();
    values.iter().any(|want| v == *want)
}

/// The `OJPH_FORCE_*` diagnostics, read from the environment ONCE (they are consulted for every
/// code-block's dispatch setup, so per-call `getenv` would be pure overhead).
struct ForceFlags {
    encode_generic: bool,
    wavelet_rev_sse2: bool,
    tx_sse2: bool,
}

fn force_flags() -> &'static ForceFlags {
    static FLAGS: OnceLock<ForceFlags> = OnceLock::new();
    FLAGS.get_or_init(|| ForceFlags {
        encode_generic: env_flag_is(ENV_FORCE_ENCODE, &["generic", "scalar", "gen", "0"]),
        wavelet_rev_sse2: env_flag_is(
            ENV_FORCE_WAVELET_REV,
            &["sse2", "sse", "generic", "scalar", "0"],
        ),
        tx_sse2: env_flag_is(ENV_FORCE_TX, &["sse2", "sse", "generic", "scalar", "0"]),
    })
}

pub fn force_encode_generic() -> bool {
    force_flags().encode_generic
}

pub fn force_wavelet_rev_sse2() -> bool {
    force_flags().wavelet_rev_sse2
}

pub fn force_tx_sse2() -> bool {
    force_flags().tx_sse2
}

/// Cap SIMD dispatch (`OJPH_CPU_MAX_LEVEL=<0-11>`): `min(detected, cap)`.
pub const ENV_CPU_MAX_LEVEL: &str = "OJPH_CPU_MAX_LEVEL";
/// Force an exact SIMD tier (`OJPH_CPU_FORCE_LEVEL=<0-11>`) for benchmarks. By default the request
/// is clamped to what the CPU supports (`min(detected, forced)`): dispatching to an unsupported
/// tier is an illegal-instruction crash in every consumer that merely inherits the environment.
/// The cargo feature `unsafe-force-simd` lifts the clamp (bench / crash testing only).
pub const ENV_CPU_FORCE_LEVEL: &str = "OJPH_CPU_FORCE_LEVEL";

pub fn get_cpu_ext_level() -> i32 {
    *CPU_EXT_LEVEL.get_or_init(init_cpu_ext_level)
}

/// Raw feature detection without `OJPH_CPU_*` overrides (for benchmark reporting).
pub fn detected_cpu_ext_level() -> i32 {
    detect_cpu_ext_level()
}

/// Human-readable label for an x86 SIMD tier (used in benchmark reports).
pub fn cpu_ext_level_name(level: i32) -> &'static str {
    match level {
        X86_CPU_EXT_LEVEL_AVX512 => "avx512",
        X86_CPU_EXT_LEVEL_AVX2FMA => "avx2+fma",
        X86_CPU_EXT_LEVEL_AVX2 => "avx2",
        X86_CPU_EXT_LEVEL_AVX => "avx",
        X86_CPU_EXT_LEVEL_SSE42 => "sse4.2",
        X86_CPU_EXT_LEVEL_SSE41 => "sse4.1",
        X86_CPU_EXT_LEVEL_SSSE3 => "ssse3",
        X86_CPU_EXT_LEVEL_SSE3 => "sse3",
        X86_CPU_EXT_LEVEL_SSE2 => "sse2",
        X86_CPU_EXT_LEVEL_SSE => "sse",
        X86_CPU_EXT_LEVEL_MMX => "mmx",
        _ => "generic",
    }
}

pub(crate) fn apply_cpu_max_level_cap(detected: i32) -> i32 {
    let Ok(raw) = std::env::var(ENV_CPU_MAX_LEVEL) else {
        return detected;
    };
    let Ok(cap) = raw.trim().parse::<i32>() else {
        return detected;
    };
    detected.min(cap)
}

fn detect_cpu_ext_level() -> i32 {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        let mut level = X86_CPU_EXT_LEVEL_GENERIC;

        if std::arch::is_x86_feature_detected!("mmx") {
            level = X86_CPU_EXT_LEVEL_MMX;
            if std::arch::is_x86_feature_detected!("sse") {
                level = X86_CPU_EXT_LEVEL_SSE;
                if std::arch::is_x86_feature_detected!("sse2") {
                    level = X86_CPU_EXT_LEVEL_SSE2;
                    if std::arch::is_x86_feature_detected!("sse3") {
                        level = X86_CPU_EXT_LEVEL_SSE3;
                        if std::arch::is_x86_feature_detected!("ssse3") {
                            level = X86_CPU_EXT_LEVEL_SSSE3;
                            if std::arch::is_x86_feature_detected!("sse4.1") {
                                level = X86_CPU_EXT_LEVEL_SSE41;
                                if std::arch::is_x86_feature_detected!("sse4.2") {
                                    level = X86_CPU_EXT_LEVEL_SSE42;
                                    if std::arch::is_x86_feature_detected!("avx") {
                                        level = X86_CPU_EXT_LEVEL_AVX;
                                        if std::arch::is_x86_feature_detected!("avx2") {
                                            level = X86_CPU_EXT_LEVEL_AVX2;
                                            if std::arch::is_x86_feature_detected!("fma") {
                                                level = X86_CPU_EXT_LEVEL_AVX2FMA;
                                            }
                                            // OpenJPH v0.31.0 hardening: the AVX-512 code paths
                                            // also use byte/word (BW) and vector-length (VL)
                                            // instructions, not just foundation (F) and
                                            // conflict-detection (CD). Checking only F+CD lets a
                                            // CPU that has F/CD but not BW/VL (e.g. some older
                                            // Knights-Landing-class Xeon Phi parts) dispatch into
                                            // AVX-512 code and hit an illegal instruction.
                                            if std::arch::is_x86_feature_detected!("avx512f")
                                                && std::arch::is_x86_feature_detected!("avx512cd")
                                                && std::arch::is_x86_feature_detected!("avx512bw")
                                                && std::arch::is_x86_feature_detected!("avx512vl")
                                            {
                                                level = X86_CPU_EXT_LEVEL_AVX512;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        return level;
    }

    #[cfg(target_arch = "aarch64")]
    {
        let mut level = ARM_CPU_EXT_LEVEL_GENERIC;
        if std::arch::is_aarch64_feature_detected!("neon") {
            level = ARM_CPU_EXT_LEVEL_ASIMD;
            if std::arch::is_aarch64_feature_detected!("sve") {
                level = ARM_CPU_EXT_LEVEL_SVE;
                if std::arch::is_aarch64_feature_detected!("sve2") {
                    level = ARM_CPU_EXT_LEVEL_SVE2;
                }
            }
        }
        return level;
    }

    #[cfg(target_arch = "arm")]
    {
        let mut level = ARM_CPU_EXT_LEVEL_GENERIC;
        if std::arch::is_arm_feature_detected!("neon") {
            level = ARM_CPU_EXT_LEVEL_NEON;
        }
        return level;
    }

    #[cfg(target_arch = "wasm32")]
    {
        if cfg!(target_feature = "simd128") {
            return 1;
        }
        return 0;
    }

    #[cfg(not(any(
        target_arch = "x86",
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "arm",
        target_arch = "wasm32"
    )))]
    {
        0
    }
}

fn parse_env_i32(name: &str) -> Option<i32> {
    let raw = std::env::var(name).ok()?;
    raw.trim().parse::<i32>().ok()
}

fn init_cpu_ext_level() -> i32 {
    let detected = detect_cpu_ext_level();
    if let Some(force) = parse_env_i32(ENV_CPU_FORCE_LEVEL) {
        // Force wins over cap (benchmark workers clear OJPH_CPU_MAX_LEVEL).
        return if cfg!(feature = "unsafe-force-simd") { force } else { force.min(detected) };
    }
    apply_cpu_max_level_cap(detected)
}

/// SIMD tiers to sweep in `ojph_bench --compare-simd` (ascending capability).
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub const BENCH_CPU_LEVELS: &[(i32, &'static str)] = &[
    (X86_CPU_EXT_LEVEL_GENERIC, "generic"),
    (X86_CPU_EXT_LEVEL_SSE2, "sse2"),
    (X86_CPU_EXT_LEVEL_AVX, "avx"),
    (X86_CPU_EXT_LEVEL_AVX2, "avx2"),
    (X86_CPU_EXT_LEVEL_AVX512, "avx512"),
];

#[inline]
pub const fn population_count(val: Ui32) -> Ui32 {
    val.count_ones()
}

#[inline]
pub const fn population_count64(val: Ui64) -> Ui32 {
    val.count_ones()
}

#[inline]
pub const fn count_leading_zeros_u32(val: Ui32) -> Ui32 {
    val.leading_zeros()
}

#[inline]
pub const fn count_leading_zeros_u64(val: Ui64) -> Ui32 {
    val.leading_zeros()
}

#[inline]
pub const fn count_trailing_zeros_u32(val: Ui32) -> Ui32 {
    val.trailing_zeros()
}

#[inline]
pub const fn swap_byte_u16(t: u16) -> u16 {
    t.swap_bytes()
}

#[inline]
pub const fn swap_byte_u32(t: u32) -> u32 {
    t.swap_bytes()
}

#[inline]
pub const fn swap_byte_u64(t: u64) -> u64 {
    t.swap_bytes()
}

#[inline]
pub fn ojph_round(val: f32) -> Si32 {
    (val + if val >= 0.0 { 0.5 } else { -0.5 }) as Si32
}

#[inline]
pub fn ojph_trunc(val: f32) -> Si32 {
    val as Si32
}

#[inline]
pub fn calc_aligned_size<T, const N: u32>(size: usize) -> usize {
    let mut size = size * core::mem::size_of::<T>() + N as usize - 1;
    size &= !((1usize << (31 - count_leading_zeros_u32(N))) - 1);
    size >>= 63 - count_leading_zeros_u64(core::mem::size_of::<T>() as Ui64);
    size
}

#[inline]
pub fn align_ptr<T, const N: u32>(ptr: *mut T) -> *mut T {
    let mut p = ptr as usize;
    p += N as usize - 1;
    p &= !((1usize << (31 - count_leading_zeros_u32(N))) - 1);
    p as *mut T
}

#[cfg(test)]
mod tests {
    use super::{
        BYTE_ALIGNMENT, ENV_CPU_MAX_LEVEL, LOG_BYTE_ALIGNMENT, OBJECT_ALIGNMENT,
        X86_CPU_EXT_LEVEL_AVX512, align_ptr, apply_cpu_max_level_cap, calc_aligned_size,
        count_leading_zeros_u32, count_trailing_zeros_u32, get_cpu_ext_level, ojph_round,
        ojph_trunc, population_count, swap_byte_u16, swap_byte_u32, swap_byte_u64,
    };

    #[test]
    fn alignment_constants_match_reference_policy() {
        if cfg!(target_arch = "wasm32") {
            assert_eq!(BYTE_ALIGNMENT, 16);
        } else {
            assert_eq!(BYTE_ALIGNMENT, 64);
        }
        assert_eq!(OBJECT_ALIGNMENT, 8);
        assert_eq!(
            LOG_BYTE_ALIGNMENT,
            31 - count_leading_zeros_u32(BYTE_ALIGNMENT)
        );
    }

    #[test]
    fn bit_helpers_match_expected_values() {
        assert_eq!(population_count(0b101101), 4);
        assert_eq!(count_leading_zeros_u32(1), 31);
        assert_eq!(count_trailing_zeros_u32(8), 3);
        assert_eq!(swap_byte_u16(0x1234), 0x3412);
        assert_eq!(swap_byte_u32(0x1234_5678), 0x7856_3412);
        assert_eq!(swap_byte_u64(0x0123_4567_89AB_CDEF), 0xEFCD_AB89_6745_2301);
    }

    #[test]
    fn rounding_helpers_match_openjph_behavior() {
        assert_eq!(ojph_round(1.4), 1);
        assert_eq!(ojph_round(1.5), 2);
        assert_eq!(ojph_round(-1.5), -2);
        assert_eq!(ojph_trunc(1.9), 1);
        assert_eq!(ojph_trunc(-1.9), -1);
    }

    #[test]
    fn alignment_templates_match_expected_shapes() {
        assert_eq!(calc_aligned_size::<u8, 8>(1), 8);
        assert_eq!(calc_aligned_size::<u32, 8>(1), 2);

        let mut buf = [0u8; 32];
        let ptr = buf.as_mut_ptr().wrapping_add(1);
        let aligned = align_ptr::<u8, 8>(ptr) as usize;
        assert_eq!(aligned % 8, 0);
    }

    #[test]
    fn cpu_extension_level_stays_in_known_range() {
        let level = get_cpu_ext_level();
        assert!((0..=X86_CPU_EXT_LEVEL_AVX512).contains(&level));
    }

    #[test]
    fn cpu_max_level_cap_clamps_detected_level() {
        unsafe {
            std::env::remove_var(ENV_CPU_MAX_LEVEL);
        }
        assert_eq!(
            apply_cpu_max_level_cap(X86_CPU_EXT_LEVEL_AVX512),
            X86_CPU_EXT_LEVEL_AVX512
        );
        unsafe {
            std::env::set_var(ENV_CPU_MAX_LEVEL, "9");
        }
        assert_eq!(apply_cpu_max_level_cap(X86_CPU_EXT_LEVEL_AVX512), 9);
        unsafe {
            std::env::remove_var(ENV_CPU_MAX_LEVEL);
        }
    }
}
