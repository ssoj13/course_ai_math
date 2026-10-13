//! `codec-simd` — cross-arch SIMD primitives for software video codecs.
//!
//! Public API is safe (slice-based, runtime length asserts).  All `unsafe`
//! lives in backend modules.  See `CAVEMAN.md` for usage and how to add a
//! new primitive.

#![warn(missing_debug_implementations)]
#![allow(
    clippy::missing_safety_doc,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::needless_range_loop,
    clippy::identity_op,
    clippy::needless_return
)]

pub mod arch;
mod dispatch;

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
mod prims_avx2;
#[cfg(target_arch = "aarch64")]
mod prims_neon;
mod prims_scalar;
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
mod prims_sse2;
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
mod prims_sse41;
#[cfg(target_arch = "wasm32")]
mod prims_wasm;

mod api;

pub use dispatch::Dispatch;

// Re-export every safe public wrapper at the crate root so the public path
// stays `codec_simd::<fn>` regardless of the internal per-family split.
pub use api::*;
