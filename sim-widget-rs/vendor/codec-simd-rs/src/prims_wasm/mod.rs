//! WebAssembly `simd128` implementations of primitives.

mod arith;
mod colour;
mod convert;
mod cost;
mod mc;

pub(crate) use arith::*;
pub(crate) use colour::*;
pub(crate) use convert::*;
pub(crate) use cost::*;
pub(crate) use mc::*;
