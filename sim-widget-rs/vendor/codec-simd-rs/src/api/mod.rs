//! Safe public API wrappers, grouped by primitive family.
//!
//! Each submodule holds the slice-based, length-asserted `pub fn` wrappers
//! for one family; every wrapper validates its arguments then calls the
//! dispatched backend pointer.  All names are re-exported at the crate root
//! (`codec_simd::<fn>`) via `pub use api::*;` in `lib.rs`, so the family
//! split is an internal organisation detail invisible to consumers.

mod arith;
mod colour;
mod convert;
mod cost;
mod dct8;
mod deblock;
mod fdct8;
mod intra;
mod mc;
mod sao;
mod tables;
mod transform;

pub use arith::*;
pub use colour::*;
pub use convert::*;
pub use cost::*;
pub use dct8::*;
pub use deblock::*;
pub use fdct8::*;
pub use intra::*;
pub use mc::*;
pub use sao::*;
pub use tables::*;
pub use transform::*;
