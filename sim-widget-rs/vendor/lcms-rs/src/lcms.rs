//! # lcms-rs
//!
//! A pure-Rust port of Little-CMS (lcms2), the ICC color-management engine.
//!
//! The port targets full 1:1 functional parity with the C reference under
//! Little-CMS 2.19 (`vfx.ref/Little-CMS`), with **no FFI** and minimum `unsafe`. See `PORTING_PLAN.md`
//! for the phased roadmap; this crate has completed **M0 (foundation)** and is
//! building out **M1 (color science)**.
//!
//! ## Module map (mirrors the lcms2 subsystems)
//! - [`types`] — foundational numeric/color/signature types and the pixel-format
//!   bitfield (the crate's public contract).
//! - [`error`] — [`Error`]/[`Result`], mirroring the `cmsERROR_*` codes.
//! - [`math`] — 3×3 matrix / 3-vector algebra, half-float tables, interpolation.
//! - [`io`] — endian-aware ICC readers/writers.
//! - [`color`] — white point, PCS conversions, tone curves, CIECAM02.
//! - [`named`] — MLU, named color lists, dictionaries.

/// Maximum number of channels a colour space may have (`cmsMAXCHANNELS`).
pub const MAX_CHANNELS: usize = 16;

#[cfg(feature = "cgats")]
pub mod cgats;
pub mod cmm;
pub mod color;
pub mod context;
pub mod detect;
pub mod error;
pub mod hash;
pub mod io;
pub mod math;
pub mod named;
pub mod pipeline;
pub mod plugin;
#[cfg(feature = "postscript")]
pub mod postscript;
pub mod profile;
pub mod tags;
pub mod types;
pub mod virt;

pub use context::Context;
pub use error::{Error, ErrorCode, Result};

/// Everything a typical consumer touches, in one import.
///
/// The module tree mirrors the lcms2 subsystems, which is right for maintaining the port but
/// puts the common types behind deep paths - and lands two closely related constant sets in
/// unrelated places (`colorspace` under [`types::sig`], `pt` under [`color::pcs`]). This module
/// is purely additive: it re-exports, it moves nothing.
///
/// ```
/// use lcms_rs::prelude::*;
///
/// let ctx = Context::new();
/// let mut src = create_srgb_profile().unwrap();
/// let mut dst = create_srgb_profile().unwrap();
/// let xf = create_transform(
///     &ctx, &mut src, formats::RGB_FLT, &mut dst, formats::RGB_FLT,
///     INTENT_PERCEPTUAL, 0,
/// ).unwrap();
/// let mut out = [0u8; 12];
/// xf.do_transform(&[0u8; 12], &mut out, 1);
/// ```
pub mod prelude {
    pub use crate::cmm::transform::{
        create_extended_transform, create_multiprofile_transform, create_proofing_transform,
        create_transform, Transform,
    };
    pub use crate::color::gamma::ToneCurve;
    pub use crate::color::pcs::pt;
    pub use crate::context::Context;
    pub use crate::error::{Error, ErrorCode, Result};
    pub use crate::pipeline::{Pipeline, Stage, StageLoc};
    pub use crate::profile::read_hl::{
        INTENT_ABSOLUTE_COLORIMETRIC, INTENT_PERCEPTUAL, INTENT_RELATIVE_COLORIMETRIC,
        INTENT_SATURATION,
    };
    pub use crate::profile::Profile;
    pub use crate::types::color::{CIELab, CIExyY, CIExyYTriple, CIEXYZ};
    pub use crate::types::format::{formats, FormatBuilder, PixelFormat};
    pub use crate::types::sig::{colorspace, Signature};
    pub use crate::virt::{
        create_gray_profile, create_lab4_profile, create_null_profile, create_rgb_profile,
        create_srgb_profile, create_xyz_profile,
    };
    pub use crate::MAX_CHANNELS;
}

/// Open one of the real ICC profiles bundled under `tests/fixtures/icc/` (copied from the
/// Little-CMS testbed, see the README there). The one loader every unit test shares.
#[cfg(test)]
pub(crate) fn test_profile(name: &str) -> profile::Profile {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/icc").join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    profile::Profile::open_from_mem(context::Context::new(), &bytes).expect("open profile")
}
