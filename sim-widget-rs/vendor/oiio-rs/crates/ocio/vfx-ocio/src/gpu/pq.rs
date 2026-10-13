//! PQ (SMPTE ST 2084) shader emission, once for every PQ op.
//!
//! Two op families use PQ: the `BuiltinTransfer` style `TransferStyle::Pq` (absolute cd/m2, the
//! ST 2084 domain, [`super::transfer`]) and OCIO's `LIN_TO_PQ` / `PQ_TO_LIN` FixedFunctions
//! (1.0 = 100 cd/m2, mirrored, unclamped, [`super::ff_curves`]). They differ only in the domain
//! wrapper around the same two rationals, so the rationals are written here once, the way the CPU
//! owner `vfx_transfer::pq` writes them once (`oetf_ratio` / `eotf_ratio`). The constants are
//! `vfx_transfer::pq`'s, never retyped.

use super::dialect::{Dialect, n};
use vfx_transfer::pq;

/// Declares `out` = the PQ encode rational of `y` (luminance as a fraction of 10000 cd/m2,
/// `y >= 0`, no clamp) — `vfx_transfer::pq::oetf_ratio`.
pub(super) fn encode(d: &Dialect, y: &str, out: &str) -> Vec<String> {
    vec![
        d.decl("yp", &format!("pow({y}, {})", d.k(pq::M1 as f64))),
        d.decl(
            out,
            &format!(
                "pow(({} + {} * yp) / (1.0 + {} * yp), {})",
                n(pq::C1 as f64),
                n(pq::C2 as f64),
                n(pq::C3 as f64),
                d.k(pq::M2 as f64)
            ),
        ),
    ]
}

/// Declares `out` = luminance as a fraction of 10000 cd/m2 from the signal `v` (`v >= 0`, no
/// clamp) — `vfx_transfer::pq::eotf_ratio`, the inverse of [`encode`].
pub(super) fn decode(d: &Dialect, v: &str, out: &str) -> Vec<String> {
    vec![
        d.decl("vp", &format!("pow({v}, {})", d.k(1.0 / pq::M2 as f64))),
        d.decl(
            out,
            &format!(
                "pow(max(vp - {}, {}) / ({} - {} * vp), {})",
                n(pq::C1 as f64),
                d.k(0.0),
                n(pq::C2 as f64),
                n(pq::C3 as f64),
                d.k(1.0 / pq::M1 as f64)
            ),
        ),
    ]
}
