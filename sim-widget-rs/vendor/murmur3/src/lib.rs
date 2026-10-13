//! MurmurHash3 family - pure `std`, zero dependencies.
//!
//! Modules:
//! - [`x64_128`] - Alembic sample digests (`MurmurHash3_x64_128`)
//! - [`x86_32`] - Cryptomatte IDs (`MurmurHash3_x86_32`)
//!
//! Original algorithm by Austin Appleby.

pub mod x64_128;
pub mod x86_32;
