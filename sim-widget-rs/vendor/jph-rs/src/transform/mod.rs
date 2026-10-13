//! Ports of files from `https://github.com/aous72/OpenJPH/tree/v0.26.3/src/core/transform`.

pub mod ojph_colour;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod ojph_colour_avx;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod ojph_colour_avx2;
pub mod ojph_colour_local;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod ojph_colour_sse;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod ojph_colour_sse2;
#[cfg(target_arch = "wasm32")]
pub mod ojph_colour_wasm;
pub mod ojph_transform;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod ojph_transform_avx;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod ojph_transform_avx2;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod ojph_transform_avx512;
pub mod ojph_transform_local;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod ojph_transform_sse;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod ojph_transform_sse2;
#[cfg(target_arch = "wasm32")]
pub mod ojph_transform_wasm;
