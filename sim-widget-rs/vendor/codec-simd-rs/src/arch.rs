//! CPU feature detection and tier dispatch.
//!
//! Model adapted from `jph-rs::openjph::ojph_arch`.  Tier integers follow the
//! x86 ladder by convention; AArch64 and wasm have their own scales.

use std::sync::OnceLock;

// --- x86 tier ladder -------------------------------------------------------

pub const X86_GENERIC: i32 = 0;
pub const X86_MMX: i32 = 1;
pub const X86_SSE: i32 = 2;
pub const X86_SSE2: i32 = 3;
pub const X86_SSE3: i32 = 4;
pub const X86_SSSE3: i32 = 5;
pub const X86_SSE41: i32 = 6;
pub const X86_SSE42: i32 = 7;
pub const X86_AVX: i32 = 8;
pub const X86_AVX2: i32 = 9;
pub const X86_AVX2_FMA: i32 = 10;
pub const X86_AVX512: i32 = 11;

// --- ARM tier ladder -------------------------------------------------------

pub const ARM_GENERIC: i32 = 0;
pub const ARM_NEON: i32 = 1;
pub const ARM_SVE: i32 = 2;
pub const ARM_SVE2: i32 = 3;

// --- wasm tier ladder ------------------------------------------------------

pub const WASM_GENERIC: i32 = 0;
pub const WASM_SIMD128: i32 = 1;

// --- env-var overrides -----------------------------------------------------

/// Pin dispatch to a specific tier integer.  Hardware must actually support
/// the tier — otherwise selected impls UB at runtime.  Use for bit-exact
/// conformance sweeps.
pub const ENV_FORCE_LEVEL: &str = "CODEC_SIMD_FORCE_LEVEL";

/// Cap dispatch at this tier.  `min(detected, cap)`.  Use to bisect which
/// tier introduced a regression.
pub const ENV_MAX_LEVEL: &str = "CODEC_SIMD_MAX_LEVEL";

// --- accessors -------------------------------------------------------------

static CPU_LEVEL: OnceLock<i32> = OnceLock::new();

/// Tier integer of the CPU running this process, after applying any
/// `CODEC_SIMD_FORCE_LEVEL` / `CODEC_SIMD_MAX_LEVEL` overrides.  Cached
/// after first call.
pub fn cpu_level() -> i32 {
    *CPU_LEVEL.get_or_init(init_cpu_level)
}

/// Tier integer of the CPU as detected, without env-var overrides.  For
/// benchmark reporting.
pub fn detected_cpu_level() -> i32 {
    detect_cpu_level()
}

/// Human-readable name of a tier integer (architecture-aware).
pub fn cpu_level_name(level: i32) -> &'static str {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    {
        return match level {
            X86_AVX512 => "avx512",
            X86_AVX2_FMA => "avx2+fma",
            X86_AVX2 => "avx2",
            X86_AVX => "avx",
            X86_SSE42 => "sse4.2",
            X86_SSE41 => "sse4.1",
            X86_SSSE3 => "ssse3",
            X86_SSE3 => "sse3",
            X86_SSE2 => "sse2",
            X86_SSE => "sse",
            X86_MMX => "mmx",
            _ => "generic",
        };
    }
    #[cfg(target_arch = "aarch64")]
    {
        return match level {
            ARM_SVE2 => "sve2",
            ARM_SVE => "sve",
            ARM_NEON => "neon",
            _ => "generic",
        };
    }
    #[cfg(target_arch = "wasm32")]
    {
        return match level {
            WASM_SIMD128 => "simd128",
            _ => "generic",
        };
    }
    #[cfg(not(any(
        target_arch = "x86_64",
        target_arch = "x86",
        target_arch = "aarch64",
        target_arch = "wasm32"
    )))]
    {
        let _ = level;
        "generic"
    }
}

// --- internals -------------------------------------------------------------

fn init_cpu_level() -> i32 {
    let detected = detect_cpu_level();
    if let Some(force) = parse_env_i32(ENV_FORCE_LEVEL) {
        return force;
    }
    apply_max_cap(detected)
}

fn detect_cpu_level() -> i32 {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    {
        let mut level = X86_GENERIC;
        if std::arch::is_x86_feature_detected!("mmx") {
            level = X86_MMX;
            if std::arch::is_x86_feature_detected!("sse") {
                level = X86_SSE;
                if std::arch::is_x86_feature_detected!("sse2") {
                    level = X86_SSE2;
                    if std::arch::is_x86_feature_detected!("sse3") {
                        level = X86_SSE3;
                        if std::arch::is_x86_feature_detected!("ssse3") {
                            level = X86_SSSE3;
                            if std::arch::is_x86_feature_detected!("sse4.1") {
                                level = X86_SSE41;
                                if std::arch::is_x86_feature_detected!("sse4.2") {
                                    level = X86_SSE42;
                                    if std::arch::is_x86_feature_detected!("avx") {
                                        level = X86_AVX;
                                        if std::arch::is_x86_feature_detected!("avx2") {
                                            level = X86_AVX2;
                                            if std::arch::is_x86_feature_detected!("fma") {
                                                level = X86_AVX2_FMA;
                                            }
                                            if std::arch::is_x86_feature_detected!("avx512f")
                                                && std::arch::is_x86_feature_detected!("avx512cd")
                                            {
                                                level = X86_AVX512;
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
        let mut level = ARM_GENERIC;
        if std::arch::is_aarch64_feature_detected!("neon") {
            level = ARM_NEON;
            if std::arch::is_aarch64_feature_detected!("sve") {
                level = ARM_SVE;
                if std::arch::is_aarch64_feature_detected!("sve2") {
                    level = ARM_SVE2;
                }
            }
        }
        return level;
    }
    #[cfg(target_arch = "wasm32")]
    {
        if cfg!(target_feature = "simd128") {
            return WASM_SIMD128;
        }
        return WASM_GENERIC;
    }
    #[cfg(not(any(
        target_arch = "x86_64",
        target_arch = "x86",
        target_arch = "aarch64",
        target_arch = "wasm32"
    )))]
    {
        0
    }
}

fn parse_env_i32(name: &str) -> Option<i32> {
    std::env::var(name).ok()?.trim().parse::<i32>().ok()
}

fn apply_max_cap(detected: i32) -> i32 {
    match parse_env_i32(ENV_MAX_LEVEL) {
        Some(cap) => detected.min(cap),
        None => detected,
    }
}
