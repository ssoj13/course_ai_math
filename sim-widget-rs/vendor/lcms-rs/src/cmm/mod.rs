//! The color-management engine (CMM) — Phase 4 (M4).
//!
//! Pixel formatters (`cmspack`), extra-channel/alpha handling ([`alpha`], `cmsalpha`),
//! pipeline optimization (`cmsopt`), rendering intents (`cmscnvrt`) and the transform
//! itself (`cmsxform`) build on the pipeline, profile and color-science layers below.

pub mod alpha;
pub mod blackpoint;
pub mod gamut;
pub mod intents;
pub mod optimize;
pub mod pack;
pub mod transform;

/// Per-buffer byte strides (`cmsStride`), threaded through line/stride transforms and the
/// extra-channel copy. `bytes_per_plane_*` are only used for planar formats.
#[derive(Debug, Clone, Copy, Default)]
pub struct Stride {
    /// Byte distance between consecutive scan lines of the input buffer.
    pub bytes_per_line_in: usize,
    /// Byte distance between consecutive scan lines of the output buffer.
    pub bytes_per_line_out: usize,
    /// Byte distance between planes of a planar input buffer.
    pub bytes_per_plane_in: usize,
    /// Byte distance between planes of a planar output buffer.
    pub bytes_per_plane_out: usize,
}
