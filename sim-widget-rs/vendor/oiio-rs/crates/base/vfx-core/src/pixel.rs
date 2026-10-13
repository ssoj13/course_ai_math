//! Pixel types and data formats for image processing.
//!
//! This module provides generic pixel types that track both their color space
//! and underlying data type at compile time.
//!
//! # Types
//!
//! - [`Rgb`] - RGB pixel (3 channels)
//! - [`Rgba`] - RGBA pixel with alpha (4 channels)
//! - [`PixelFormat`] - Trait for pixel data types (u8, u16, f16, f32)
//!
//! # Design
//!
//! Pixels are parameterized by:
//! 1. **Color Space** (`C: ColorSpace`) - compile-time color space tracking
//! 2. **Data Type** (`T: PixelFormat`) - the underlying numeric type
//!
//! ```
//! use vfx_core::prelude::*;
//!
//! // 8-bit sRGB pixel
//! let srgb_pixel: Rgba<Srgb, u8> = Rgba::new(255, 128, 64, 255);
//!
//! // 32-bit float ACEScg pixel
//! let aces_pixel: Rgba<AcesCg, f32> = Rgba::new(0.18, 0.18, 0.18, 1.0);
//! ```
//!
//! # Memory Layout
//!
//! All pixel types use `#[repr(C)]` for predictable memory layout,
//! enabling safe zero-copy operations with GPU buffers and FFI.
//!
//! # Dependencies
//!
//! - `half` crate for `f16` support
//!
//! # Used By
//!
//! - `vfx-core::Image` - image buffers store pixels
//! - `vfx-ops` - pixel-level operations
//! - `vfx-io` - format readers/writers

use crate::colorspace::ColorSpace;
use half::f16;
use std::fmt;
use std::marker::PhantomData;
use std::ops::{Add, Mul, Sub};

// ============================================================================
// Rec.709 Luminance Constants
// ============================================================================

/// Rec.709 luminance coefficient for red channel.
///
/// Used in the standard luminance formula: `Y = 0.2126*R + 0.7152*G + 0.0722*B`
pub const REC709_LUMA_R: f32 = 0.2126;

/// Rec.709 luminance coefficient for green channel.
pub const REC709_LUMA_G: f32 = 0.7152;

/// Rec.709 luminance coefficient for blue channel.
pub const REC709_LUMA_B: f32 = 0.0722;

/// Rec.709 luminance coefficients as an array [R, G, B].
///
/// # Example
/// ```
/// use vfx_core::pixel::REC709_LUMA;
/// let rgb = [0.5, 0.3, 0.2];
/// let luma = rgb[0] * REC709_LUMA[0] + rgb[1] * REC709_LUMA[1] + rgb[2] * REC709_LUMA[2];
/// ```
pub const REC709_LUMA: [f32; 3] = [REC709_LUMA_R, REC709_LUMA_G, REC709_LUMA_B];

/// Calculate Rec.709 luminance from RGB values.
///
/// This is the standard luminance calculation for sRGB/Rec.709 primaries:
/// `Y = 0.2126*R + 0.7152*G + 0.0722*B`
///
/// # Arguments
/// * `rgb` - RGB values as [R, G, B] array
///
/// # Returns
/// The luminance value
///
/// # Example
/// ```
/// use vfx_core::pixel::luminance_rec709;
/// let luma = luminance_rec709([0.5, 0.3, 0.2]);
/// // 0.5 * 0.2126 + 0.3 * 0.7152 + 0.2 * 0.0722 = 0.3353
/// assert!((luma - 0.3353).abs() < 0.0001);
/// ```
#[inline]
pub fn luminance_rec709(rgb: [f32; 3]) -> f32 {
    rgb[0] * REC709_LUMA_R + rgb[1] * REC709_LUMA_G + rgb[2] * REC709_LUMA_B
}

/// Trait for pixel data types.
///
/// Implemented for standard numeric types used in image processing:
/// - `u8` - 8-bit unsigned (0-255)
/// - `u16` - 16-bit unsigned (0-65535)
/// - `f16` - 16-bit float (half precision)
/// - `f32` - 32-bit float (single precision)
///
/// # Required Methods
///
/// - [`to_f32`](PixelFormat::to_f32) - Convert to normalized f32 [0.0, 1.0] for integers
/// - [`from_f32`](PixelFormat::from_f32) - Convert from normalized f32
///
/// # Constants
///
/// - [`BITS`](PixelFormat::BITS) - Bit depth of the type
/// - [`IS_FLOAT`](PixelFormat::IS_FLOAT) - Whether this is a floating-point type
/// - [`MAX_VALUE`](PixelFormat::MAX_VALUE) - Maximum representable value
///
/// # Example
///
/// ```
/// use vfx_core::PixelFormat;
///
/// // Convert 8-bit to float
/// let byte_val: u8 = 128;
/// let float_val = byte_val.to_f32();
/// assert!((float_val - 0.502).abs() < 0.01);
///
/// // Convert float to 16-bit
/// let back: u16 = PixelFormat::from_f32(0.5);
/// assert_eq!(back, 32768);
/// ```
pub trait PixelFormat: Copy + Clone + Default + Send + Sync + PartialOrd + 'static {
    /// Number of bits per channel.
    const BITS: u32;

    /// Whether this is a floating-point format.
    ///
    /// - `true` for f16, f32
    /// - `false` for u8, u16
    const IS_FLOAT: bool;

    /// Maximum representable value.
    ///
    /// - 255 for u8
    /// - 65535 for u16
    /// - f32::MAX for floats
    const MAX_VALUE: f32;

    /// Minimum representable value (for floats, can be negative).
    const MIN_VALUE: f32;

    /// Convert to f32.
    ///
    /// For integers, normalizes to [0.0, 1.0] range.
    /// For floats, returns the value directly.
    fn to_f32(self) -> f32;

    /// Convert from f32.
    ///
    /// For integers, expects [0.0, 1.0] range and clamps.
    /// For floats, returns the value directly (may clamp for f16).
    fn from_f32(v: f32) -> Self;

    /// Linear interpolation between two values.
    #[inline]
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self::from_f32(a.to_f32() * (1.0 - t) + b.to_f32() * t)
    }

    /// Clamp value to valid range.
    fn clamp(self, min: Self, max: Self) -> Self;

    /// Zero value.
    fn zero() -> Self;

    /// One value (1.0 for floats, max for integers).
    fn one() -> Self;
}

// ============================================================================
// Quantisation: normalised float -> integer code value
// ============================================================================
//
// THE conversion from a `[0,1]` float to an integer code, for the whole stack. It is three
// operations, which is exactly why it kept getting written inline in a slightly different way
// each time — playa carried 27 truncating call sites against one rounding one, vfx-io's
// `imagebuf::storage` had a third spelling, and this file had a fourth.
//
// **Round to nearest, never truncate.** `(v * 255.0) as u8` truncates toward zero, so code `k`
// reconstructs as `k/255` — the LEFT EDGE of the interval it represents. Every value in
// `[k/255, (k+1)/255)` collapses to the bottom of its own bucket, the mean error is **-0.5 LSB
// instead of zero**, and an image written that way is uniformly ~0.2% dark. Uniformly, which is
// why it does not look like a bug: it looks like a slightly different grade. Adding 0.5 first
// puts the reconstruction at the bucket's CENTRE, where the error is +/-0.5 LSB with no bias.
//
// This mirrors OIIO's `scaled_conversion` (`fmath.h`) and FFmpeg's integer rounding. It is not a
// matter of taste: a float -> u8 -> float round trip has to converge, and with truncation it
// drifts darker on every pass.
//
// `+ 0.5` rather than `f32::round()` on purpose. They agree for non-negative input (which is all
// this takes), but `round()` is round-half-AWAY-FROM-ZERO, which has no single SSE2 instruction —
// LLVM emits a call to `roundf` or an inline sequence, and a call in a per-pixel loop costs the
// whole loop its vectorisation. `+ 0.5` is an add.
//
// The cast saturates in Rust (since 1.45), so an out-of-range or NaN input cannot produce a
// garbage code; the explicit clamp is kept because relying on saturation to mean "clamp" is a
// thing a reader has to know rather than see.

/// Normalised float `[0,1]` -> 8-bit code, rounded to nearest.
///
/// See the module note above for why this is not `(v * 255.0) as u8`.
///
/// ```rust
/// use vfx_core::quantize_u8;
/// assert_eq!(quantize_u8(0.0), 0);
/// assert_eq!(quantize_u8(1.0), 255);
/// assert_eq!(quantize_u8(0.5), 128);   // truncation would give 127
/// ```
#[inline]
#[must_use]
pub fn quantize_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// Normalised float `[0,1]` -> 16-bit code, rounded to nearest.
#[inline]
#[must_use]
pub fn quantize_u16(v: f32) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16
}

/// Normalised float `[0,1]` -> `bits`-wide unsigned code (10-, 12-bit DPX/Cineon, ...), rounded
/// to nearest. `bits` is 1..=32.
#[inline]
#[must_use]
pub fn quantize_bits(v: f32, bits: u32) -> u32 {
    debug_assert!((1..=32).contains(&bits));
    let max = ((1u64 << bits) - 1) as f64;
    // f64: a 32-bit scale does not fit f32's 24-bit mantissa.
    (f64::from(v.clamp(0.0, 1.0)) * max + 0.5) as u32
}

/// Normalised float `[-1,1]` -> `i32` code (`v * i32::MAX`), rounding half away from zero like
/// OIIO's `scaled_conversion` for signed targets.
#[inline]
#[must_use]
pub fn quantize_i32(v: f32) -> i32 {
    let s = f64::from(v.clamp(-1.0, 1.0)) * f64::from(i32::MAX);
    (s + if s < 0.0 { -0.5 } else { 0.5 }) as i32
}

/// OIIO's `scaled_conversion` (`fmath.h`), the general form behind the helpers above: scale,
/// round half away from zero, clamp to `[min, max]`. Returns the rounded value as `f32` for the
/// caller to cast into its storage type. NaN maps to `min`.
#[inline]
#[must_use]
pub fn quantize_scaled(v: f32, scale: f32, min: f32, max: f32) -> f32 {
    let s = v * scale;
    let s = s + if s < 0.0 { -0.5 } else { 0.5 };
    if s.is_nan() { min } else { s.clamp(min, max) }
}

/// 16-bit code -> 8-bit code, rounded to nearest (`round(v * 255 / 65535)` = `round(v / 257)`),
/// as OIIO converts `uint16` to `uint8`. `v >> 8` truncates: 51528 gives 201 where OIIO gives 200.
#[inline]
#[must_use]
pub fn u16_to_u8(v: u16) -> u8 {
    ((u32::from(v) + 128) / 257) as u8
}

#[cfg(test)]
mod quantize_tests {
    use super::{quantize_bits, quantize_i32, quantize_u8, quantize_u16, u16_to_u8};

    /// Values measured with OIIO 3.1.14: a uint16 PNG holding 51528, 384, 65407, 32640 converted
    /// with `oiiotool -d uint8` gives 200, 1, 255, 127; the float constant (0.5, 0.2509804,
    /// 0.0019608) written as uint8 gives (128, 64, 1).
    #[test]
    fn conversions_match_oiio() {
        assert_eq!(
            [51528u16, 384, 65407, 32640].map(u16_to_u8),
            [200, 1, 255, 127]
        );
        assert_eq!(
            [0.5f32, 0.2509804, 0.0019608].map(quantize_u8),
            [128, 64, 1]
        );
        // Exhaustive: u16 -> u8 is exactly round(v * 255 / 65535).
        for v in 0..=u16::MAX {
            let want = (f64::from(v) * 255.0 / 65535.0).round() as u8;
            assert_eq!(u16_to_u8(v), want, "v = {v}");
        }
        assert_eq!(quantize_bits(0.5, 10), 512); // 511.5 rounds up
        assert_eq!(quantize_bits(1.0, 32), u32::MAX);
        assert_eq!(quantize_i32(-1.0), -i32::MAX);
        assert_eq!(quantize_i32(0.5), 1_073_741_824); // 1073741823.5 rounds away from zero
    }

    /// The bucket boundaries are where truncation and rounding differ, so they are what a guard
    /// has to pin. Half-way between two codes must land on the UPPER one.
    #[test]
    fn quantisation_rounds_to_nearest_at_every_boundary() {
        for k in 0u16..255 {
            let mid = (f32::from(k) + 0.5) / 255.0;
            assert_eq!(
                quantize_u8(mid),
                (k + 1) as u8,
                "the midpoint above code {k} must round up; truncation would give {k}"
            );
            // A hair below the midpoint stays on the lower code — otherwise the "fix" would be a
            // ceiling, which is the same bias with the opposite sign.
            assert_eq!(
                quantize_u8(mid - 1e-4),
                k as u8,
                "just below the midpoint of {k}"
            );
        }
    }

    /// A float -> u8 -> float round trip must be a fixed point on the exact code values.
    ///
    /// Note what this does NOT prove: it survives the truncating mutation, because `k/255 * 255`
    /// happens to land back on `k` in f32 without needing the rounding. The boundary test above
    /// is the one that fails when rounding is removed. This one guards the other direction — an
    /// over-correction (a ceiling, or a scale of 256) breaks it and not the boundaries.
    #[test]
    fn the_round_trip_is_a_fixed_point() {
        for k in 0u16..=255 {
            let v = f32::from(k) / 255.0;
            assert_eq!(
                quantize_u8(v),
                k as u8,
                "code {k} must survive a round trip"
            );
        }
        for k in [0u32, 1, 32767, 65534, 65535] {
            let v = k as f32 / 65535.0;
            assert_eq!(quantize_u16(v), k as u16, "16-bit code {k} must survive");
        }
    }

    /// Out of range and NaN must produce the ends, not a wrapped code.
    #[test]
    fn out_of_range_saturates() {
        assert_eq!(quantize_u8(-1.0), 0);
        assert_eq!(quantize_u8(2.0), 255);
        assert_eq!(quantize_u8(f32::NAN), 0);
        assert_eq!(quantize_u8(f32::INFINITY), 255);
        assert_eq!(quantize_u16(-0.5), 0);
        assert_eq!(quantize_u16(1.5), 65535);
    }
}

impl PixelFormat for u8 {
    const BITS: u32 = 8;
    const IS_FLOAT: bool = false;
    const MAX_VALUE: f32 = 255.0;
    const MIN_VALUE: f32 = 0.0;

    #[inline]
    fn to_f32(self) -> f32 {
        self as f32 / 255.0
    }

    #[inline]
    fn from_f32(v: f32) -> Self {
        quantize_u8(v)
    }

    #[inline]
    fn clamp(self, min: Self, max: Self) -> Self {
        Ord::clamp(self, min, max)
    }

    #[inline]
    fn zero() -> Self {
        0
    }

    #[inline]
    fn one() -> Self {
        255
    }
}

impl PixelFormat for u16 {
    const BITS: u32 = 16;
    const IS_FLOAT: bool = false;
    const MAX_VALUE: f32 = 65535.0;
    const MIN_VALUE: f32 = 0.0;

    #[inline]
    fn to_f32(self) -> f32 {
        self as f32 / 65535.0
    }

    #[inline]
    fn from_f32(v: f32) -> Self {
        quantize_u16(v)
    }

    #[inline]
    fn clamp(self, min: Self, max: Self) -> Self {
        Ord::clamp(self, min, max)
    }

    #[inline]
    fn zero() -> Self {
        0
    }

    #[inline]
    fn one() -> Self {
        65535
    }
}

impl PixelFormat for f16 {
    const BITS: u32 = 16;
    const IS_FLOAT: bool = true;
    const MAX_VALUE: f32 = 65504.0; // f16 max
    const MIN_VALUE: f32 = -65504.0;

    #[inline]
    fn to_f32(self) -> f32 {
        self.to_f32()
    }

    #[inline]
    fn from_f32(v: f32) -> Self {
        f16::from_f32(v)
    }

    #[inline]
    fn clamp(self, min: Self, max: Self) -> Self {
        if self < min {
            min
        } else if self > max {
            max
        } else {
            self
        }
    }

    #[inline]
    fn zero() -> Self {
        f16::ZERO
    }

    #[inline]
    fn one() -> Self {
        f16::ONE
    }
}

impl PixelFormat for f32 {
    const BITS: u32 = 32;
    const IS_FLOAT: bool = true;
    const MAX_VALUE: f32 = f32::MAX;
    const MIN_VALUE: f32 = f32::MIN;

    #[inline]
    fn to_f32(self) -> f32 {
        self
    }

    #[inline]
    fn from_f32(v: f32) -> Self {
        v
    }

    #[inline]
    fn clamp(self, min: Self, max: Self) -> Self {
        f32::clamp(self, min, max)
    }

    #[inline]
    fn zero() -> Self {
        0.0
    }

    #[inline]
    fn one() -> Self {
        1.0
    }
}

/// RGB pixel with color space tracking.
///
/// A 3-channel pixel type for red, green, and blue values.
///
/// # Type Parameters
///
/// - `C: ColorSpace` - The color space this pixel belongs to
/// - `T: PixelFormat` - The underlying data type (u8, u16, f16, f32)
///
/// # Memory Layout
///
/// Uses `#[repr(C)]` for predictable layout: `[R, G, B]`
///
/// # Example
///
/// ```
/// use vfx_core::prelude::*;
///
/// // Create an sRGB pixel
/// let pixel: Rgb<Srgb, u8> = Rgb::new(255, 128, 64);
/// assert_eq!(pixel.r, 255);
///
/// // Convert to float
/// let float_pixel: Rgb<Srgb, f32> = pixel.convert_format();
/// assert!((float_pixel.r - 1.0).abs() < 0.01);
/// ```
#[repr(C)]
#[derive(Copy, Clone, Default, PartialEq)]
pub struct Rgb<C: ColorSpace, T: PixelFormat> {
    /// Red channel value.
    pub r: T,
    /// Green channel value.
    pub g: T,
    /// Blue channel value.
    pub b: T,
    /// PhantomData to track color space at compile time.
    _colorspace: PhantomData<C>,
}

impl<C: ColorSpace, T: PixelFormat> Rgb<C, T> {
    /// Create a new RGB pixel.
    ///
    /// # Arguments
    ///
    /// * `r` - Red channel value
    /// * `g` - Green channel value
    /// * `b` - Blue channel value
    #[inline]
    pub const fn new(r: T, g: T, b: T) -> Self {
        Self {
            r,
            g,
            b,
            _colorspace: PhantomData,
        }
    }

    /// Create a grayscale pixel with equal RGB values.
    #[inline]
    pub const fn gray(v: T) -> Self {
        Self::new(v, v, v)
    }

    /// Create a black pixel (all zeros).
    #[inline]
    pub fn black() -> Self {
        Self::new(T::zero(), T::zero(), T::zero())
    }

    /// Create a white pixel (all max values).
    #[inline]
    pub fn white() -> Self {
        Self::new(T::one(), T::one(), T::one())
    }

    /// Get RGB values as an array.
    #[inline]
    pub fn to_array(self) -> [T; 3] {
        [self.r, self.g, self.b]
    }

    /// Create from an array.
    #[inline]
    pub fn from_array(arr: [T; 3]) -> Self {
        Self::new(arr[0], arr[1], arr[2])
    }

    /// Convert to f32 array (normalized for integers).
    #[inline]
    pub fn to_f32_array(self) -> [f32; 3] {
        [self.r.to_f32(), self.g.to_f32(), self.b.to_f32()]
    }

    /// Create from f32 array.
    #[inline]
    pub fn from_f32_array(arr: [f32; 3]) -> Self {
        Self::new(
            T::from_f32(arr[0]),
            T::from_f32(arr[1]),
            T::from_f32(arr[2]),
        )
    }

    /// Convert to a different pixel format (same color space).
    ///
    /// # Example
    ///
    /// ```
    /// use vfx_core::prelude::*;
    ///
    /// let byte_pixel: Rgb<Srgb, u8> = Rgb::new(255, 128, 0);
    /// let float_pixel: Rgb<Srgb, f32> = byte_pixel.convert_format();
    /// ```
    #[inline]
    pub fn convert_format<U: PixelFormat>(self) -> Rgb<C, U> {
        Rgb::new(
            U::from_f32(self.r.to_f32()),
            U::from_f32(self.g.to_f32()),
            U::from_f32(self.b.to_f32()),
        )
    }

    /// Apply a function to each channel.
    #[inline]
    pub fn map<F: Fn(T) -> T>(self, f: F) -> Self {
        Self::new(f(self.r), f(self.g), f(self.b))
    }

    /// Apply a function with f32 intermediates.
    #[inline]
    pub fn map_f32<F: Fn(f32) -> f32>(self, f: F) -> Self {
        Self::new(
            T::from_f32(f(self.r.to_f32())),
            T::from_f32(f(self.g.to_f32())),
            T::from_f32(f(self.b.to_f32())),
        )
    }

    /// Calculate luminance using Rec.709 coefficients.
    ///
    /// Note: This is only accurate for Rec.709/sRGB primaries.
    /// For other color spaces, use proper luminance matrices.
    #[inline]
    pub fn luminance_rec709(self) -> f32 {
        luminance_rec709(self.to_f32_array())
    }
}

impl<C: ColorSpace, T: PixelFormat + fmt::Debug> fmt::Debug for Rgb<C, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(&format!("Rgb<{}>", C::NAME))
            .field("r", &self.r)
            .field("g", &self.g)
            .field("b", &self.b)
            .finish()
    }
}

impl<C: ColorSpace, T: PixelFormat + fmt::Display> fmt::Display for Rgb<C, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RGB({}, {}, {})", self.r, self.g, self.b)
    }
}

/// RGBA pixel with color space tracking.
///
/// A 4-channel pixel type for red, green, blue, and alpha values.
///
/// # Alpha Handling
///
/// By default, alpha is **NOT** premultiplied. Use explicit functions
/// to premultiply/unpremultiply when needed.
///
/// # Type Parameters
///
/// - `C: ColorSpace` - The color space this pixel belongs to
/// - `T: PixelFormat` - The underlying data type (u8, u16, f16, f32)
///
/// # Memory Layout
///
/// Uses `#[repr(C)]` for predictable layout: `[R, G, B, A]`
///
/// # Example
///
/// ```
/// use vfx_core::prelude::*;
///
/// // Opaque red pixel in ACEScg
/// let pixel: Rgba<AcesCg, f32> = Rgba::new(1.0, 0.0, 0.0, 1.0);
///
/// // Semi-transparent
/// let semi: Rgba<AcesCg, f32> = Rgba::with_alpha(pixel.rgb(), 0.5);
/// ```
#[must_use]
#[repr(C)]
#[derive(Copy, Clone, Default, PartialEq)]
pub struct Rgba<C: ColorSpace, T: PixelFormat> {
    /// Red channel value.
    pub r: T,
    /// Green channel value.
    pub g: T,
    /// Blue channel value.
    pub b: T,
    /// Alpha channel value.
    pub a: T,
    /// PhantomData to track color space at compile time.
    _colorspace: PhantomData<C>,
}

impl<C: ColorSpace, T: PixelFormat> Rgba<C, T> {
    /// Create a new RGBA pixel.
    #[inline]
    pub const fn new(r: T, g: T, b: T, a: T) -> Self {
        Self {
            r,
            g,
            b,
            a,
            _colorspace: PhantomData,
        }
    }

    /// Create from RGB with specified alpha.
    #[inline]
    pub fn with_alpha(rgb: Rgb<C, T>, a: T) -> Self {
        Self::new(rgb.r, rgb.g, rgb.b, a)
    }

    /// Create an opaque pixel (alpha = 1.0).
    #[inline]
    pub fn opaque(r: T, g: T, b: T) -> Self {
        Self::new(r, g, b, T::one())
    }

    /// Create from RGB with opaque alpha.
    #[inline]
    pub fn from_rgb(rgb: Rgb<C, T>) -> Self {
        Self::with_alpha(rgb, T::one())
    }

    /// Create a grayscale pixel with equal RGB values.
    #[inline]
    pub fn gray(v: T, a: T) -> Self {
        Self::new(v, v, v, a)
    }

    /// Create a black pixel (all zeros, opaque).
    #[inline]
    pub fn black() -> Self {
        Self::new(T::zero(), T::zero(), T::zero(), T::one())
    }

    /// Create a white pixel (all max values, opaque).
    #[inline]
    pub fn white() -> Self {
        Self::new(T::one(), T::one(), T::one(), T::one())
    }

    /// Create a transparent pixel (all zeros including alpha).
    #[inline]
    pub fn transparent() -> Self {
        Self::new(T::zero(), T::zero(), T::zero(), T::zero())
    }

    /// Get RGB component (discarding alpha).
    #[inline]
    pub fn rgb(self) -> Rgb<C, T> {
        Rgb::new(self.r, self.g, self.b)
    }

    /// Get RGBA values as an array.
    #[inline]
    pub fn to_array(self) -> [T; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// Create from an array.
    #[inline]
    pub fn from_array(arr: [T; 4]) -> Self {
        Self::new(arr[0], arr[1], arr[2], arr[3])
    }

    /// Convert to f32 array.
    #[inline]
    pub fn to_f32_array(self) -> [f32; 4] {
        [
            self.r.to_f32(),
            self.g.to_f32(),
            self.b.to_f32(),
            self.a.to_f32(),
        ]
    }

    /// Create from f32 array.
    #[inline]
    pub fn from_f32_array(arr: [f32; 4]) -> Self {
        Self::new(
            T::from_f32(arr[0]),
            T::from_f32(arr[1]),
            T::from_f32(arr[2]),
            T::from_f32(arr[3]),
        )
    }

    /// Convert to a different pixel format (same color space).
    #[inline]
    pub fn convert_format<U: PixelFormat>(self) -> Rgba<C, U> {
        Rgba::new(
            U::from_f32(self.r.to_f32()),
            U::from_f32(self.g.to_f32()),
            U::from_f32(self.b.to_f32()),
            U::from_f32(self.a.to_f32()),
        )
    }

    /// Apply a function to RGB channels (preserving alpha).
    #[inline]
    pub fn map_rgb<F: Fn(T) -> T>(self, f: F) -> Self {
        Self::new(f(self.r), f(self.g), f(self.b), self.a)
    }

    /// Apply a function to all channels including alpha.
    #[inline]
    pub fn map<F: Fn(T) -> T>(self, f: F) -> Self {
        Self::new(f(self.r), f(self.g), f(self.b), f(self.a))
    }

    /// Apply a function with f32 intermediates (RGB only).
    #[inline]
    pub fn map_rgb_f32<F: Fn(f32) -> f32>(self, f: F) -> Self {
        Self::new(
            T::from_f32(f(self.r.to_f32())),
            T::from_f32(f(self.g.to_f32())),
            T::from_f32(f(self.b.to_f32())),
            self.a,
        )
    }

    /// Premultiply RGB by alpha.
    ///
    /// Converts straight alpha to premultiplied alpha:
    /// `(R, G, B, A) -> (R*A, G*A, B*A, A)`
    #[inline]
    pub fn premultiply(self) -> Self {
        let a = self.a.to_f32();
        Self::new(
            T::from_f32(self.r.to_f32() * a),
            T::from_f32(self.g.to_f32() * a),
            T::from_f32(self.b.to_f32() * a),
            self.a,
        )
    }

    /// Unpremultiply RGB by alpha.
    ///
    /// Converts premultiplied alpha to straight alpha:
    /// `(R*A, G*A, B*A, A) -> (R, G, B, A)`
    ///
    /// OIIO's `ImageBufAlgo::unpremult` rule, [`crate::alpha::unpremult_value`]: alpha 0 (or 1)
    /// leaves the colour as it is, any other alpha divides. (This used to return transparent
    /// black below `1e-6`, a fourth rule nothing else in the workspace followed; B15.)
    #[inline]
    pub fn unpremultiply(self) -> Self {
        let a = self.a.to_f32();
        let f = |v: T| T::from_f32(crate::alpha::unpremult_value(v.to_f32(), a));
        Self::new(f(self.r), f(self.g), f(self.b), self.a)
    }

    /// Check if pixel is fully opaque.
    #[inline]
    pub fn is_opaque(self) -> bool {
        self.a.to_f32() >= 1.0 - 1e-6
    }

    /// Check if pixel is fully transparent.
    #[inline]
    pub fn is_transparent(self) -> bool {
        self.a.to_f32() < 1e-6
    }
}

impl<C: ColorSpace, T: PixelFormat + fmt::Debug> fmt::Debug for Rgba<C, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(&format!("Rgba<{}>", C::NAME))
            .field("r", &self.r)
            .field("g", &self.g)
            .field("b", &self.b)
            .field("a", &self.a)
            .finish()
    }
}

impl<C: ColorSpace, T: PixelFormat + fmt::Display> fmt::Display for Rgba<C, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RGBA({}, {}, {}, {})", self.r, self.g, self.b, self.a)
    }
}

// ============================================================================
// Arithmetic Operations
// ============================================================================

impl<C: ColorSpace, T: PixelFormat> Add for Rgb<C, T> {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self::new(
            T::from_f32(self.r.to_f32() + rhs.r.to_f32()),
            T::from_f32(self.g.to_f32() + rhs.g.to_f32()),
            T::from_f32(self.b.to_f32() + rhs.b.to_f32()),
        )
    }
}

impl<C: ColorSpace, T: PixelFormat> Sub for Rgb<C, T> {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(
            T::from_f32(self.r.to_f32() - rhs.r.to_f32()),
            T::from_f32(self.g.to_f32() - rhs.g.to_f32()),
            T::from_f32(self.b.to_f32() - rhs.b.to_f32()),
        )
    }
}

impl<C: ColorSpace, T: PixelFormat> Mul<f32> for Rgb<C, T> {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: f32) -> Self::Output {
        Self::new(
            T::from_f32(self.r.to_f32() * rhs),
            T::from_f32(self.g.to_f32() * rhs),
            T::from_f32(self.b.to_f32() * rhs),
        )
    }
}

impl<C: ColorSpace, T: PixelFormat> Add for Rgba<C, T> {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self::new(
            T::from_f32(self.r.to_f32() + rhs.r.to_f32()),
            T::from_f32(self.g.to_f32() + rhs.g.to_f32()),
            T::from_f32(self.b.to_f32() + rhs.b.to_f32()),
            T::from_f32(self.a.to_f32() + rhs.a.to_f32()),
        )
    }
}

impl<C: ColorSpace, T: PixelFormat> Mul<f32> for Rgba<C, T> {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: f32) -> Self::Output {
        Self::new(
            T::from_f32(self.r.to_f32() * rhs),
            T::from_f32(self.g.to_f32() * rhs),
            T::from_f32(self.b.to_f32() * rhs),
            T::from_f32(self.a.to_f32() * rhs),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::colorspace::Srgb;

    #[test]
    fn test_pixel_format_u8() {
        assert_eq!(u8::BITS, 8);
        const { assert!(!u8::IS_FLOAT) };
        assert!((128u8.to_f32() - 0.502).abs() < 0.01);
        assert_eq!(u8::from_f32(0.5), 128);
    }

    #[test]
    fn test_pixel_format_f32() {
        assert_eq!(<f32 as PixelFormat>::BITS, 32);
        const { assert!(f32::IS_FLOAT) };
        assert_eq!(0.5f32.to_f32(), 0.5);
        assert_eq!(f32::from_f32(0.5), 0.5);
    }

    #[test]
    fn test_rgb_creation() {
        let pixel: Rgb<Srgb, u8> = Rgb::new(255, 128, 64);
        assert_eq!(pixel.r, 255);
        assert_eq!(pixel.g, 128);
        assert_eq!(pixel.b, 64);
    }

    #[test]
    fn test_rgb_format_conversion() {
        let byte_pixel: Rgb<Srgb, u8> = Rgb::new(255, 128, 0);
        let float_pixel: Rgb<Srgb, f32> = byte_pixel.convert_format();
        assert!((float_pixel.r - 1.0).abs() < 0.01);
        assert!((float_pixel.g - 0.502).abs() < 0.01);
        assert!((float_pixel.b - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_rgba_premultiply() {
        let pixel: Rgba<Srgb, f32> = Rgba::new(1.0, 0.5, 0.25, 0.5);
        let premul = pixel.premultiply();
        assert!((premul.r - 0.5).abs() < 0.001);
        assert!((premul.g - 0.25).abs() < 0.001);
        assert!((premul.b - 0.125).abs() < 0.001);
        assert!((premul.a - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_rgba_unpremultiply() {
        let premul: Rgba<Srgb, f32> = Rgba::new(0.5, 0.25, 0.125, 0.5);
        let straight = premul.unpremultiply();
        assert!((straight.r - 1.0).abs() < 0.001);
        assert!((straight.g - 0.5).abs() < 0.001);
        assert!((straight.b - 0.25).abs() < 0.001);
        // Alpha 0 keeps its colour (oiiotool --unpremult); tiny alpha divides.
        let zero: Rgba<Srgb, f32> = Rgba::new(0.3, 0.2, 0.1, 0.0);
        assert_eq!(zero.unpremultiply(), zero);
        let tiny: Rgba<Srgb, f32> = Rgba::new(0.3, 0.2, 0.1, 1e-7);
        assert!((tiny.unpremultiply().r - 0.3 / 1e-7).abs() < 1.0);
    }

    #[test]
    fn test_pixel_arithmetic() {
        let a: Rgb<Srgb, f32> = Rgb::new(0.5, 0.3, 0.2);
        let b: Rgb<Srgb, f32> = Rgb::new(0.1, 0.2, 0.3);
        let sum = a + b;
        assert!((sum.r - 0.6).abs() < 0.001);
        assert!((sum.g - 0.5).abs() < 0.001);
        assert!((sum.b - 0.5).abs() < 0.001);
    }
}
