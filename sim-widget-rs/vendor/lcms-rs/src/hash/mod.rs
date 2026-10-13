//! Message-digest hashing — port of `src/cmsmd5.c`.
//!
//! lcms2 hashes a serialised ICC profile with MD5 to derive its `ProfileID`
//! (ICC spec 4.4, section 7.2.18). This subsystem provides the reusable MD5
//! primitive ([`md5`]); the profile-level `cmsMD5computeID` wrapper is implemented
//! in [`crate::profile`] (`Profile::compute_id`), which zeros the `flags`,
//! `RenderingIntent` and `ProfileID` header fields, serialises the profile, feeds
//! the bytes through [`md5`], and stores the digest.

pub mod md5;

pub use md5::{md5, Md5};
