//! Named-data objects — pure-Rust port of `cmsnamed.c`.
//!
//! This module groups the three loosely-related "named" subsystems lcms2 keeps together
//! in `cmsnamed.c`:
//!
//! * [`mlu`] — multi-localized unicode ([`Mlu`]): localized string tables.
//! * [`namedcolor`] — named color lists ([`NamedColorList`]): spot-color palettes.
//! * [`dict`] — dictionaries ([`Dict`]) and profile sequence descriptions ([`Seq`]).
//!
//! See each submodule for how the C sub-allocator / byte-`MemPool` / linked-list details
//! are replaced by owned Rust containers.

pub mod dict;
pub mod mlu;
pub mod namedcolor;

pub use dict::{Dict, DictEntry, ProfileSequenceDesc, Seq};
pub use mlu::{Mlu, MluEntry};
pub use namedcolor::{NamedColor, NamedColorInfo, NamedColorList};
