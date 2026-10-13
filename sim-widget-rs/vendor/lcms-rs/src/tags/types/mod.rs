//! Per-family tag-type (de)serializers — the bodies of the 34 built-in `cmsTagTypeHandler`s
//! and the 3 MPE element handlers. Each module exposes `pub(crate)` `read_*`/`write_*`
//! functions that [`super::handler::TypeHandler`] fans out to.

pub(crate) mod curve;
pub(crate) mod dict;
pub(crate) mod lut;
pub(crate) mod misc;
pub(crate) mod mlu;
pub(crate) mod mpe;
pub(crate) mod namedcolor;
pub(crate) mod seq;
pub(crate) mod xyz;
