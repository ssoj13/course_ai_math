//! Generic LogCamera transform used by multiple camera log curves.
//!
//! This module implements the standard OCIO `LogCameraTransform` formula:
//!
//! ```text
//! encode (linear -> log):
//!   if linear >= lin_break:
//!     log = log_slope * log_base(lin_slope * linear + lin_offset) + log_offset
//!   else:
//!     log = lin_ext_slope * linear + lin_ext_offset
//!
//! decode (log -> linear):
//!   if log >= log_break:
//!     linear = (base ^ ((log - log_offset) / log_slope) - lin_offset) / lin_slope
//!   else:
//!     linear = (log - lin_ext_offset) / lin_ext_slope
//! ```
//!
//! `lin_ext_slope` and `lin_ext_offset` are computed from C1 continuity at
//! `lin_break` by [`LogCameraTransform::build`], so that both value and derivative are
//! continuous there. Some vendors publish a linear segment that is *not* the tangent of their
//! own log segment; those curves must use [`LogCameraTransform::build_with_lin_ext`] and pass
//! the published pair verbatim (OCIO models the same thing as an explicit `linearSlope` on
//! `LogCameraTransform`).
//!
//! # Files that use this struct
//!
//! | Camera | File | Base | Notes |
//! |--------|------|------|-------|
//! | ARRI LogC3    | log_c.rs            | 10 | EI 800 |
//! | Sony S-Log3   | s_log3.rs           | 10 | code-value scale /1023 |
//! | Panasonic V-Log | v_log.rs          | 10 | lin_slope = 1 |
//! | DJI D-Log     | d_log.rs            | 10 | vendor linear tail, `build_with_lin_ext` |
//! | BMD Film Gen5 | bmd_film.rs         | e  | natural log |
//! | RED Log3G10   | red_log.rs          | 10 | f64, negative break |
//!
//! # Files that do NOT use this struct (special formulas)
//!
//! | Camera | File | Reason |
//! |--------|------|--------|
//! | ARRI LogC4    | log_c4.rs           | Base-2, dynamic linear params, f64 |
//! | Sony S-Log2   | s_log2.rs           | SCALE divider, break at linear=0 |
//! | RED REDLogFilm | red_log.rs (partial)| LogAffine (no linear extension) |
//! | DaVinci Intermediate | davinci_intermediate.rs | Base-2, `(log2(x+A)+B)*C` form |

/// Generic LogCamera transfer function with C1-continuous linear extension below cutoff.
///
/// All fields are `f64` for precision; f32 API wrappers are provided per camera file.
///
/// # Formula
///
/// **Encode** (`linear -> log`):
/// ```text
/// if linear >= lin_break:
///   log = log_slope * ln(lin_slope * linear + lin_offset) / ln(base) + log_offset
/// else:
///   log = lin_ext_slope * linear + lin_ext_offset
/// ```
///
/// **Decode** (`log -> linear`):
/// ```text
/// if log >= log_break:
///   linear = (base^((log - log_offset) / log_slope) - lin_offset) / lin_slope
/// else:
///   linear = (log - lin_ext_offset) / lin_ext_slope
/// ```
#[derive(Clone, Copy, Debug)]
pub struct LogCameraTransform {
    /// Natural log of the base (precomputed: ln(base))
    pub ln_base: f64,
    /// log-side slope: scales the logarithm result
    pub log_slope: f64,
    /// log-side offset: added after log scaling
    pub log_offset: f64,
    /// linear-side slope: scales linear input before log
    pub lin_slope: f64,
    /// linear-side offset: added to scaled linear before log
    pub lin_offset: f64,
    /// break point in linear domain
    pub lin_break: f64,

    // Derived at construction (C1 continuity at lin_break):
    /// break point in log domain (precomputed)
    pub log_break: f64,
    /// linear extension slope (precomputed from derivative at lin_break)
    pub lin_ext_slope: f64,
    /// linear extension offset (precomputed)
    pub lin_ext_offset: f64,
}

impl LogCameraTransform {
    /// Build a fully initialized transform from primary parameters.
    ///
    /// Computes all derived fields (break points, linear extension) for C1 continuity.
    pub fn build(
        base: f64,
        log_slope: f64,
        log_offset: f64,
        lin_slope: f64,
        lin_offset: f64,
        lin_break: f64,
    ) -> Self {
        let ln_base = base.ln();

        // Value of (lin_slope * lin_break + lin_offset) — input to the log
        let lin_at_break = lin_slope * lin_break + lin_offset;

        // Derivative of log segment at lin_break:
        // d/dx [log_slope * ln(lin_slope*x + lin_offset) / ln_base + log_offset]
        //   = log_slope * lin_slope / (ln_base * (lin_slope*x + lin_offset))
        let lin_ext_slope = log_slope * lin_slope / (ln_base * lin_at_break);

        // C0 continuity: lin_ext_offset = log_break - lin_ext_slope * lin_break
        let log_break = log_slope * lin_at_break.ln() / ln_base + log_offset;
        let lin_ext_offset = log_break - lin_ext_slope * lin_break;

        Self::build_with_lin_ext(
            base,
            log_slope,
            log_offset,
            lin_slope,
            lin_offset,
            lin_break,
            lin_ext_slope,
            lin_ext_offset,
        )
    }

    /// Build a transform whose linear extension is given explicitly rather than derived.
    ///
    /// Use this when the vendor publishes a linear segment that is not the tangent of their own
    /// log segment (DJI D-Log is the case in this crate: its published pair meets the log branch
    /// to ~2e-6 in value but its slope 6.025 differs from the log tangent 6.224). Re-deriving
    /// such a tail from C1 continuity silently changes the curve, so the published pair is
    /// carried verbatim instead.
    ///
    /// `log_break` is still derived from the log segment, so `encode`/`decode` agree exactly at
    /// `lin_break` regardless of any C0 gap in the vendor's own constants.
    #[allow(clippy::too_many_arguments)]
    pub fn build_with_lin_ext(
        base: f64,
        log_slope: f64,
        log_offset: f64,
        lin_slope: f64,
        lin_offset: f64,
        lin_break: f64,
        lin_ext_slope: f64,
        lin_ext_offset: f64,
    ) -> Self {
        let ln_base = base.ln();
        let log_break =
            log_slope * (lin_slope * lin_break + lin_offset).ln() / ln_base + log_offset;

        Self {
            ln_base,
            log_slope,
            log_offset,
            lin_slope,
            lin_offset,
            lin_break,
            log_break,
            lin_ext_slope,
            lin_ext_offset,
        }
    }

    /// Encode: linear -> log.
    #[inline]
    pub fn encode(&self, linear: f64) -> f64 {
        if linear >= self.lin_break {
            let x = self.lin_slope * linear + self.lin_offset;
            self.log_slope * x.ln() / self.ln_base + self.log_offset
        } else {
            self.lin_ext_slope * linear + self.lin_ext_offset
        }
    }

    /// Decode: log -> linear.
    #[inline]
    pub fn decode(&self, log: f64) -> f64 {
        if log >= self.log_break {
            let exp = (log - self.log_offset) / self.log_slope;
            // base^exp = e^(exp * ln_base)
            let lin_val = (exp * self.ln_base).exp();
            (lin_val - self.lin_offset) / self.lin_slope
        } else {
            (log - self.lin_ext_offset) / self.lin_ext_slope
        }
    }

    /// Encode f32 input, returning f32.
    #[inline]
    pub fn encode_f32(&self, linear: f32) -> f32 {
        self.encode(linear as f64) as f32
    }

    /// Decode f32 input, returning f32.
    #[inline]
    pub fn decode_f32(&self, log: f32) -> f32 {
        self.decode(log as f64) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify the generic struct matches known log_c constants (EI 800).
    #[test]
    fn test_log_c_equiv() {
        // LogC3 EI 800 parameters: encode = C * log10(A*x + B) + D
        // Rewritten as: log_slope=C, log_offset=D, lin_slope=A, lin_offset=B, base=10
        let t = LogCameraTransform::build(
            10.0, 0.247190, // C (log_slope)
            0.385537, // D (log_offset)
            5.555556, // A (lin_slope)
            0.052272, // B (lin_offset)
            0.010591, // CUT (lin_break)
        );

        // Compare with log_c::encode reference value: 18% gray ~ 0.391
        let enc = t.encode_f32(0.18);
        assert!((enc - 0.391).abs() < 0.001, "18% gray: got {}", enc);

        // Roundtrip
        let dec = t.decode_f32(enc);
        assert!((dec - 0.18).abs() < 1e-5, "roundtrip: got {}", dec);
    }

    /// Verify roundtrip works for values below break (linear extension).
    #[test]
    fn test_linear_extension_roundtrip() {
        let t = LogCameraTransform::build(10.0, 0.247190, 0.385537, 5.555556, 0.052272, 0.010591);

        // Value below CUT -> uses linear extension
        let x = 0.005;
        let enc = t.encode(x);
        let dec = t.decode(enc);
        assert!(
            (dec - x).abs() < 1e-12,
            "linear ext roundtrip: {} -> {}",
            x,
            dec
        );
    }

    /// Verify C0 continuity at the break point.
    #[test]
    fn test_continuity_at_break() {
        let t = LogCameraTransform::build(10.0, 0.247190, 0.385537, 5.555556, 0.052272, 0.010591);

        let eps = 1e-9;
        let below = t.encode(t.lin_break - eps);
        let above = t.encode(t.lin_break + eps);
        let at = t.encode(t.lin_break);

        assert!(
            (at - below).abs() < 1e-6,
            "C0 below break: {} vs {}",
            below,
            at
        );
        assert!(
            (above - at).abs() < 1e-6,
            "C0 above break: {} vs {}",
            at,
            above
        );
    }

    /// Test with BMD Gen5 (base=e) parameters.
    #[test]
    fn test_bmd_gen5_base_e() {
        let t = LogCameraTransform::build(
            std::f64::consts::E,
            0.0869287606549122,  // log_slope
            0.530013339229194,   // log_offset
            1.0,                 // lin_slope
            0.00549407243225781, // lin_offset
            0.005,               // lin_break
        );

        let enc = t.encode_f32(0.18);
        // BMD Gen5: 18% gray should be roughly 0.35..0.55
        assert!(enc > 0.35 && enc < 0.55, "BMD Gen5 18% gray: {}", enc);

        let dec = t.decode_f32(enc);
        assert!((dec - 0.18).abs() < 1e-5, "BMD Gen5 roundtrip: {}", dec);
    }
}
