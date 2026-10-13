//! Pure Rust one-to-one port of the OpenJPH core.
//!
//! Reference source tree:
//! <https://github.com/aous72/OpenJPH/tree/v0.26.3/src/core>

// One-to-one C++ port: nearly every function is unsafe with raw pointers and SIMD intrinsics.
#![allow(unsafe_op_in_unsafe_fn)]
// jph-rs is a line-by-line 1:1 port of OpenJPH (C++ v0.26.3). It deliberately mirrors the C++
// structure — raw pointers in public fns, C-style index loops, explicit casts, etc. — so the
// idiomatic-Rust clippy lints below do NOT apply: rewriting to be "idiomatic" would break the
// 1:1 fidelity that is this crate's entire purpose. Suppressed crate-wide (not per-site) so
// downstream consumers (exr-core) can run `cargo clippy -- -D warnings` cleanly. This is the
// documented handling for vendored 1:1-port code; tracked in crates/jph-rs/CLAUDE.md.
//
// NOTE: `clippy::incompatible_msrv` is intentionally NOT in this list — it was never a
// port-fidelity issue. It fired 454× only because the workspace `rust-version` was declared
// `1.85` while jph-rs uses std SIMD intrinsics stabilised in `1.89`. The fix was to declare the
// TRUE minimum (`rust-version = "1.89"` in the workspace Cargo.toml), NOT to suppress the lint.
#![allow(clippy::not_unsafe_ptr_arg_deref)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::redundant_static_lifetimes)]
#![allow(clippy::identity_op)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::unnecessary_cast)]
#![allow(clippy::manual_div_ceil)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::excessive_precision)]
#![allow(clippy::needless_return)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::manual_swap)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::if_same_then_else)]

pub mod codestream;
pub mod coding;
pub mod openjph;
pub mod mem;
pub mod others;
pub mod parse;
pub mod transform;

pub use codestream::ojph_params_local::Jp2kMarker;
pub use openjph::ojph_base::{Point, Rect, Size};
pub use openjph::ojph_defs::{NumFracBits, ojph_div_ceil, ojph_max, ojph_min};
pub use openjph::ojph_version::{
    OPENJPH_VERSION_MAJOR, OPENJPH_VERSION_MINOR, OPENJPH_VERSION_PATCH,
};

// Re-exports for HTJ2K integration
pub use codestream::ojph_params_local::{ParamCod, ParamNlt, ParamSiz};
pub use openjph::ojph_codestream::Codestream;
pub use openjph::ojph_defs::{Si32, Ui8, Ui32};
pub use openjph::ojph_file::{InfileBase, MemInfile, MemOutfile, OutfileBase, VecOutfile};
pub use openjph::ojph_mem::LineBuf;

// High-level in-memory codec API (Layer 2) — one-call encode/decode for downstream consumers
// (heif-rs, exr-core) that should never touch OpenJPH's per-line raw-pointer protocol.
pub use mem::{
    DecodeError, DecodeParams, EncodeError, EncodeOptions, EncodeParams, Image, Plane, Progression, Quant,
    Rounding, decode, encode, try_decode, try_decode_with, try_encode, try_encode_with,
};

// Parse-only per-code-block front-end (Layer 2) — enumerates each block's decode plan WITHOUT
// entropy decode or DWT, for the GPU port (jph-wgpu-rs). `parse::Geom` is re-exported as
// `ParseGeom` to avoid clashing with the `debug-extract` oracle's `Geom`.
pub use parse::{CodeblockPlan, CodecConfig, CompInfo, ParsedImage, enumerate};
pub use parse::Geom as ParseGeom;

// Debug-only oracle tap for the GPU port (jph-wgpu-rs): per-code-block (coded bytes ->
// decoded coefficients) capture for bit-exact kernel validation. Off-by-default feature.
#[cfg(feature = "debug-extract")]
pub use others::debug_extract;

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub use coding::ojph_block_encoder_avx2::{
    Avx2LzcntMode, ENV_AVX2_LZCNT, avx2_lzcnt_mode_from_env, get_avx2_lzcnt_mode,
    parse_avx2_lzcnt_mode, set_avx2_lzcnt_mode,
};
