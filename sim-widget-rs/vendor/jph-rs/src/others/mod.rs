//! Ports of files from `https://github.com/aous72/OpenJPH/tree/v0.26.3/src/core/others`.

// Not an OpenJPH port: a debug-only oracle tap for the GPU port (`jph-wgpu-rs`).
// Off-by-default `debug-extract` feature; see the module docs for the rationale.
#[cfg(feature = "debug-extract")]
pub mod debug_extract;
