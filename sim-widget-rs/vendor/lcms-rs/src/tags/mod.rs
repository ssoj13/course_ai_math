//! ICC tag-type serialization — a bit-exact, pure-Rust port of `cmstypes.c`.
//!
//! This module implements every built-in ICC tag type and every built-in tag descriptor,
//! the round-trip layer profile I/O sits on top of. Its three faces:
//!
//! * [`value::TagValue`] — the decoded payload of a tag (lcms's opaque `void*`), one variant
//!   per decoded object.
//! * [`handler`] — the tag-type handlers ([`TypeHandler`], keyed via [`get_handler`]): the 34
//!   distinct `SupportedTagTypes` handlers plus the 3 MPE element handlers used inside `mpet`.
//! * [`descriptor`] — the [`TagDescriptor`] table ([`get_descriptor`], 66 tags) with the
//!   `DecideType` callbacks that pick which supported type a tag is written as.
//!
//! # Fidelity
//! All integers are big-endian (via [`crate::io::endian`]); every position-table type
//! reproduces the ICC 4.3 offset/size directory relative to the 8-byte type base; the two
//! "broken" type aliases (`CorbisBrokenXYZ`, `MonacoBrokenCurve`) are read through the XYZ /
//! curve handlers. The per-type `#[cfg(test)]` round-trips are the bit-exact completeness gate
//! (write → read → write reproduces identical bytes).

pub mod descriptor;
pub mod handler;
pub mod types;
pub mod value;

pub use descriptor::{get_descriptor, get_descriptor_thr, TagDescriptor};
pub use handler::{get_handler, get_handler_thr, TypeHandler};
pub use value::{
    Measurement, Mhc2, Screening, ScreeningChannel, TagValue, UcrBg, VideoSignal, ViewingConditions,
};

/// Shared bit-exactness harness for the per-type round-trip tests.
///
/// The core invariant is *serialization idempotency*: `write(v) → b1`, `read(b1) → v'`,
/// `write(v') → b2`, and `b1 == b2`. The 8-byte type base is included so position-table
/// offsets (computed relative to it) are exercised on the real framing.
#[cfg(test)]
pub(crate) mod testutil {
    use std::io::Cursor;

    use super::handler::{read_type_base, write_type_base, TypeHandler};
    use super::value::TagValue;

    /// Frame a value as `[type base | body]` and return the bytes (`icc_version` reaches the
    /// pseq/psid writers via `SaveDescription`).
    pub(crate) fn round_trip_bytes(h: TypeHandler, v: &TagValue, icc_version: f64) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        write_type_base(&mut buf, h.signature()).expect("write type base");
        h.write(&mut buf, v, icc_version).expect("handler write");
        buf.into_inner()
    }

    /// Read a value back from framed bytes (`SizeOfTag` is the body size after the type base).
    pub(crate) fn read_back(h: TypeHandler, bytes: &[u8]) -> TagValue {
        let mut cur = Cursor::new(bytes.to_vec());
        let _sig = read_type_base(&mut cur).expect("read type base");
        let body = bytes.len() as u32 - 8;
        h.read(&mut cur, body).expect("handler read")
    }

    /// Assert serialization is idempotent for `h` at the given ICC version.
    pub(crate) fn round_trip_stable_versioned(h: TypeHandler, v: &TagValue, icc_version: f64) {
        let b1 = round_trip_bytes(h, v, icc_version);
        let v2 = read_back(h, &b1);
        let b2 = round_trip_bytes(h, &v2, icc_version);
        assert_eq!(b1, b2, "serialization not idempotent for {h:?}");
    }

    /// [`round_trip_stable_versioned`] at ICC v4.3 (the common case).
    pub(crate) fn round_trip_stable(h: TypeHandler, v: &TagValue) {
        round_trip_stable_versioned(h, v, 4.3);
    }
}
