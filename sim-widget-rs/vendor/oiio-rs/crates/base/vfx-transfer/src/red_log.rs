//! RED camera log transfer functions.
//!
//! Includes REDLogFilm and REDLog3G10 encodings used by RED cameras.
//!
//! # REDLogFilm
//!
//! Older RED log encoding with Cineon-style curve (LogAffine transform).
//! Does NOT fit `LogCameraTransform` because it has no linear extension —
//! it applies a pure log curve across the entire range (clamped at zero).
//!
//! # REDLog3G10
//!
//! Modern RED log encoding with extended dynamic range.
//! Uses `LogCameraTransform` (base=10) with a negative linear break point.
//!
//! # Reference
//!
//! Source: OpenColorIO/src/OpenColorIO/transforms/builtins/RedCameras.cpp
//! <https://github.com/AcademySoftwareFoundation/OpenColorIO/blob/main/src/OpenColorIO/transforms/builtins/RedCameras.cpp>

use crate::log_camera::LogCameraTransform;
use std::sync::OnceLock;

// =============================================================================
// REDLogFilm — LogAffine (no linear extension below break)
//
// WHY it does NOT use LogCameraTransform:
//   REDLogFilm is a pure LogAffine transform — it applies
//   `y = logSideSlope * log10(linSideSlope * x + linSideOffset) + logSideOffset`
//   across the entire input range. There is no linear extension below a break;
//   instead, inputs that would cause log(<=0) are clamped to 0.0.
//   LogCameraTransform always has a linear extension, which would change the
//   behaviour for small/negative inputs.
// =============================================================================

const REDLOGFILM_REF_WHITE: f64 = 685.0 / 1023.0;
const REDLOGFILM_REF_BLACK: f64 = 95.0 / 1023.0;
const REDLOGFILM_GAMMA: f64 = 0.6;
const REDLOGFILM_RANGE: f64 = 0.002 * 1023.0;
const REDLOGFILM_HIGHLIGHT: f64 = 1.0;
const REDLOGFILM_SHADOW: f64 = 0.0;

/// Precomputed LogAffine parameters for REDLogFilm.
#[derive(Clone, Copy)]
struct RedLogFilmParams {
    log_side_slope: f64,
    log_side_offset: f64,
    lin_side_slope: f64,
    lin_side_offset: f64,
}

static REDLOGFILM_PARAMS: OnceLock<RedLogFilmParams> = OnceLock::new();

#[inline]
fn redlogfilm_params() -> RedLogFilmParams {
    *REDLOGFILM_PARAMS.get_or_init(|| {
        let multi_factor = REDLOGFILM_RANGE / REDLOGFILM_GAMMA;
        let gain = (REDLOGFILM_HIGHLIGHT - REDLOGFILM_SHADOW)
            / (1.0 - 10.0_f64.powf(multi_factor * (REDLOGFILM_REF_BLACK - REDLOGFILM_REF_WHITE)));
        let offset = gain - (REDLOGFILM_HIGHLIGHT - REDLOGFILM_SHADOW);
        RedLogFilmParams {
            log_side_slope: 1.0 / multi_factor,
            log_side_offset: REDLOGFILM_REF_WHITE,
            lin_side_slope: 1.0 / gain,
            lin_side_offset: (offset - REDLOGFILM_SHADOW) / gain,
        }
    })
}

/// REDLogFilm encode: Linear to REDLogFilm.
///
/// # Example
///
/// ```rust
/// use vfx_transfer::red_log::redlogfilm_encode;
///
/// let log = redlogfilm_encode(0.18);
/// assert!((log - 0.5).abs() < 0.1);
/// ```
#[inline]
pub fn redlogfilm_encode(linear: f32) -> f32 {
    let p = redlogfilm_params();
    let lin_val = p.lin_side_slope * (linear as f64) + p.lin_side_offset;
    // COLOR-08: returning 0.0 at the pole was wrong — 0.0 is a *mid-scale* code value here, so a
    // deep-negative input silently produced something brighter than legitimate blacks. OCIO clamps
    // the log's argument to the smallest positive float instead of special-casing the result
    // (LogOpCPU.cpp:688,707 — `mm_pixel = _mm_max_ps(mm_pixel, mm_minValue)` with
    // `minValue = std::numeric_limits<float>::min()`), which yields a large negative code value.
    // Clamping in f32's range, not f64's, keeps parity with the reference's f32 pipeline.
    let lin_val = lin_val.max(f32::MIN_POSITIVE as f64);
    (p.log_side_slope * lin_val.log10() + p.log_side_offset) as f32
}

/// REDLogFilm decode: REDLogFilm to linear.
///
/// # Example
///
/// ```rust
/// use vfx_transfer::red_log::redlogfilm_decode;
///
/// let linear = redlogfilm_decode(0.5);
/// assert!(linear > 0.0 && linear < 1.0);
/// ```
#[inline]
pub fn redlogfilm_decode(log: f32) -> f32 {
    let p = redlogfilm_params();
    let log_val = (log as f64 - p.log_side_offset) / p.log_side_slope;
    let lin_val = 10.0_f64.powf(log_val);
    ((lin_val - p.lin_side_offset) / p.lin_side_slope) as f32
}

// =============================================================================
// REDLog3G10 — LogCameraTransform (base=10, negative break)
// =============================================================================

const LOG3G10_LIN_SIDE_SLOPE: f64 = 155.975327;
const LOG3G10_LIN_SIDE_OFFSET: f64 = 0.01 * LOG3G10_LIN_SIDE_SLOPE + 1.0;
const LOG3G10_LOG_SIDE_SLOPE: f64 = 0.224282;
const LOG3G10_LOG_SIDE_OFFSET: f64 = 0.0;
const LOG3G10_LIN_SIDE_BREAK: f64 = -0.01;

static LOG3G10: OnceLock<LogCameraTransform> = OnceLock::new();

#[inline]
fn log3g10() -> &'static LogCameraTransform {
    LOG3G10.get_or_init(|| {
        LogCameraTransform::build(
            10.0,
            LOG3G10_LOG_SIDE_SLOPE,
            LOG3G10_LOG_SIDE_OFFSET,
            LOG3G10_LIN_SIDE_SLOPE,
            LOG3G10_LIN_SIDE_OFFSET,
            LOG3G10_LIN_SIDE_BREAK,
        )
    })
}

/// REDLog3G10 encode: Linear to REDLog3G10.
///
/// Handles negative values via linear extension.
///
/// # Example
///
/// ```rust
/// use vfx_transfer::red_log::log3g10_encode;
///
/// let log = log3g10_encode(0.18);
/// assert!(log > 0.3 && log < 0.5);
/// ```
#[inline]
pub fn log3g10_encode(linear: f32) -> f32 {
    log3g10().encode_f32(linear)
}

/// REDLog3G10 decode: REDLog3G10 to linear.
///
/// # Example
///
/// ```rust
/// use vfx_transfer::red_log::log3g10_decode;
///
/// let linear = log3g10_decode(0.4);
/// assert!(linear > 0.0 && linear < 1.0);
/// ```
#[inline]
pub fn log3g10_decode(log: f32) -> f32 {
    log3g10().decode_f32(log)
}

/// Applies REDLogFilm encoding to RGB.
#[inline]
pub fn redlogfilm_encode_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [
        redlogfilm_encode(rgb[0]),
        redlogfilm_encode(rgb[1]),
        redlogfilm_encode(rgb[2]),
    ]
}

/// Applies REDLogFilm decoding to RGB.
#[inline]
pub fn redlogfilm_decode_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [
        redlogfilm_decode(rgb[0]),
        redlogfilm_decode(rgb[1]),
        redlogfilm_decode(rgb[2]),
    ]
}

/// Applies REDLog3G10 encoding to RGB.
#[inline]
pub fn log3g10_encode_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [
        log3g10_encode(rgb[0]),
        log3g10_encode(rgb[1]),
        log3g10_encode(rgb[2]),
    ]
}

/// Applies REDLog3G10 decoding to RGB.
#[inline]
pub fn log3g10_decode_rgb(rgb: [f32; 3]) -> [f32; 3] {
    [
        log3g10_decode(rgb[0]),
        log3g10_decode(rgb[1]),
        log3g10_decode(rgb[2]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redlogfilm_roundtrip() {
        let test_values = [0.01, 0.18, 0.5, 1.0, 2.0];
        for &l in &test_values {
            let encoded = redlogfilm_encode(l);
            let decoded = redlogfilm_decode(encoded);
            assert!(
                (l - decoded).abs() < l * 0.01 + 0.001,
                "l={}, encoded={}, decoded={}",
                l,
                encoded,
                decoded
            );
        }
    }

    #[test]
    fn test_log3g10_roundtrip() {
        let test_values = [-0.005, 0.0, 0.01, 0.18, 0.5, 1.0, 2.0];
        for &l in &test_values {
            let encoded = log3g10_encode(l);
            let decoded = log3g10_decode(encoded);
            assert!(
                (l - decoded).abs() < l.abs() * 0.01 + 0.001,
                "l={}, encoded={}, decoded={}",
                l,
                encoded,
                decoded
            );
        }
    }

    #[test]
    fn test_log3g10_negative_handling() {
        // REDLog3G10 has break at -0.01, test linear extension below that
        let neg = log3g10_encode(-0.02);
        let at_break = log3g10_encode(-0.01);
        assert!(neg < at_break, "Below break should give smaller value");

        let decoded = log3g10_decode(neg);
        assert!(
            (decoded - (-0.02)).abs() < 0.001,
            "Negative roundtrip failed: got {}",
            decoded
        );
    }
}
