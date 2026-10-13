//! Ports of files from `https://github.com/aous72/OpenJPH/tree/v0.26.3/src/core/coding`.

#[cfg(test)]
mod codeblock_fuzz_tests;
pub mod ojph_block_common;
pub mod ojph_block_decoder;
pub mod ojph_block_decoder32;
pub mod ojph_block_decoder64;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod ojph_block_decoder_avx2;
/// JPEG 2000 Part-1 (legacy, non-HT) EBCOT block decoder: the three bit-plane passes driven by the
/// MQ arithmetic coder, producing the same codeblock buffer layout as the HT decoder.
pub mod ojph_block_decoder_part1;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod ojph_block_decoder_ssse3;
#[cfg(target_arch = "wasm32")]
pub mod ojph_block_decoder_wasm;
pub mod ojph_block_encoder;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod ojph_block_encoder_avx2;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod ojph_block_encoder_avx512;
/// JPEG 2000 Part-1 (legacy, non-HT) MQ arithmetic decoder. Ported from OpenHTJ2K (OpenJPH is
/// HT-only). Foundation for the Part-1 EBCOT block decoder.
pub mod ojph_mq_decoder;
/// Code-block coding-style flags and the BYPASS/RESTART segment model (single source of truth).
pub mod ojph_cmodes;
/// JPEG 2000 Part-1 EBCOT context look-up tables (ZC / SC) and MQ context labels.
pub mod ojph_t1_luts;
pub mod table0;
pub mod table1;
