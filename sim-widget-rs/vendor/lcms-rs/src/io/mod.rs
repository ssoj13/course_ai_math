//! Byte-level I/O for ICC data.
//!
//! The endian-aware primitives ([`endian`]) that every tag serializer is built on, plus
//! the `cmsIOHANDLER` abstraction ([`handler`]): in-memory / file / byte-counting backends
//! and the two-pass profile-save size calculation. The primitives operate directly on any
//! [`std::io::Read`]/[`std::io::Write`], which those backends implement.

pub mod endian;
pub mod handler;

pub use handler::{FileIo, IoHandler, MemIo, NullIo};
