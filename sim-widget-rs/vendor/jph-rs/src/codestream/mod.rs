//! Ports of files from `https://github.com/aous72/OpenJPH/tree/v0.26.3/src/core/codestream`.

pub mod ojph_bitbuffer_read;
pub mod ojph_bitbuffer_write;
pub mod ojph_codeblock;
pub mod ojph_codeblock_fun;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod ojph_codestream_avx;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod ojph_codestream_avx2;
pub mod ojph_codestream_gen;
pub mod ojph_codestream_local;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod ojph_codestream_sse;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod ojph_codestream_sse2;
#[cfg(target_arch = "wasm32")]
pub mod ojph_codestream_wasm;
pub mod ojph_params_local;
pub mod ojph_packet_ml;
pub mod ojph_precinct;
pub mod ojph_resolution;
pub mod ojph_subband;
pub mod ojph_tile;
pub mod ojph_tile_comp;
