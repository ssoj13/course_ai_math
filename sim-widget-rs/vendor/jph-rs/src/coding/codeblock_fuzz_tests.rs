//! Differential fuzz harness at the **individual code-block** level: encode/decode a code-block
//! directly (bypassing the tile/subband machinery) so every SIMD tier can be driven with the same
//! raw coefficient buffer and compared byte-for-byte. This is the correctness gate the AVX2
//! MEL/VLC/UVLC rewrite port (BUG1.md Round 9/10) must pass before any re-benchmark is trusted —
//! a subtle bit-packing bug here would silently corrupt real HTJ2K-compressed EXR chunks.
//!
//! Scope (32-bit path; the 64-bit path shares the same MEL/VLC/UVLC entropy-coding core reached
//! through `ojph_encode_codeblock64`/`ojph_decode_codeblock64`, which do not have AVX2 variants
//! to differential-test against): every SIMD tier available on this host (generic always; avx2
//! x86/x86_64; avx512 x86_64 when the CPU supports it) must produce a byte-identical coded
//! bitstream for the same input buffer, AND every decoder tier (generic; ssse3; avx2) must decode
//! any tier's coded bytes back to the exact original buffer.
//!
//! **Buffer geometry mirrors real production allocation, not just the "logical" code-block
//! size.** Per `ojph_subband.rs`/`ojph_codeblock.rs`, an actual code-block's `width`/`height`
//! passed to encode/decode can be *smaller* than its `nominal` size (a power-of-two, e.g. a
//! tile-edge code-block clipped to 37x23 out of a 64x64 nominal grid cell) — but `Codeblock`
//! always allocates its coefficient buffer at the *nominal* size with `stride =
//! round_up_16(nominal.w)` (`Codeblock::pre_alloc`/`finalize_alloc`), never at exactly
//! `width*height`. The HT codec's quad-based (4-column, 2-row) internal processing relies on
//! that headroom past the logical `width`/`height` edge. A first version of this harness
//! allocated buffers at exactly `width*height` with `stride == width` for clipped (non-power-of-
//! two) shapes and hit `STATUS_HEAP_CORRUPTION` — not a codec bug, a harness bug: production
//! code never actually gives the codec that little room. Fixed by allocating
//! `nominal_h * round_up_16(nominal_w)` and passing the padded `stride` (not `width`) to
//! encode/decode, exactly like `Codeblock` does, then comparing only the logical `width x
//! height` sub-rectangle.
//!
//! Deterministic xorshift PRNG (no external `rand` dependency, matching this crate's
//! zero-library-dependency policy) drives the coefficient magnitudes/signs so failures reproduce
//! from the printed seed.

use crate::codestream::ojph_codeblock_fun::{CbDecoderFun32, CbEncoderFun32};
use crate::coding::ojph_block_decoder32::ojph_decode_codeblock32;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use crate::coding::ojph_block_decoder_avx2::ojph_decode_codeblock_avx2;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use crate::coding::ojph_block_decoder_ssse3::ojph_decode_codeblock_ssse3;
use crate::coding::ojph_block_encoder::{initialize_block_encoder_tables, ojph_encode_codeblock32};
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use crate::coding::ojph_block_encoder_avx2::{
    initialize_block_encoder_tables_avx2, ojph_encode_codeblock_avx2,
};
#[cfg(target_arch = "x86_64")]
use crate::coding::ojph_block_encoder_avx512::{
    initialize_block_encoder_tables_avx512, ojph_encode_codeblock_avx512,
};
use crate::openjph::ojph_defs::Ui32;
use crate::openjph::ojph_mem::{mem_elastic_allocator, CodedLists};

/// Small deterministic xorshift32 PRNG — reproducible without pulling in `rand`.
struct Xorshift32(u32);

impl Xorshift32 {
    fn new(seed: u32) -> Self {
        // xorshift requires a non-zero state.
        Self(if seed == 0 { 0xDEAD_BEEF } else { seed })
    }

    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
}

/// One code-block-level test case. `nominal_w`/`nominal_h` are the (power-of-two) allocation
/// size `Codeblock` would actually use; `width`/`height` (<= nominal) are the logical, possibly
/// tile-edge-clipped, code-block size passed to encode/decode — see module doc.
#[derive(Clone, Copy)]
struct Case {
    nominal_w: Ui32,
    nominal_h: Ui32,
    width: Ui32,
    height: Ui32,
    /// Number of significant magnitude bits (`k_max`); `missing_msbs = k_max - 1` is what the
    /// encoder API takes (mirrors `Codeblock::encode`'s `self.k_max - 1`).
    k_max: Ui32,
    pattern: Pattern,
    seed: u32,
}

impl Case {
    /// Production stride: nominal width rounded up to 16 `Ui32` (`Codeblock::pre_alloc`'s
    /// `f = BYTE_ALIGNMENT/4 - 1 = 15; stride = (nominal.w + f) & !f`).
    fn stride(&self) -> Ui32 {
        (self.nominal_w + 15) & !15
    }

    /// Total `Ui32` buffer capacity a real `Codeblock` would allocate for this nominal size.
    fn buf_len(&self) -> usize {
        (self.stride() * self.nominal_h) as usize
    }
}

#[derive(Clone, Copy)]
enum Pattern {
    /// Every coefficient random (magnitude in `0..1<<k_max`, random sign).
    Random,
    /// Every coefficient at the extreme (`(1<<k_max)-1`), alternating sign — worst case for
    /// carry/borrow-sensitive bit-packing (MEL run-length, VLC u-value overflow).
    AllMax,
    /// Every coefficient zero except a sparse scatter of max-magnitude outliers — exercises
    /// MEL's zero-run coding.
    Sparse,
}

/// Build a full `nominal_h * stride()` coefficient buffer (matching real allocation), with the
/// logical `width x height` region filled in the format `Codeblock::push` produces
/// (`gen_rev_tx_to_cb32`/`gen_irv_tx_to_cb32`): bit 31 = sign, magnitude in
/// `val << (31 - k_max)`. Padding beyond `width`/`height` (within the nominal allocation) stays
/// zero, matching a freshly zero-allocated `Codeblock` buffer.
fn build_buffer(case: Case) -> Vec<Ui32> {
    let shift = 31 - case.k_max;
    let max_mag = (1u32 << case.k_max) - 1;
    let mut rng = Xorshift32::new(case.seed);
    let stride = case.stride() as usize;
    let mut buf = vec![0u32; case.buf_len()];
    for row in 0..case.height as usize {
        for col in 0..case.width as usize {
            let (mag, sign) = match case.pattern {
                Pattern::Random => {
                    let mag = rng.next_u32() & max_mag;
                    // A "negative zero" (sign set, magnitude 0) never occurs in real captured
                    // data (`gen_rev_tx_to_cb32` only sets the sign bit when the source pixel is
                    // genuinely negative, which implies nonzero magnitude) and the codec does
                    // not preserve a sign for an insignificant (zero-magnitude) coefficient —
                    // there is nothing to have a sign. Force sign=0 when mag==0 so this
                    // synthetic generator can't manufacture an unrepresentable state.
                    let sign = if mag != 0 && rng.next_u32() & 1 == 1 {
                        0x8000_0000
                    } else {
                        0
                    };
                    (mag, sign)
                }
                Pattern::AllMax => {
                    let sign = if (row + col) & 1 == 0 { 0x8000_0000 } else { 0 };
                    (max_mag, sign)
                }
                Pattern::Sparse => {
                    if rng.next_u32().is_multiple_of(7) {
                        let sign = if rng.next_u32() & 1 == 1 {
                            0x8000_0000
                        } else {
                            0
                        };
                        (max_mag, sign)
                    } else {
                        (0, 0)
                    }
                }
            };
            buf[row * stride + col] = sign | (mag << shift);
        }
    }
    buf
}

/// Extracts the logical `width x height` sub-rectangle out of a full nominal-strided buffer, for
/// comparing only the part that carries real data (padding is allocator headroom, not part of
/// the code-block's actual content).
fn logical_region(buf: &[Ui32], case: Case) -> Vec<Ui32> {
    let stride = case.stride() as usize;
    let mut out = Vec::with_capacity((case.width * case.height) as usize);
    for row in 0..case.height as usize {
        out.extend_from_slice(&buf[row * stride..row * stride + case.width as usize]);
    }
    out
}

/// Masks a logical region's values down to the bits the codec actually round-trips: bit 31
/// (sign) plus magnitude bits at and above `shift = 31 - k_max`. A HT cleanup-pass-only decode
/// (`num_passes == 1`, as this harness always uses — matching `Codeblock::encode`'s HT call)
/// reconstructs the coefficient at the *midpoint* of its quantization bin by setting the bit
/// immediately below `shift` to 1 (JPEG 2000 HT dequantization dithering) — real, spec-mandated
/// behavior, not a bug: `gen_rev_tx_from_cb32`'s inverse transform (`(v & 0x7FFFFFFF) >> shift`)
/// discards that bit before it ever reaches a pixel. This harness compares codeblock buffers
/// directly (bypassing the pixel-domain inverse transform), so it must mask the same bits away
/// itself, or every case looks like a false round-trip mismatch.
fn mask_to_shift(region: &mut [Ui32], case: Case) {
    let shift = 31 - case.k_max;
    let low_bits_mask = (1u32 << shift) - 1;
    for v in region.iter_mut() {
        *v &= !low_bits_mask;
    }
}

fn describe(case: Case, pattern_name: &str) -> String {
    format!(
        "nominal={}x{} logical={}x{} k_max={} pattern={pattern_name} seed={}",
        case.nominal_w, case.nominal_h, case.width, case.height, case.k_max, case.seed
    )
}

/// Runs `encoder` on `buf` (a full nominal-strided buffer, as built by [`build_buffer`]),
/// returns the coded bytes (copied out of the elastic allocator) and `lengths[0]` (the single-
/// pass byte length; `num_passes` is always 1 for the direct codeblock API — matches
/// `Codeblock::encode`'s always-1-pass HT call).
unsafe fn run_encoder(encoder: CbEncoderFun32, buf: &mut [Ui32], case: Case) -> (Vec<u8>, Ui32) {
    let missing_msbs = case.k_max - 1;
    let mut lengths = [0u32; 2];
    let mut elastic = mem_elastic_allocator::new(4096);
    let mut coded: *mut CodedLists = core::ptr::null_mut();
    unsafe {
        encoder(
            buf.as_mut_ptr(),
            missing_msbs,
            1,
            case.width,
            case.height,
            case.stride(),
            &mut lengths,
            &mut elastic,
            &mut coded,
        )
        .expect("codeblock encode must not error on well-formed synthetic input");
        let len = lengths[0] as usize;
        let bytes = if coded.is_null() {
            Vec::new()
        } else {
            core::slice::from_raw_parts((*coded).buf, len).to_vec()
        };
        (bytes, lengths[0])
    }
}

/// Decodes `coded` (produced by any encoder tier) with `decoder` into a full nominal-strided
/// buffer (matching real allocation), returns the logical `width x height` sub-rectangle for
/// comparison against the original.
unsafe fn run_decoder(
    decoder: CbDecoderFun32,
    coded: &mut [u8],
    lengths1: Ui32,
    case: Case,
) -> Vec<Ui32> {
    let missing_msbs = case.k_max - 1;
    let mut decoded = vec![0u32; case.buf_len()];
    unsafe {
        let ok = decoder(
            coded.as_mut_ptr(),
            decoded.as_mut_ptr(),
            missing_msbs,
            1,
            lengths1,
            0,
            case.width,
            case.height,
            case.stride(),
            false,
        );
        assert!(ok, "decode must report success on a well-formed codeblock");
    }
    logical_region(&decoded, case)
}

/// `(nominal_w, nominal_h, width, height)`: every full (unclipped) power-of-two nominal size the
/// HT codec supports (4x4 minimum .. 64x64 maximum code-block, both square and non-square), plus
/// tile-edge-clipped shapes (`width`/`height` < nominal) — including shapes not aligned to the
/// AVX2 (16-lane) or AVX-512 (32-lane) SIMD width (37x23, 61x64, 64x61, 5x7, 7x5) so the
/// tail-masking logic (`width & 15`/`width & 31` scalar copy-out path) is exercised on every
/// tier, and a 1x1 degenerate clip.
const GEOMETRIES: &[(Ui32, Ui32, Ui32, Ui32)] = &[
    (4, 4, 4, 4),
    (8, 8, 8, 8),
    (16, 16, 16, 16),
    (32, 32, 32, 32),
    (64, 64, 64, 64),
    (32, 64, 32, 64),
    (64, 32, 64, 32),
    (8, 8, 5, 7),
    (8, 8, 7, 5),
    (16, 16, 1, 1),
    (64, 64, 37, 23),
    (64, 64, 61, 64),
    (64, 64, 64, 61),
    (64, 32, 64, 17),
    (32, 64, 17, 64),
];

/// `k_max` values: 1 (minimum — a single significant bit), a couple of mid-range values, and 30
/// (near the 31-bit magnitude ceiling `gen_*_tx_to_cb32` can produce).
const K_MAX_VALUES: &[Ui32] = &[1, 2, 5, 13, 24, 30];

fn all_cases() -> Vec<(Case, &'static str)> {
    let mut out = Vec::new();
    let mut seed = 1u32;
    for &(nw, nh, w, h) in GEOMETRIES {
        for &k in K_MAX_VALUES {
            for (pattern, name) in [
                (Pattern::Random, "random"),
                (Pattern::AllMax, "all_max"),
                (Pattern::Sparse, "sparse"),
            ] {
                out.push((
                    Case {
                        nominal_w: nw,
                        nominal_h: nh,
                        width: w,
                        height: h,
                        k_max: k,
                        pattern,
                        seed,
                    },
                    name,
                ));
                seed = seed.wrapping_mul(2654435761).wrapping_add(1);
            }
        }
    }
    out
}

/// Encodes every case with the generic (scalar, source-of-truth) encoder, decodes with the
/// generic decoder, and asserts the round trip reproduces the exact original coefficient buffer.
/// This alone is the baseline correctness check every SIMD tier below is compared against.
#[test]
fn generic_encode_decode_round_trip() {
    assert!(initialize_block_encoder_tables());
    let mut failures = Vec::new();
    for (case, pattern_name) in all_cases() {
        let mut buf = build_buffer(case);
        let original = logical_region(&buf, case);
        unsafe {
            let (mut bytes, len1) = run_encoder(ojph_encode_codeblock32, &mut buf, case);
            let mut decoded = run_decoder(ojph_decode_codeblock32, &mut bytes, len1, case);
            mask_to_shift(&mut decoded, case);
            if decoded != original {
                failures.push(format!(
                    "{}: generic round-trip mismatch",
                    describe(case, pattern_name)
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Every SIMD encoder tier must produce a byte-identical coded bitstream to the generic (scalar)
/// encoder for the same input. This is the direct correctness gate for porting the 0.31.0 AVX2
/// MEL/VLC/UVLC rewrite: the new kernel must keep passing this test.
#[test]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn simd_encoders_byte_identical_to_generic() {
    assert!(initialize_block_encoder_tables());
    let avx2_available = std::arch::is_x86_feature_detected!("avx2");
    if avx2_available {
        assert!(initialize_block_encoder_tables_avx2());
    }
    #[cfg(target_arch = "x86_64")]
    let avx512_available = std::arch::is_x86_feature_detected!("avx512f")
        && std::arch::is_x86_feature_detected!("avx512cd");
    #[cfg(target_arch = "x86_64")]
    if avx512_available {
        assert!(initialize_block_encoder_tables_avx512());
    }

    let mut failures = Vec::new();
    for (case, pattern_name) in all_cases() {
        let mut buf_gen = build_buffer(case);
        let (bytes_gen, len_gen) =
            unsafe { run_encoder(ojph_encode_codeblock32, &mut buf_gen, case) };

        if avx2_available {
            let mut buf_avx2 = build_buffer(case);
            let (bytes_avx2, len_avx2) =
                unsafe { run_encoder(ojph_encode_codeblock_avx2, &mut buf_avx2, case) };
            if len_gen != len_avx2 || bytes_gen != bytes_avx2 {
                failures.push(format!(
                    "{}: avx2 encoder diverges from generic (len {len_gen} vs {len_avx2})",
                    describe(case, pattern_name)
                ));
            }
        }

        #[cfg(target_arch = "x86_64")]
        if avx512_available {
            let mut buf_avx512 = build_buffer(case);
            let (bytes_avx512, len_avx512) =
                unsafe { run_encoder(ojph_encode_codeblock_avx512, &mut buf_avx512, case) };
            if len_gen != len_avx512 || bytes_gen != bytes_avx512 {
                failures.push(format!(
                    "{}: avx512 encoder diverges from generic (len {len_gen} vs {len_avx512})",
                    describe(case, pattern_name)
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Every SIMD decoder tier must decode the generic encoder's coded bytes back to the exact
/// original coefficient buffer (cross-tier decode agreement, the counterpart of the encode check
/// above — required before porting the 0.31.0 AVX2/SSSE3 decoder rewrites).
#[test]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn simd_decoders_agree_with_generic() {
    assert!(initialize_block_encoder_tables());
    let avx2_available = std::arch::is_x86_feature_detected!("avx2");
    let ssse3_available = std::arch::is_x86_feature_detected!("ssse3");

    let mut failures = Vec::new();
    for (case, pattern_name) in all_cases() {
        let mut buf = build_buffer(case);
        let original = logical_region(&buf, case);
        let (mut bytes, len1) = unsafe { run_encoder(ojph_encode_codeblock32, &mut buf, case) };

        if ssse3_available {
            let mut bytes_copy = bytes.clone();
            let mut decoded =
                unsafe { run_decoder(ojph_decode_codeblock_ssse3, &mut bytes_copy, len1, case) };
            mask_to_shift(&mut decoded, case);
            if decoded != original {
                failures.push(format!(
                    "{}: ssse3 decoder diverges from original",
                    describe(case, pattern_name)
                ));
            }
        }
        if avx2_available {
            let mut decoded =
                unsafe { run_decoder(ojph_decode_codeblock_avx2, &mut bytes, len1, case) };
            mask_to_shift(&mut decoded, case);
            if decoded != original {
                failures.push(format!(
                    "{}: avx2 decoder diverges from original",
                    describe(case, pattern_name)
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Differential fuzz gate for the SigProp (SPP) and MagRef (MRP) passes' AVX2 decode
/// (`decode_spp_avx2`/`decode_mrp_avx2`, Round 10 step 2: ported to the new `destuff_frwd`/
/// `dfetch64` bitstream reader). Unlike every other test in this file, this one cannot start
/// from a real encoded codeblock: **no encoder available to this repo (jph-rs's own, or the real
/// C++ OpenJPH 0.31.0 oracle) ever emits `num_passes > 1`** — `ojph_block_encoder.cpp`/`.rs`
/// hard-`debug_assert!(num_passes == 1)` and explicitly mark `num_passes` unused; the SPP/MRP
/// decode path exists only to read SPP/MRP-bearing codeblocks produced by OTHER encoders (Kakadu
/// is the one named in `tests/spp_probe.rs`, which is `MLDIR`-gated on Kakadu-generated fixtures
/// not available in this environment). So this test cannot compare against a "real" bitstream —
/// instead it exploits the fact that SPP/MRP decode's job is purely mechanical (destuff a byte
/// window, then walk the codeblock's `sigma` significance pattern extracting bits at computed
/// positions): a real cleanup pass (from the generic encoder) establishes a real, valid `sigma`,
/// and a deterministic-but-arbitrary byte sequence stands in for the SPP+MRP segment (its content
/// doesn't need to be "meaningful" HT data, since no encoder produces any — it only needs to
/// exercise the code fairly, which arbitrary bytes do). If the AVX2 (destuff-based) and generic
/// scalar (unchanged, `rev_struct`-based) decoders read the SAME arbitrary bytes into the SAME
/// significance pattern and produce byte-identical `decoded_data`, that is strong evidence the
/// destuff-based rewrite computes the same result as the pre-rewrite reader — the same
/// generic-vs-avx2 methodology every other test in this file uses, just with a synthesized
/// SPP/MRP segment in place of a real encoder's output (which does not exist for this pass).
#[test]
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn spp_mrp_avx2_agrees_with_generic() {
    assert!(initialize_block_encoder_tables());
    let avx2_available = std::arch::is_x86_feature_detected!("avx2");
    if !avx2_available {
        return;
    }

    const GEOMETRIES2: &[(Ui32, Ui32, Ui32, Ui32)] = &[
        (8, 8, 8, 8),
        (16, 16, 16, 16),
        (32, 32, 32, 32),
        (64, 64, 64, 64),
        (32, 64, 32, 64),
        (64, 64, 37, 23),
        (64, 64, 61, 64),
        (8, 8, 5, 7),
    ];
    // Excludes k_max=30: missing_msbs would be 29, which the decoder's own hardening forces
    // num_passes back down to 1 (a real, correct guard — not something this test should route
    // around), defeating the point of this test.
    const K_MAX_VALUES2: &[Ui32] = &[1, 2, 5, 13, 24];
    const LEN2_VALUES: &[Ui32] = &[8, 32, 96];

    let mut failures = Vec::new();
    let mut seed = 0x5EED_1234u32;
    for &(nw, nh, w, h) in GEOMETRIES2 {
        for &k_max in K_MAX_VALUES2 {
            for &len2 in LEN2_VALUES {
                for num_passes in [2u32, 3u32] {
                    let case = Case {
                        nominal_w: nw,
                        nominal_h: nh,
                        width: w,
                        height: h,
                        k_max,
                        pattern: Pattern::Random,
                        seed,
                    };
                    seed = seed.wrapping_mul(2654435761).wrapping_add(1);

                    let mut buf = build_buffer(case);
                    let (cleanup_bytes, len1) =
                        unsafe { run_encoder(ojph_encode_codeblock32, &mut buf, case) };

                    // Arbitrary (not "meaningful" HT data — see the test's doc comment)
                    // deterministic bytes standing in for the SPP+MRP segment.
                    let mut rng = Xorshift32::new(seed);
                    seed = seed.wrapping_mul(2654435761).wrapping_add(1);
                    let spp_mrp_bytes: Vec<u8> =
                        (0..len2).map(|_| (rng.next_u32() & 0xFF) as u8).collect();

                    let mut coded_data = cleanup_bytes.clone();
                    coded_data.extend_from_slice(&spp_mrp_bytes);

                    let mut coded_generic = coded_data.clone();
                    let mut decoded_generic = vec![0u32; case.buf_len()];
                    let ok_generic = unsafe {
                        ojph_decode_codeblock32(
                            coded_generic.as_mut_ptr(),
                            decoded_generic.as_mut_ptr(),
                            case.k_max - 1,
                            num_passes,
                            len1,
                            len2,
                            case.width,
                            case.height,
                            case.stride(),
                            false,
                        )
                    };

                    let mut coded_avx2 = coded_data.clone();
                    let mut decoded_avx2 = vec![0u32; case.buf_len()];
                    let ok_avx2 = unsafe {
                        ojph_decode_codeblock_avx2(
                            coded_avx2.as_mut_ptr(),
                            decoded_avx2.as_mut_ptr(),
                            case.k_max - 1,
                            num_passes,
                            len1,
                            len2,
                            case.width,
                            case.height,
                            case.stride(),
                            false,
                        )
                    };

                    let desc = format!(
                        "{} num_passes={num_passes} len2={len2}",
                        describe(case, "random")
                    );
                    if ok_generic != ok_avx2 {
                        failures.push(format!(
                            "{desc}: generic ok={ok_generic} vs avx2 ok={ok_avx2}"
                        ));
                        continue;
                    }
                    if ok_generic && decoded_generic != decoded_avx2 {
                        failures.push(format!("{desc}: decoded_data diverges"));
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
