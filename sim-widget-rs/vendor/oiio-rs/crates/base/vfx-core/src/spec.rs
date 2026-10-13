//! Image specification and metadata.
//!
//! This module provides [`ImageSpec`] - a comprehensive description of image
//! properties including dimensions, pixel format, channel layout, and metadata.
//! It is designed to be compatible with OpenImageIO's `ImageSpec`.
//!
//! # Overview
//!
//! [`ImageSpec`] contains everything needed to interpret raw pixel data:
//!
//! - **Dimensions**: width, height, depth (for 3D textures)
//! - **Origin**: x, y, z coordinates of the data window origin
//! - **Full/Display window**: full_x, full_y, full_z, full_width, full_height, full_depth
//! - **Tiling**: tile_width, tile_height, tile_depth for tiled images
//! - **Channels**: nchannels, channel_names, channelformats
//! - **Special channels**: alpha_channel, z_channel indices
//! - **Deep images**: deep flag for deep/volumetric data
//! - **Metadata**: arbitrary key-value attributes
//!
//! # Usage
//!
//! ```rust
//! use vfx_core::{ImageSpec, Rect, DataFormat};
//!
//! // Simple RGB image
//! let spec = ImageSpec::rgb(1920, 1080);
//!
//! // RGBA with custom display window
//! let mut spec = ImageSpec::rgba(1920, 1080);
//! spec.full_width = 2048; // 2K DCI with letterbox
//!
//! // Add metadata
//! spec.set_attr("Author", "VFX Artist");
//! spec.set_attr("Software", "vfx-rs");
//!
//! // Compute memory requirements
//! let scanline_bytes = spec.scanline_bytes(false);
//! let image_bytes = spec.image_bytes(false);
//! ```
//!
//! # Display vs Data Window
//!
//! VFX workflows often use overscan - extra pixels beyond the final frame
//! for filtering and transforms. The display window is the final output area,
//! while the data window is where actual pixel data exists.
//!
//! ```text
//! ┌─────────────────────────────┐
//! │        Display Window       │
//! │   (full_x, full_y) origin   │
//! │   ┌───────────────────┐     │
//! │   │    Data Window    │     │
//! │   │   (x, y) origin   │     │
//! │   └───────────────────┘     │
//! │                             │
//! └─────────────────────────────┘
//! ```
//!
//! # Dependencies
//!
//! - [`crate::rect::Rect`] - For window definitions
//! - [`crate::format::TypeDesc`] - For type descriptors
//!
//! # Used By
//!
//! - [`crate::image::Image`] - Stores spec alongside pixel data
//! - `vfx-io` - Reads/writes spec from image files

use crate::Rect;
use crate::Roi3D;
use crate::format::{DataFormat, TypeDesc};
use std::collections::HashMap;

/// Alias for backward compatibility.
/// Prefer using [`DataFormat`] directly.
#[deprecated(since = "0.2.0", note = "Use DataFormat instead")]
pub type ChannelFormat = DataFormat;

/// Attribute value that can be stored in image metadata.
///
/// Supports common types found in image file metadata.
///
/// Non-exhaustive: match with a wildcard, and read through [`AttrValue::value`] to look past a
/// [`AttrValue::Typed`] wrapper.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum AttrValue {
    /// Integer value
    Int(i64),
    /// Floating-point value
    Float(f64),
    /// String value
    String(String),
    /// Integer array
    IntArray(Vec<i64>),
    /// Float array
    FloatArray(Vec<f64>),
    /// String array (OIIO `TypeDesc(STRING, n)`, e.g. an EXR `stringvector` such as `multiView`)
    StringArray(Vec<String>),
    /// 3x3 matrix (row-major)
    Matrix3([f32; 9]),
    /// 4x4 matrix (row-major)
    Matrix4([f32; 16]),
    /// Raw binary data (ICC profiles, EXIF/XMP blobs). PLAN9.md C5/O5: without this variant, a
    /// `vfx_io::Attrs::Bytes` value (an embedded ICC profile, say) has nowhere to go when folded
    /// into an `ImageSpec` — it is silently dropped, and cannot be written back out.
    Bytes(Vec<u8>),
    /// A value that carries the exact type it has in the file format it came from (an EXR type
    /// name: `float`, `double`, `v2d`, `m44d`, `rational`, `timecode`, an opaque type's own
    /// name, ...), because the variants above have one int, one float and untyped arrays. A
    /// reader wraps only where the shape alone would lose the type; the matching writer
    /// re-applies it. Every accessor reads through the wrapper ([`AttrValue::value`]).
    Typed {
        /// The format's type name.
        ty: String,
        /// The value, never itself `Typed`.
        value: Box<AttrValue>,
    },
}

impl AttrValue {
    /// Wraps `value` with its exact type name (see [`AttrValue::Typed`]).
    pub fn typed(ty: impl Into<String>, value: AttrValue) -> Self {
        let value = match value {
            Self::Typed { value, .. } => *value,
            other => other,
        };
        Self::Typed {
            ty: ty.into(),
            value: Box::new(value),
        }
    }

    /// The value itself: looks through a [`AttrValue::Typed`] wrapper.
    pub fn value(&self) -> &AttrValue {
        match self {
            Self::Typed { value, .. } => value,
            other => other,
        }
    }

    /// The exact type name, when this value carries one.
    pub fn type_tag(&self) -> Option<&str> {
        match self {
            Self::Typed { ty, .. } => Some(ty),
            _ => None,
        }
    }

    /// Returns this value as an integer, if applicable.
    pub fn as_int(&self) -> Option<i64> {
        match self.value() {
            Self::Int(v) => Some(*v),
            Self::Float(v) => Some(*v as i64),
            _ => None,
        }
    }

    /// Returns this value as a float, if applicable.
    pub fn as_float(&self) -> Option<f64> {
        match self.value() {
            Self::Int(v) => Some(*v as f64),
            Self::Float(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns this value as a string, if applicable.
    pub fn as_str(&self) -> Option<&str> {
        match self.value() {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    /// Returns the int array if this value is an IntArray.
    pub fn as_int_array(&self) -> Option<&[i64]> {
        match self.value() {
            Self::IntArray(arr) => Some(arr),
            _ => None,
        }
    }

    /// Returns the float array if this value is a FloatArray.
    pub fn as_float_array(&self) -> Option<&[f64]> {
        match self.value() {
            Self::FloatArray(arr) => Some(arr),
            _ => None,
        }
    }

    /// Returns the 3x3 matrix if this value is a Matrix3.
    pub fn as_matrix3(&self) -> Option<&[f32; 9]> {
        match self.value() {
            Self::Matrix3(m) => Some(m),
            _ => None,
        }
    }

    /// Returns the strings if this value is a StringArray.
    pub fn as_string_array(&self) -> Option<&[String]> {
        match self.value() {
            Self::StringArray(v) => Some(v),
            _ => None,
        }
    }

    /// Returns the raw bytes if this value is a Bytes blob.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self.value() {
            Self::Bytes(b) => Some(b),
            _ => None,
        }
    }

    /// Returns the 4x4 matrix if this value is a Matrix4.
    pub fn as_matrix4(&self) -> Option<&[f32; 16]> {
        match self.value() {
            Self::Matrix4(m) => Some(m),
            _ => None,
        }
    }

    /// Returns the type name of this attribute value.
    pub fn type_name(&self) -> &'static str {
        match self.value() {
            Self::Int(_) => "int",
            Self::Float(_) => "float",
            Self::String(_) => "string",
            Self::IntArray(_) => "int[]",
            Self::FloatArray(_) => "float[]",
            Self::StringArray(_) => "string[]",
            Self::Matrix3(_) => "matrix33",
            Self::Matrix4(_) => "matrix44",
            Self::Bytes(_) => "bytes",
            Self::Typed { .. } => "typed",
        }
    }
}

impl std::fmt::Display for AttrValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Typed { value, .. } => write!(f, "{value}"),
            Self::Int(v) => write!(f, "{}", v),
            Self::Float(v) => write!(f, "{}", v),
            Self::String(s) => write!(f, "\"{}\"", s),
            Self::IntArray(arr) => {
                write!(f, "[")?;
                for (i, v) in arr.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, "]")
            }
            Self::FloatArray(arr) => {
                write!(f, "[")?;
                for (i, v) in arr.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, "]")
            }
            Self::Matrix3(m) => {
                write!(f, "[")?;
                for (i, v) in m.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, "]")
            }
            Self::Matrix4(m) => {
                write!(f, "[")?;
                for (i, v) in m.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, "]")
            }
            Self::StringArray(arr) => {
                write!(f, "[")?;
                for (i, s) in arr.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "\"{}\"", s)?;
                }
                write!(f, "]")
            }
            Self::Bytes(v) => write!(f, "<{} bytes>", v.len()),
        }
    }
}

impl From<i32> for AttrValue {
    fn from(v: i32) -> Self {
        Self::Int(v as i64)
    }
}

impl From<i64> for AttrValue {
    fn from(v: i64) -> Self {
        Self::Int(v)
    }
}

impl From<f32> for AttrValue {
    fn from(v: f32) -> Self {
        Self::Float(v as f64)
    }
}

impl From<f64> for AttrValue {
    fn from(v: f64) -> Self {
        Self::Float(v)
    }
}

impl From<String> for AttrValue {
    fn from(v: String) -> Self {
        Self::String(v)
    }
}

impl From<&str> for AttrValue {
    fn from(v: &str) -> Self {
        Self::String(v.to_string())
    }
}

impl From<Vec<String>> for AttrValue {
    fn from(v: Vec<String>) -> Self {
        Self::StringArray(v)
    }
}

impl From<Vec<u8>> for AttrValue {
    fn from(v: Vec<u8>) -> Self {
        Self::Bytes(v)
    }
}

/// Comprehensive image specification describing format and metadata.
///
/// This struct contains all information needed to interpret raw pixel data,
/// including dimensions, channel layout, data type, and arbitrary metadata.
/// It is designed to be compatible with OpenImageIO's `ImageSpec`.
///
/// # Design
///
/// Modeled after OpenImageIO's `ImageSpec`:
/// - Uses Rust enums instead of type codes
/// - Strongly typed channel format via [`DataFormat`]
/// - HashMap for flexible metadata
/// - Full support for origin coordinates, tiling, and deep images
///
/// # Example
///
/// ```rust
/// use vfx_core::{ImageSpec, DataFormat, Rect};
///
/// let mut spec = ImageSpec::new(1920, 1080, 4, DataFormat::F16);
/// spec.channel_names = vec!["R".into(), "G".into(), "B".into(), "A".into()];
/// spec.set_attr("compression", "piz");
///
/// assert_eq!(spec.bytes_per_pixel(), 8); // 4 channels * 2 bytes
/// ```
#[derive(Debug, Clone)]
pub struct ImageSpec {
    // === Dimensions (data window) ===
    /// Image width in pixels (data window)
    pub width: u32,
    /// Image height in pixels (data window)
    pub height: u32,
    /// Image depth (1 for 2D images, >1 for 3D textures)
    pub depth: u32,

    // === Data window origin ===
    /// X origin of the data window (pixel data)
    pub x: i32,
    /// Y origin of the data window (pixel data)
    pub y: i32,
    /// Z origin of the data window (for 3D images)
    pub z: i32,

    // === Full/Display window ===
    /// Full/display width
    pub full_width: u32,
    /// Full/display height
    pub full_height: u32,
    /// Full/display depth
    pub full_depth: u32,
    /// Full/display X origin
    pub full_x: i32,
    /// Full/display Y origin
    pub full_y: i32,
    /// Full/display Z origin
    pub full_z: i32,

    // === Tiling ===
    /// Tile width (0 = not tiled, scanline-based)
    pub tile_width: u32,
    /// Tile height (0 = not tiled)
    pub tile_height: u32,
    /// Tile depth (for 3D tiled images)
    pub tile_depth: u32,

    // === Channels ===
    /// Number of channels per pixel
    pub nchannels: u8,
    /// Default data type for channels
    pub format: DataFormat,
    /// Per-channel data formats (if different from default)
    pub channelformats: Vec<DataFormat>,
    /// Channel names (e.g., ["R", "G", "B", "A"])
    pub channel_names: Vec<String>,
    /// Index of alpha channel (-1 if none)
    pub alpha_channel: i32,
    /// Index of depth/Z channel (-1 if none)
    pub z_channel: i32,

    // === Deep image ===
    /// Whether this is a deep (multi-sample per pixel) image
    pub deep: bool,

    // === Legacy fields for backward compatibility ===
    /// Data window - region containing actual pixel data
    #[deprecated(since = "0.3.0", note = "Use x, y, width, height instead")]
    pub data_window: Rect,
    /// Display window - final output region
    #[deprecated(
        since = "0.3.0",
        note = "Use full_x, full_y, full_width, full_height instead"
    )]
    pub display_window: Rect,
    /// Alias for nchannels (backward compatibility)
    #[deprecated(since = "0.3.0", note = "Use nchannels instead")]
    pub channels: u8,

    // === Metadata ===
    /// Arbitrary metadata attributes
    pub attributes: HashMap<String, AttrValue>,
}

impl ImageSpec {
    /// Creates a new image specification with given dimensions and format.
    ///
    /// Both data and display windows are initialized to the full image.
    ///
    /// # Arguments
    ///
    /// * `width` - Image width in pixels
    /// * `height` - Image height in pixels
    /// * `nchannels` - Number of channels per pixel
    /// * `format` - Channel data type
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::{ImageSpec, DataFormat};
    ///
    /// let spec = ImageSpec::new(1920, 1080, 4, DataFormat::F16);
    /// assert_eq!(spec.width, 1920);
    /// assert_eq!(spec.nchannels, 4);
    /// ```
    #[allow(deprecated)]
    pub fn new(width: u32, height: u32, nchannels: u8, format: DataFormat) -> Self {
        let window = Rect::from_size(width, height);
        Self {
            // Dimensions
            width,
            height,
            depth: 1,
            // Origin
            x: 0,
            y: 0,
            z: 0,
            // Full/Display window
            full_width: width,
            full_height: height,
            full_depth: 1,
            full_x: 0,
            full_y: 0,
            full_z: 0,
            // Tiling
            tile_width: 0,
            tile_height: 0,
            tile_depth: 0,
            // Channels
            nchannels,
            format,
            channelformats: Vec::new(),
            channel_names: Vec::new(),
            alpha_channel: -1,
            z_channel: -1,
            // Deep
            deep: false,
            // Legacy
            data_window: window,
            display_window: window,
            channels: nchannels,
            // Metadata
            attributes: HashMap::new(),
        }
    }

    /// Creates a spec for an RGB image (3 channels).
    ///
    /// Uses F16 format by default (standard for VFX).
    #[inline]
    pub fn rgb(width: u32, height: u32) -> Self {
        let mut spec = Self::new(width, height, 3, DataFormat::F16);
        spec.channel_names = vec!["R".into(), "G".into(), "B".into()];
        spec
    }

    /// Creates a spec for an RGBA image (4 channels).
    ///
    /// Uses F16 format by default (standard for VFX).
    #[inline]
    pub fn rgba(width: u32, height: u32) -> Self {
        let mut spec = Self::new(width, height, 4, DataFormat::F16);
        spec.channel_names = vec!["R".into(), "G".into(), "B".into(), "A".into()];
        spec.alpha_channel = 3;
        spec
    }

    /// Creates a spec for a grayscale image (1 channel).
    #[inline]
    pub fn gray(width: u32, height: u32) -> Self {
        let mut spec = Self::new(width, height, 1, DataFormat::F16);
        spec.channel_names = vec!["Y".into()];
        spec
    }

    /// Creates a spec for a grayscale + alpha image (2 channels).
    #[inline]
    pub fn gray_alpha(width: u32, height: u32) -> Self {
        let mut spec = Self::new(width, height, 2, DataFormat::F16);
        spec.channel_names = vec!["Y".into(), "A".into()];
        spec.alpha_channel = 1;
        spec
    }

    /// Creates a spec from a [`Roi3D`].
    ///
    /// The ROI defines the image dimensions and channel count.
    /// Format defaults to F32.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::{ImageSpec, Roi3D};
    ///
    /// let roi = Roi3D::new_2d_with_channels(0, 1920, 0, 1080, 0, 4);
    /// let spec = ImageSpec::from_roi(&roi);
    /// assert_eq!(spec.width, 1920);
    /// assert_eq!(spec.height, 1080);
    /// assert_eq!(spec.nchannels, 4);
    /// ```
    pub fn from_roi(roi: &Roi3D) -> Self {
        let width = (roi.xend - roi.xbegin).max(0) as u32;
        let height = (roi.yend - roi.ybegin).max(0) as u32;
        let depth = (roi.zend - roi.zbegin).max(0) as u32;
        let nchannels = (roi.chend - roi.chbegin).clamp(0, 255) as u8;

        let mut spec = Self::new(width, height, nchannels, DataFormat::F32);
        spec.depth = depth;
        spec.x = roi.xbegin;
        spec.y = roi.ybegin;
        spec.z = roi.zbegin;
        spec.default_channel_names();
        spec
    }

    /// Creates a spec from a [`Roi3D`] with a specified number of channels.
    ///
    /// The ROI defines spatial dimensions, but channel count is overridden.
    pub fn from_roi_nchannels(roi: &Roi3D, nchannels: u32) -> Self {
        let width = (roi.xend - roi.xbegin).max(0) as u32;
        let height = (roi.yend - roi.ybegin).max(0) as u32;
        let depth = (roi.zend - roi.zbegin).max(0) as u32;

        let mut spec = Self::new(width, height, nchannels.min(255) as u8, DataFormat::F32);
        spec.depth = depth;
        spec.x = roi.xbegin;
        spec.y = roi.ybegin;
        spec.z = roi.zbegin;
        spec.default_channel_names();
        spec
    }

    // ==========================================================================
    // OIIO-Compatible Methods
    // ==========================================================================

    /// Sets default channel names based on channel count.
    ///
    /// Mirrors OIIO's `ImageSpec::default_channel_names()`
    /// (`src/libOpenImageIO/formatspec.cpp`) exactly:
    ///
    /// - 1 channel: `["Y"]`, `alpha_channel`/`z_channel` stay `-1`.
    /// - 2 channels: `["R", "G"]` (OIIO does NOT special-case 2 channels as
    ///   luminance+alpha; only the 1-channel case is special).
    /// - 3 channels: `["R", "G", "B"]`.
    /// - 4 channels: `["R", "G", "B", "A"]`, `alpha_channel = 3`.
    /// - 5+ channels: `["R", "G", "B", "A", "channel4", "channel5", ...]`.
    ///
    /// `z_channel` is never inferred from names here (OIIO leaves it to the
    /// format reader / explicit metadata), so it is always reset to `-1`.
    pub fn default_channel_names(&mut self) {
        let n = self.nchannels;
        self.channel_names = Vec::with_capacity(n as usize);
        self.alpha_channel = -1;
        self.z_channel = -1;

        if n == 1 {
            // Special case: single channel is named "Y".
            self.channel_names.push("Y".into());
            return;
        }

        if n >= 1 {
            self.channel_names.push("R".into());
        }
        if n >= 2 {
            self.channel_names.push("G".into());
        }
        if n >= 3 {
            self.channel_names.push("B".into());
        }
        if n >= 4 {
            self.channel_names.push("A".into());
            self.alpha_channel = 3;
        }
        for i in 4..n {
            self.channel_names.push(format!("channel{}", i));
        }
    }

    /// The storage format of channel `chan`: its `channelformats` entry, else `format` (OIIO
    /// `ImageSpec::channelformat`, `imageio.h`: "safe even if channelformats is not filled out").
    #[inline]
    pub fn channelformat(&self, chan: usize) -> DataFormat {
        self.channelformats
            .get(chan)
            .copied()
            .unwrap_or(self.format)
    }

    /// Returns the number of bytes for a single channel in the given format.
    ///
    /// If `native` is true, uses per-channel formats; otherwise uses default format.
    #[inline]
    pub fn channel_bytes(&self, chan: usize, native: bool) -> usize {
        if native && chan < self.channelformats.len() {
            self.channelformats[chan].bytes_per_channel()
        } else {
            self.format.bytes_per_channel()
        }
    }

    /// Returns the number of bytes for a single scanline (row).
    ///
    /// If `native` is true, uses per-channel formats.
    ///
    /// Saturates at `usize::MAX` on overflow rather than wrapping, matching
    /// OIIO's `clamped_mult64` (`formatspec.cpp::scanline_bytes`). A wrapped
    /// (silently small) result here is exactly the failure mode that turns a
    /// corrupt or hostile image header's huge `width` into an under-sized
    /// allocation followed by a buffer overflow when the real pixel count is
    /// written into it — see [`Self::size_t_safe`], which callers should check
    /// before trusting this value for an allocation.
    #[inline]
    pub fn scanline_bytes(&self, native: bool) -> usize {
        (self.width as usize).saturating_mul(self.pixel_bytes(native))
    }

    /// Returns the number of bytes for a single pixel.
    ///
    /// If `native` is true and per-channel formats exist, sums their sizes.
    #[inline]
    pub fn pixel_bytes(&self, native: bool) -> usize {
        if native && !self.channelformats.is_empty() {
            self.channelformats
                .iter()
                .map(|f| f.bytes_per_channel())
                .sum()
        } else {
            self.nchannels as usize * self.format.bytes_per_channel()
        }
    }

    /// Returns the number of pixels per tile.
    ///
    /// Returns 0 if the image is not tiled. Saturates at `u64::MAX` on
    /// overflow rather than wrapping — see [`Self::scanline_bytes`].
    #[inline]
    pub fn tile_pixels(&self) -> u64 {
        if self.tile_width == 0 || self.tile_height == 0 {
            0
        } else {
            let d = self.tile_depth.max(1) as u64;
            (self.tile_width as u64)
                .saturating_mul(self.tile_height as u64)
                .saturating_mul(d)
        }
    }

    /// Returns the number of bytes per tile.
    ///
    /// Returns 0 if the image is not tiled. Saturates at `usize::MAX` on
    /// overflow rather than wrapping — see [`Self::scanline_bytes`].
    #[inline]
    pub fn tile_bytes(&self, native: bool) -> usize {
        (self.tile_pixels() as usize).saturating_mul(self.pixel_bytes(native))
    }

    /// Returns the total number of bytes for the entire image.
    ///
    /// Saturates at `u64::MAX` on overflow rather than wrapping, matching
    /// OIIO's `clamped_mult64` (`formatspec.cpp::image_bytes`) — see
    /// [`Self::scanline_bytes`] for why this matters, and check
    /// [`Self::size_t_safe`] before using this to size an allocation.
    #[inline]
    pub fn image_bytes(&self, native: bool) -> u64 {
        self.image_pixels()
            .saturating_mul(self.pixel_bytes(native) as u64)
    }

    /// Returns the total number of pixels in the image.
    ///
    /// Saturates at `u64::MAX` on overflow rather than wrapping — a corrupt
    /// or hostile header's `width`/`height`/`depth` (each up to `u32::MAX`)
    /// can overflow a plain `u64` product (their true product needs up to
    /// 96 bits), so an unchecked multiply here would silently wrap to a
    /// small pixel count instead of the huge one the header actually claims.
    #[inline]
    pub fn image_pixels(&self) -> u64 {
        (self.width as u64)
            .saturating_mul(self.height as u64)
            .saturating_mul(self.depth as u64)
    }

    /// Returns true if all size calculations are safe (won't overflow).
    ///
    /// Deliberately does NOT just compare [`Self::image_bytes`] against
    /// `usize::MAX`: on any 64-bit target `usize::MAX == u64::MAX`, so once
    /// `image_bytes` saturates on overflow (see its doc comment) that
    /// comparison becomes `u64::MAX <= u64::MAX`, always true — the exact
    /// overflow case this function exists to catch would silently report
    /// itself as safe. Instead this recomputes the same product with
    /// `checked_mul`, which distinguishes "overflowed" (`None`) from "the
    /// real value happens to be `u64::MAX`" (`Some(u64::MAX)`, not reachable
    /// in practice, but not conflated with the overflow case either).
    #[inline]
    pub fn size_t_safe(&self) -> bool {
        let pixels = (self.width as u64)
            .checked_mul(self.height as u64)
            .and_then(|p| p.checked_mul(self.depth as u64));
        let Some(pixels) = pixels else {
            return false;
        };
        match pixels.checked_mul(self.pixel_bytes(false) as u64) {
            Some(bytes) => bytes <= usize::MAX as u64,
            None => false,
        }
    }

    /// Computes automatic strides for pixel data.
    ///
    /// Returns (x_stride, y_stride, z_stride) in bytes. Saturates at
    /// `usize::MAX` on overflow rather than wrapping — see
    /// [`Self::scanline_bytes`].
    pub fn auto_stride(&self, native: bool) -> (usize, usize, usize) {
        let pixel_size = self.pixel_bytes(native);
        let x_stride = pixel_size;
        let y_stride = (self.width as usize).saturating_mul(pixel_size);
        let z_stride = y_stride.saturating_mul(self.height as usize);
        (x_stride, y_stride, z_stride)
    }

    /// Checks if a tile range is valid for this image.
    ///
    /// Returns true if the tile coordinates are within the tiled region.
    pub fn valid_tile_range(&self, x: i32, y: i32, z: i32) -> bool {
        if self.tile_width == 0 || self.tile_height == 0 {
            return false;
        }

        let tw = self.tile_width as i32;
        let th = self.tile_height as i32;
        let td = self.tile_depth.max(1) as i32;

        x >= self.x
            && x < self.x + self.width as i32
            && (x - self.x) % tw == 0
            && y >= self.y
            && y < self.y + self.height as i32
            && (y - self.y) % th == 0
            && z >= self.z
            && z < self.z + self.depth as i32
            && (z - self.z) % td == 0
    }

    /// Copies dimensions from another ImageSpec.
    ///
    /// Copies width, height, depth, x, y, z, full_* dimensions,
    /// tile dimensions, and nchannels (but not format or metadata).
    pub fn copy_dimensions(&mut self, other: &ImageSpec) {
        self.width = other.width;
        self.height = other.height;
        self.depth = other.depth;
        self.x = other.x;
        self.y = other.y;
        self.z = other.z;
        self.full_width = other.full_width;
        self.full_height = other.full_height;
        self.full_depth = other.full_depth;
        self.full_x = other.full_x;
        self.full_y = other.full_y;
        self.full_z = other.full_z;
        self.tile_width = other.tile_width;
        self.tile_height = other.tile_height;
        self.tile_depth = other.tile_depth;
    }

    /// Sets the format for all channels.
    pub fn set_format(&mut self, format: DataFormat) {
        self.format = format;
        self.channelformats.clear();
    }

    /// Sets the `oiio:ColorSpace` attribute and nothing else. OIIO's `set_colorspace` (which also
    /// erases contradicting attributes) and its names are `vfx_io::Metadata::set_colorspace` and
    /// `vfx_io::colorspace`.
    pub fn set_colorspace(&mut self, colorspace: &str) {
        self.set_attr("oiio:ColorSpace", colorspace);
    }

    /// Gets the colorspace metadata attribute.
    pub fn get_colorspace(&self) -> Option<&str> {
        self.get_string("oiio:ColorSpace")
    }

    /// Returns the TypeDesc for the primary format.
    pub fn format_typedesc(&self) -> TypeDesc {
        TypeDesc::from(self.format)
    }

    /// Returns the TypeDesc for a specific channel.
    pub fn channel_typedesc(&self, chan: usize) -> TypeDesc {
        if chan < self.channelformats.len() {
            TypeDesc::from(self.channelformats[chan])
        } else {
            TypeDesc::from(self.format)
        }
    }

    /// Returns true if the spec represents an undefined/invalid image.
    #[inline]
    pub fn undefined(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Returns the number of bytes per pixel.
    ///
    /// This is `nchannels * bytes_per_channel`.
    #[inline]
    pub fn bytes_per_pixel(&self) -> usize {
        self.nchannels as usize * self.format.bytes_per_channel()
    }

    /// Returns the number of bytes per scanline (row).
    ///
    /// This is `width * bytes_per_pixel`.
    #[inline]
    pub fn bytes_per_row(&self) -> usize {
        self.width as usize * self.bytes_per_pixel()
    }

    /// Returns the total number of pixels in the image.
    #[inline]
    pub fn pixel_count(&self) -> u64 {
        self.width as u64 * self.height as u64 * self.depth as u64
    }

    /// Returns the total size of pixel data in bytes.
    ///
    /// This is the minimum buffer size needed to store all pixels.
    #[inline]
    pub fn data_size(&self) -> usize {
        self.pixel_count() as usize * self.bytes_per_pixel()
    }

    /// Returns `true` if the image has an alpha channel.
    ///
    /// Uses the `alpha_channel` field if set, otherwise OIIO's name rule
    /// ([`crate::channels::is_alpha_channel_name`]: suffix `A`/`Alpha`, so `diffuse.A` counts).
    pub fn has_alpha(&self) -> bool {
        self.get_alpha_channel().is_some()
    }

    /// Returns the index of the alpha channel, if present.
    ///
    /// Uses the `alpha_channel` field if set, otherwise OIIO's name rule
    /// ([`crate::channels::find_alpha_channel`]).
    pub fn get_alpha_channel(&self) -> Option<usize> {
        if self.alpha_channel >= 0 {
            return Some(self.alpha_channel as usize);
        }
        crate::channels::find_alpha_channel(&self.channel_names)
    }

    /// Returns the index of the depth/Z channel, if present.
    ///
    /// Uses the `z_channel` field if set, otherwise OIIO's name rule
    /// ([`crate::channels::find_z_channel`]: suffix `Z`/`Depth`).
    pub fn get_z_channel(&self) -> Option<usize> {
        if self.z_channel >= 0 {
            return Some(self.z_channel as usize);
        }
        crate::channels::find_z_channel(&self.channel_names)
    }

    /// Sets `alpha_channel` and `z_channel` from `channel_names` by OIIO's name rule, as the
    /// OpenEXR reader does (`exrinput.cpp` `query_channels`); `-1` when no name matches.
    pub fn set_alpha_z_from_names(&mut self) {
        self.alpha_channel = crate::channels::alpha_channel_index(&self.channel_names);
        self.z_channel = crate::channels::z_channel_index(&self.channel_names);
    }

    /// The index of the channel named exactly `name` (OIIO `ImageSpec::channelindex`,
    /// `formatspec.cpp`: an exact, case-sensitive comparison).
    pub fn channelindex(&self, name: &str) -> Option<usize> {
        self.channel_names.iter().position(|n| n == name)
    }

    /// Returns the channel name for an index, if available.
    pub fn channel_name(&self, index: usize) -> Option<&str> {
        self.channel_names.get(index).map(|s| s.as_str())
    }

    /// Sets an attribute value.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::ImageSpec;
    ///
    /// let mut spec = ImageSpec::rgb(1920, 1080);
    /// spec.set_attr("Author", "VFX Artist");
    /// spec.set_attr("FrameRate", 24);
    /// spec.set_attr("ExposureTime", 0.041667);
    /// ```
    pub fn set_attr(&mut self, key: impl Into<String>, value: impl Into<AttrValue>) {
        self.attributes.insert(key.into(), value.into());
    }

    /// Gets an attribute value by key.
    pub fn get_attr(&self, key: &str) -> Option<&AttrValue> {
        self.attributes.get(key)
    }

    /// Gets an attribute as a string.
    pub fn get_string(&self, key: &str) -> Option<&str> {
        self.get_attr(key).and_then(|v| v.as_str())
    }

    /// Gets an attribute as an integer.
    pub fn get_int(&self, key: &str) -> Option<i64> {
        self.get_attr(key).and_then(|v| v.as_int())
    }

    /// Gets an attribute as a float.
    pub fn get_float(&self, key: &str) -> Option<f64> {
        self.get_attr(key).and_then(|v| v.as_float())
    }

    /// Gets an attribute as an integer with a default value.
    pub fn get_int_attribute(&self, key: &str, default: i64) -> i64 {
        self.get_int(key).unwrap_or(default)
    }

    /// Gets an attribute as a float with a default value.
    pub fn get_float_attribute(&self, key: &str, default: f64) -> f64 {
        self.get_float(key).unwrap_or(default)
    }

    /// Gets an attribute as a string with a default value.
    pub fn get_string_attribute(&self, key: &str, default: &str) -> String {
        self.get_string(key)
            .map(|s| s.to_string())
            .unwrap_or_else(|| default.to_string())
    }

    /// Removes an attribute by key.
    ///
    /// Returns `true` if the attribute existed and was removed.
    pub fn erase_attribute(&mut self, key: &str) -> bool {
        self.attributes.remove(key).is_some()
    }

    /// Returns the type of an attribute.
    pub fn getattributetype(&self, key: &str) -> Option<TypeDesc> {
        self.get_attr(key).map(|v| match v.value() {
            AttrValue::Float(_) if v.type_tag() == Some("float") => TypeDesc::FLOAT,
            AttrValue::Int(_) => TypeDesc::INT64,
            AttrValue::Float(_) => TypeDesc::DOUBLE,
            AttrValue::String(_) => TypeDesc::STRING,
            AttrValue::IntArray(arr) => TypeDesc::INT64.array(arr.len() as i32),
            AttrValue::FloatArray(arr) => TypeDesc::DOUBLE.array(arr.len() as i32),
            AttrValue::StringArray(arr) => TypeDesc::STRING.array(arr.len() as i32),
            AttrValue::Matrix3(_) => TypeDesc::matrix33(),
            AttrValue::Matrix4(_) => TypeDesc::matrix44(),
            // OIIO's own convention for opaque blobs (e.g. `"ICCProfile"`) is an unsigned char
            // array of the blob's length, not a dedicated type code.
            AttrValue::Bytes(b) => TypeDesc::UINT8.array(b.len() as i32),
            // `value()` never returns a wrapper; the arm exists for the non-exhaustive enum.
            AttrValue::Typed { .. } => TypeDesc::UNKNOWN,
        })
    }

    /// Returns `true` if data and display windows differ.
    ///
    /// Indicates the image has overscan or is a crop.
    #[inline]
    pub fn has_overscan(&self) -> bool {
        self.x != self.full_x
            || self.y != self.full_y
            || self.z != self.full_z
            || self.width != self.full_width
            || self.height != self.full_height
            || self.depth != self.full_depth
    }

    /// Returns true if the image is tiled.
    #[inline]
    pub fn is_tiled(&self) -> bool {
        self.tile_width > 0 && self.tile_height > 0
    }

    /// Returns `true` if this is a 3D (volumetric) texture.
    #[inline]
    pub fn is_3d(&self) -> bool {
        self.depth > 1
    }

    /// Creates a copy with a different format.
    pub fn with_format(&self, format: DataFormat) -> Self {
        let mut spec = self.clone();
        spec.format = format;
        spec.channelformats.clear();
        spec
    }

    /// Creates a copy with different dimensions.
    #[allow(deprecated)]
    pub fn with_size(&self, width: u32, height: u32) -> Self {
        let mut spec = self.clone();
        spec.width = width;
        spec.height = height;
        spec.full_width = width;
        spec.full_height = height;
        spec.data_window = Rect::from_size(width, height);
        spec.display_window = Rect::from_size(width, height);
        spec
    }

    /// Creates a ROI from the image dimensions.
    pub fn roi(&self) -> crate::Roi3D {
        crate::Roi3D::new(
            self.x..self.x + self.width as i32,
            self.y..self.y + self.height as i32,
            self.z..self.z + self.depth as i32,
            0..self.nchannels as i32,
        )
    }

    /// Creates a ROI from the full/display dimensions.
    pub fn roi_full(&self) -> crate::Roi3D {
        crate::Roi3D::new(
            self.full_x..self.full_x + self.full_width as i32,
            self.full_y..self.full_y + self.full_height as i32,
            self.full_z..self.full_z + self.full_depth as i32,
            0..self.nchannels as i32,
        )
    }

    /// Sets dimensions from a ROI.
    pub fn set_roi(&mut self, roi: &crate::Roi3D) {
        self.x = roi.xbegin;
        self.y = roi.ybegin;
        self.z = roi.zbegin;
        self.width = roi.width() as u32;
        self.height = roi.height() as u32;
        self.depth = roi.depth() as u32;
    }

    /// Sets full/display dimensions from a ROI.
    pub fn set_roi_full(&mut self, roi: &crate::Roi3D) {
        self.full_x = roi.xbegin;
        self.full_y = roi.ybegin;
        self.full_z = roi.zbegin;
        self.full_width = roi.width() as u32;
        self.full_height = roi.height() as u32;
        self.full_depth = roi.depth() as u32;
    }

    // =========================================================================
    // Metadata Value Formatting (OIIO Parity)
    // =========================================================================

    /// Returns a formatted string representation of a metadata attribute.
    ///
    /// This provides OIIO-compatible metadata formatting, suitable for display.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::ImageSpec;
    ///
    /// let mut spec = ImageSpec::rgba(1920, 1080);
    /// spec.set_attr("Author", "John Doe");
    /// spec.set_attr("FrameRate", 24);
    ///
    /// assert_eq!(spec.metadata_val("Author"), Some("\"John Doe\"".to_string()));
    /// assert_eq!(spec.metadata_val("FrameRate"), Some("24".to_string()));
    /// ```
    pub fn metadata_val(&self, key: &str) -> Option<String> {
        self.get_attr(key).map(|v| v.to_string())
    }

    /// Returns iterator over all attribute names.
    pub fn attribute_names(&self) -> impl Iterator<Item = &str> {
        self.attributes.keys().map(|s| s.as_str())
    }

    /// Returns the number of attributes.
    pub fn attribute_count(&self) -> usize {
        self.attributes.len()
    }

    /// Finds an attribute by name pattern (glob-style).
    ///
    /// Supports simple wildcards: `*` matches any sequence, `?` matches single char.
    pub fn find_attribute(&self, pattern: &str) -> Option<(&str, &AttrValue)> {
        for (key, value) in &self.attributes {
            if Self::glob_match(pattern, key) {
                return Some((key.as_str(), value));
            }
        }
        None
    }

    /// Returns all attributes matching a pattern.
    pub fn find_attributes(&self, pattern: &str) -> Vec<(&str, &AttrValue)> {
        self.attributes
            .iter()
            .filter(|(key, _)| Self::glob_match(pattern, key))
            .map(|(k, v)| (k.as_str(), v))
            .collect()
    }

    fn glob_match(pattern: &str, text: &str) -> bool {
        let mut p_chars = pattern.chars().peekable();
        let mut t_chars = text.chars().peekable();

        while let Some(pc) = p_chars.next() {
            match pc {
                '*' => {
                    // Try matching zero or more characters
                    let remaining_pattern: String = p_chars.collect();
                    let remaining_text: String = t_chars.collect();

                    for i in 0..=remaining_text.len() {
                        if Self::glob_match(&remaining_pattern, &remaining_text[i..]) {
                            return true;
                        }
                    }
                    return false;
                }
                '?' => {
                    if t_chars.next().is_none() {
                        return false;
                    }
                }
                c => {
                    if t_chars.next() != Some(c) {
                        return false;
                    }
                }
            }
        }

        t_chars.peek().is_none()
    }

    // =========================================================================
    // Serialization (OIIO Parity)
    // =========================================================================

    /// Serialization format: compact text.
    pub const SERIALIZE_TEXT: u32 = 0;
    /// Serialization format: verbose text with attributes.
    pub const SERIALIZE_TEXT_VERBOSE: u32 = 1;
    /// Serialization format: XML.
    pub const SERIALIZE_XML: u32 = 2;

    /// Serializes the ImageSpec to a formatted string.
    ///
    /// # Arguments
    ///
    /// * `format` - Serialization format (SERIALIZE_TEXT, SERIALIZE_TEXT_VERBOSE, or SERIALIZE_XML)
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::ImageSpec;
    ///
    /// let spec = ImageSpec::rgba(1920, 1080);
    /// let text = spec.serialize(ImageSpec::SERIALIZE_TEXT);
    /// assert!(text.contains("1920 x 1080"));
    /// ```
    pub fn serialize(&self, format: u32) -> String {
        match format {
            Self::SERIALIZE_XML => self.to_xml(),
            Self::SERIALIZE_TEXT_VERBOSE => self.to_text_verbose(),
            _ => self.to_text(),
        }
    }

    /// Serializes to compact text format.
    fn to_text(&self) -> String {
        let mut s = String::new();

        // Dimensions
        if self.depth > 1 {
            s.push_str(&format!(
                "{} x {} x {}",
                self.width, self.height, self.depth
            ));
        } else {
            s.push_str(&format!("{} x {}", self.width, self.height));
        }

        // Channels
        s.push_str(&format!(", {} channel", self.nchannels));
        if self.nchannels != 1 {
            s.push('s');
        }

        // Channel names if non-default
        if !self.channel_names.is_empty() {
            s.push_str(" (");
            s.push_str(&self.channel_names.join(", "));
            s.push(')');
        }

        // Format
        s.push_str(&format!(", {}", self.format));

        // Data/display window offset
        if self.x != 0 || self.y != 0 {
            s.push_str(&format!(", origin +{},+{}", self.x, self.y));
        }

        // Full/display window if different
        if self.has_overscan() {
            s.push_str(&format!(
                ", full/display window {} x {}",
                self.full_width, self.full_height
            ));
            if self.full_x != 0 || self.full_y != 0 {
                s.push_str(&format!(" +{},+{}", self.full_x, self.full_y));
            }
        }

        // Tiling
        if self.is_tiled() {
            s.push_str(&format!(
                ", {} x {} tiles",
                self.tile_width, self.tile_height
            ));
            if self.tile_depth > 1 {
                s.push_str(&format!(" x {}", self.tile_depth));
            }
        }

        // Deep
        if self.deep {
            s.push_str(", deep");
        }

        s
    }

    /// Serializes to verbose text format with all attributes.
    fn to_text_verbose(&self) -> String {
        let mut s = self.to_text();
        s.push('\n');

        // Add all attributes
        let mut keys: Vec<_> = self.attributes.keys().collect();
        keys.sort();

        for key in keys {
            if let Some(value) = self.attributes.get(key) {
                s.push_str(&format!("    {}: {}\n", key, value));
            }
        }

        s
    }

    /// Serializes the ImageSpec to XML format.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::ImageSpec;
    ///
    /// let spec = ImageSpec::rgba(1920, 1080);
    /// let xml = spec.to_xml();
    /// assert!(xml.contains("<ImageSpec>"));
    /// assert!(xml.contains("<width>1920</width>"));
    /// ```
    pub fn to_xml(&self) -> String {
        let mut xml = String::new();
        xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        xml.push_str("<ImageSpec>\n");

        // Core dimensions
        xml.push_str(&format!("  <width>{}</width>\n", self.width));
        xml.push_str(&format!("  <height>{}</height>\n", self.height));
        xml.push_str(&format!("  <depth>{}</depth>\n", self.depth));
        xml.push_str(&format!("  <x>{}</x>\n", self.x));
        xml.push_str(&format!("  <y>{}</y>\n", self.y));
        xml.push_str(&format!("  <z>{}</z>\n", self.z));

        // Full dimensions
        xml.push_str(&format!("  <full_width>{}</full_width>\n", self.full_width));
        xml.push_str(&format!(
            "  <full_height>{}</full_height>\n",
            self.full_height
        ));
        xml.push_str(&format!("  <full_depth>{}</full_depth>\n", self.full_depth));
        xml.push_str(&format!("  <full_x>{}</full_x>\n", self.full_x));
        xml.push_str(&format!("  <full_y>{}</full_y>\n", self.full_y));
        xml.push_str(&format!("  <full_z>{}</full_z>\n", self.full_z));

        // Tiles
        xml.push_str(&format!("  <tile_width>{}</tile_width>\n", self.tile_width));
        xml.push_str(&format!(
            "  <tile_height>{}</tile_height>\n",
            self.tile_height
        ));
        xml.push_str(&format!("  <tile_depth>{}</tile_depth>\n", self.tile_depth));

        // Channels
        xml.push_str(&format!("  <nchannels>{}</nchannels>\n", self.nchannels));
        xml.push_str(&format!("  <format>{}</format>\n", self.format));
        xml.push_str(&format!(
            "  <alpha_channel>{}</alpha_channel>\n",
            self.alpha_channel
        ));
        xml.push_str(&format!("  <z_channel>{}</z_channel>\n", self.z_channel));
        xml.push_str(&format!("  <deep>{}</deep>\n", self.deep));

        // Channel names
        if !self.channel_names.is_empty() {
            xml.push_str("  <channel_names>\n");
            for name in &self.channel_names {
                xml.push_str(&format!("    <name>{}</name>\n", Self::xml_escape(name)));
            }
            xml.push_str("  </channel_names>\n");
        }

        // Per-channel formats
        if !self.channelformats.is_empty() {
            xml.push_str("  <channelformats>\n");
            for fmt in &self.channelformats {
                xml.push_str(&format!("    <format>{}</format>\n", fmt));
            }
            xml.push_str("  </channelformats>\n");
        }

        // Attributes
        if !self.attributes.is_empty() {
            xml.push_str("  <attributes>\n");
            let mut keys: Vec<_> = self.attributes.keys().collect();
            keys.sort();
            for key in keys {
                if let Some(value) = self.attributes.get(key) {
                    xml.push_str(&format!(
                        "    <attrib name=\"{}\" type=\"{}\">{}</attrib>\n",
                        Self::xml_escape(key),
                        value.type_name(),
                        Self::xml_escape(&value.to_string().replace('"', ""))
                    ));
                }
            }
            xml.push_str("  </attributes>\n");
        }

        xml.push_str("</ImageSpec>\n");
        xml
    }

    /// Parses an ImageSpec from XML format.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::ImageSpec;
    ///
    /// let spec = ImageSpec::rgba(1920, 1080);
    /// let xml = spec.to_xml();
    /// let parsed = ImageSpec::from_xml(&xml).unwrap();
    /// assert_eq!(parsed.width, 1920);
    /// assert_eq!(parsed.height, 1080);
    /// ```
    pub fn from_xml(xml: &str) -> Result<Self, String> {
        let mut spec = ImageSpec::default();

        // Simple XML parsing - extract values between tags
        fn extract_value<T: std::str::FromStr>(xml: &str, tag: &str) -> Option<T> {
            let open_tag = format!("<{}>", tag);
            let close_tag = format!("</{}>", tag);
            let start = xml.find(&open_tag)? + open_tag.len();
            let end = xml[start..].find(&close_tag)? + start;
            xml[start..end].trim().parse().ok()
        }

        // Parse core dimensions
        if let Some(v) = extract_value::<u32>(xml, "width") {
            spec.width = v;
        }
        if let Some(v) = extract_value::<u32>(xml, "height") {
            spec.height = v;
        }
        if let Some(v) = extract_value::<u32>(xml, "depth") {
            spec.depth = v;
        }
        if let Some(v) = extract_value::<i32>(xml, "x") {
            spec.x = v;
        }
        if let Some(v) = extract_value::<i32>(xml, "y") {
            spec.y = v;
        }
        if let Some(v) = extract_value::<i32>(xml, "z") {
            spec.z = v;
        }

        // Parse full dimensions
        if let Some(v) = extract_value::<u32>(xml, "full_width") {
            spec.full_width = v;
        }
        if let Some(v) = extract_value::<u32>(xml, "full_height") {
            spec.full_height = v;
        }
        if let Some(v) = extract_value::<u32>(xml, "full_depth") {
            spec.full_depth = v;
        }
        if let Some(v) = extract_value::<i32>(xml, "full_x") {
            spec.full_x = v;
        }
        if let Some(v) = extract_value::<i32>(xml, "full_y") {
            spec.full_y = v;
        }
        if let Some(v) = extract_value::<i32>(xml, "full_z") {
            spec.full_z = v;
        }

        // Parse tile dimensions
        if let Some(v) = extract_value::<u32>(xml, "tile_width") {
            spec.tile_width = v;
        }
        if let Some(v) = extract_value::<u32>(xml, "tile_height") {
            spec.tile_height = v;
        }
        if let Some(v) = extract_value::<u32>(xml, "tile_depth") {
            spec.tile_depth = v;
        }

        // Parse channels
        if let Some(v) = extract_value::<u8>(xml, "nchannels") {
            spec.nchannels = v;
            #[allow(deprecated)]
            {
                spec.channels = v;
            }
        }
        if let Some(v) = extract_value::<i32>(xml, "alpha_channel") {
            spec.alpha_channel = v;
        }
        if let Some(v) = extract_value::<i32>(xml, "z_channel") {
            spec.z_channel = v;
        }

        // Parse format
        if let Some(fmt_str) = extract_value::<String>(xml, "format") {
            spec.format = match fmt_str.to_lowercase().as_str() {
                "u8" | "uint8" => DataFormat::U8,
                "u16" | "uint16" => DataFormat::U16,
                "u32" | "uint32" => DataFormat::U32,
                "f16" | "half" => DataFormat::F16,
                "f32" | "float" => DataFormat::F32,
                _ => DataFormat::F16,
            };
        }

        // Parse deep flag
        if let Some(deep_str) = extract_value::<String>(xml, "deep") {
            spec.deep = deep_str == "true" || deep_str == "1";
        }

        // Parse channel names
        if let Some(names_start) = xml.find("<channel_names>")
            && let Some(names_end) = xml.find("</channel_names>")
        {
            let names_section = &xml[names_start..names_end];
            let mut idx = 0;
            while let Some(start) = names_section[idx..].find("<name>") {
                let start = idx + start + 6;
                if let Some(end) = names_section[start..].find("</name>") {
                    let name = &names_section[start..start + end];
                    spec.channel_names.push(Self::xml_unescape(name));
                    idx = start + end;
                } else {
                    break;
                }
            }
        }

        // Parse attributes
        if let Some(attrs_start) = xml.find("<attributes>")
            && let Some(attrs_end) = xml.find("</attributes>")
        {
            let attrs_section = &xml[attrs_start..attrs_end];
            let mut idx = 0;
            while let Some(attrib_start) = attrs_section[idx..].find("<attrib ") {
                let start = idx + attrib_start;
                if let Some(attrib_end) = attrs_section[start..].find("</attrib>") {
                    let attrib = &attrs_section[start..start + attrib_end + 9];
                    if let (Some(name), Some(type_name), Some(value)) = (
                        Self::extract_attr(attrib, "name"),
                        Self::extract_attr(attrib, "type"),
                        Self::extract_content(attrib),
                    ) {
                        let attr_value = match type_name.as_str() {
                            "int" => value.parse::<i64>().ok().map(AttrValue::Int),
                            "float" => value.parse::<f64>().ok().map(AttrValue::Float),
                            "string" => Some(AttrValue::String(value)),
                            _ => Some(AttrValue::String(value)),
                        };
                        if let Some(v) = attr_value {
                            spec.attributes.insert(name, v);
                        }
                    }
                    idx = start + attrib_end + 9;
                } else {
                    break;
                }
            }
        }

        // Update data/display windows (deprecated fields, but kept for compatibility)
        #[allow(deprecated)]
        Self::update_deprecated_windows(&mut spec);

        Ok(spec)
    }

    #[allow(deprecated)]
    fn update_deprecated_windows(spec: &mut ImageSpec) {
        spec.data_window = Rect::new(spec.x as u32, spec.y as u32, spec.width, spec.height);
        spec.display_window = Rect::new(
            spec.full_x as u32,
            spec.full_y as u32,
            spec.full_width,
            spec.full_height,
        );
    }

    fn xml_escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }

    fn xml_unescape(s: &str) -> String {
        s.replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
    }

    fn extract_attr(xml: &str, attr: &str) -> Option<String> {
        let pattern = format!("{}=\"", attr);
        let start = xml.find(&pattern)? + pattern.len();
        let end = xml[start..].find('"')? + start;
        Some(Self::xml_unescape(&xml[start..end]))
    }

    fn extract_content(xml: &str) -> Option<String> {
        let start = xml.find('>')? + 1;
        let end = xml[start..].find('<')? + start;
        Some(Self::xml_unescape(xml[start..end].trim()))
    }
}

impl Default for ImageSpec {
    #[allow(deprecated)]
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            depth: 1,
            x: 0,
            y: 0,
            z: 0,
            full_width: 0,
            full_height: 0,
            full_depth: 1,
            full_x: 0,
            full_y: 0,
            full_z: 0,
            tile_width: 0,
            tile_height: 0,
            tile_depth: 0,
            nchannels: 0,
            format: DataFormat::default(),
            channelformats: Vec::new(),
            channel_names: Vec::new(),
            alpha_channel: -1,
            z_channel: -1,
            deep: false,
            data_window: Rect::default(),
            display_window: Rect::default(),
            channels: 0,
            attributes: HashMap::new(),
        }
    }
}

impl std::fmt::Display for ImageSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.depth > 1 {
            write!(
                f,
                "{}x{}x{} {} {}ch",
                self.width, self.height, self.depth, self.format, self.nchannels
            )
        } else {
            write!(
                f,
                "{}x{} {} {}ch",
                self.width, self.height, self.format, self.nchannels
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_format() {
        assert_eq!(DataFormat::U8.bytes_per_channel(), 1);
        assert_eq!(DataFormat::U16.bytes_per_channel(), 2);
        assert_eq!(DataFormat::F16.bytes_per_channel(), 2);
        assert_eq!(DataFormat::F32.bytes_per_channel(), 4);

        assert!(!DataFormat::U8.is_float());
        assert!(DataFormat::F16.is_float());
    }

    #[test]
    fn test_spec_new() {
        let spec = ImageSpec::new(1920, 1080, 4, DataFormat::F16);
        assert_eq!(spec.width, 1920);
        assert_eq!(spec.height, 1080);
        assert_eq!(spec.nchannels, 4);
        assert_eq!(spec.format, DataFormat::F16);
        // New OIIO-compatible fields
        assert_eq!(spec.x, 0);
        assert_eq!(spec.y, 0);
        assert_eq!(spec.full_width, 1920);
        assert_eq!(spec.full_height, 1080);
    }

    #[test]
    fn test_spec_rgb_rgba() {
        let rgb = ImageSpec::rgb(100, 100);
        assert_eq!(rgb.nchannels, 3);
        assert!(!rgb.has_alpha());

        let rgba = ImageSpec::rgba(100, 100);
        assert_eq!(rgba.nchannels, 4);
        assert!(rgba.has_alpha());
        assert_eq!(rgba.get_alpha_channel(), Some(3));
        assert_eq!(rgba.alpha_channel, 3);
    }

    #[test]
    fn test_spec_bytes() {
        let spec = ImageSpec::rgba(100, 100);
        assert_eq!(spec.bytes_per_pixel(), 8); // 4 * 2
        assert_eq!(spec.bytes_per_row(), 800);
        assert_eq!(spec.data_size(), 80000);
        // New methods
        assert_eq!(spec.pixel_bytes(false), 8);
        assert_eq!(spec.scanline_bytes(false), 800);
        assert_eq!(spec.image_bytes(false), 80000);
        assert_eq!(spec.image_pixels(), 10000);
    }

    #[test]
    fn test_spec_attributes() {
        let mut spec = ImageSpec::rgb(100, 100);
        spec.set_attr("Author", "Test");
        spec.set_attr("FrameRate", 24);
        spec.set_attr("ExposureTime", 0.041667);

        assert_eq!(spec.get_string("Author"), Some("Test"));
        assert_eq!(spec.get_int("FrameRate"), Some(24));
        assert!((spec.get_float("ExposureTime").unwrap() - 0.041667).abs() < 0.0001);

        // Test new attribute methods
        assert_eq!(spec.get_int_attribute("FrameRate", 0), 24);
        assert_eq!(spec.get_int_attribute("NonExistent", 42), 42);

        // Test erase_attribute
        assert!(spec.erase_attribute("Author"));
        assert!(!spec.erase_attribute("Author")); // Already removed
        assert!(spec.get_string("Author").is_none());
    }

    #[test]
    fn test_spec_overscan() {
        let mut spec = ImageSpec::rgba(1920, 1080);
        assert!(!spec.has_overscan());

        // Set overscan using new fields
        spec.full_width = 2048;
        spec.full_height = 1156;
        assert!(spec.has_overscan());
    }

    #[test]
    fn test_spec_tiling() {
        let mut spec = ImageSpec::rgba(1920, 1080);
        assert!(!spec.is_tiled());
        assert_eq!(spec.tile_pixels(), 0);

        spec.tile_width = 64;
        spec.tile_height = 64;
        spec.tile_depth = 1;
        assert!(spec.is_tiled());
        assert_eq!(spec.tile_pixels(), 64 * 64);
        assert_eq!(spec.tile_bytes(false), 64 * 64 * 8);
    }

    #[test]
    fn test_spec_auto_stride() {
        let spec = ImageSpec::rgba(100, 100);
        let (x, y, z) = spec.auto_stride(false);
        assert_eq!(x, 8); // pixel size
        assert_eq!(y, 800); // scanline
        assert_eq!(z, 80000); // full image
    }

    #[test]
    fn test_spec_roi() {
        let spec = ImageSpec::rgba(1920, 1080);
        let roi = spec.roi();
        assert_eq!(roi.width(), 1920);
        assert_eq!(roi.height(), 1080);
        assert_eq!(roi.nchannels(), 4);
    }

    #[test]
    fn test_spec_copy_dimensions() {
        let src = ImageSpec::rgba(1920, 1080);
        let mut dst = ImageSpec::default();
        dst.copy_dimensions(&src);
        assert_eq!(dst.width, 1920);
        assert_eq!(dst.height, 1080);
        assert_eq!(dst.nchannels, 0); // Not copied
    }

    #[test]
    fn test_spec_colorspace() {
        let mut spec = ImageSpec::rgba(100, 100);
        spec.set_colorspace("ACEScg");
        assert_eq!(spec.get_colorspace(), Some("ACEScg"));
    }

    /// `width`/`height`/`depth` come straight from a file header (u32, up to
    /// ~4.29e9 each), so their true product can need up to 96 bits. A plain
    /// `u64` multiply wraps instead of erroring in release builds, so a
    /// corrupt/hostile header could make `image_bytes()` silently report a
    /// tiny value -- exactly the setup for an under-sized allocation that a
    /// real pixel loop then overflows. Every size accessor must saturate
    /// (matching OIIO's `clamped_mult64`), and `size_t_safe()` must catch it.
    #[test]
    fn image_size_accessors_saturate_instead_of_wrapping_on_overflow() {
        let mut spec = ImageSpec::new(u32::MAX, u32::MAX, 4, DataFormat::F32);
        spec.depth = u32::MAX;

        assert_eq!(spec.image_pixels(), u64::MAX, "image_pixels must saturate");
        assert_eq!(
            spec.image_bytes(false),
            u64::MAX,
            "image_bytes must saturate"
        );
        // `scanline_bytes` only multiplies by `width` (a single u32-derived
        // factor), so u32::MAX pixels does not itself overflow a 64-bit
        // usize -- it is `image_bytes`/`image_pixels` (the product of all
        // three dimensions) that needs the guard. Still must be the real,
        // non-wrapped value, not silently wrapped to something small.
        assert_eq!(spec.scanline_bytes(false), 4_294_967_295usize * 16);
        assert!(
            !spec.size_t_safe(),
            "an overflowing spec must never report size_t_safe() == true"
        );

        // y_stride = width * pixel_size, same single-factor case as
        // scanline_bytes above: it is the real value, not wrapped, but not
        // large enough to need saturation itself. z_stride multiplies that
        // by height too, which DOES overflow and must saturate.
        let (_, y_stride, z_stride) = spec.auto_stride(false);
        assert_eq!(y_stride, 4_294_967_295usize * 16);
        assert_eq!(z_stride, usize::MAX, "auto_stride z_stride must saturate");

        let mut tiled = ImageSpec::new(1920, 1080, 4, DataFormat::F32);
        tiled.tile_width = u32::MAX;
        tiled.tile_height = u32::MAX;
        tiled.tile_depth = u32::MAX;
        assert_eq!(tiled.tile_pixels(), u64::MAX, "tile_pixels must saturate");
        assert_eq!(
            tiled.tile_bytes(false),
            usize::MAX,
            "tile_bytes must saturate"
        );

        // A normal, non-overflowing spec must still compute exact values.
        let normal = ImageSpec::new(1920, 1080, 4, DataFormat::F32);
        assert_eq!(normal.image_pixels(), 1920u64 * 1080);
        assert_eq!(normal.image_bytes(false), 1920u64 * 1080 * 4 * 4);
        assert!(normal.size_t_safe());
    }

    #[test]
    fn test_default_channel_names() {
        let mut spec = ImageSpec::new(100, 100, 4, DataFormat::F16);
        spec.default_channel_names();
        assert_eq!(spec.channel_names, vec!["R", "G", "B", "A"]);
        assert_eq!(spec.alpha_channel, 3);
    }

    /// OIIO's `ImageSpec::default_channel_names()` only special-cases
    /// nchannels == 1 ("Y"); 2 channels is ["R", "G"] with no alpha, NOT
    /// ["Y", "A"]. Regression test for a real divergence from the reference.
    #[test]
    fn test_default_channel_names_matches_oiio_reference() {
        let mut spec = ImageSpec::new(100, 100, 1, DataFormat::F16);
        spec.default_channel_names();
        assert_eq!(spec.channel_names, vec!["Y"]);
        assert_eq!(spec.alpha_channel, -1);
        assert_eq!(spec.z_channel, -1);

        let mut spec = ImageSpec::new(100, 100, 2, DataFormat::F16);
        spec.default_channel_names();
        assert_eq!(spec.channel_names, vec!["R", "G"]);
        assert_eq!(spec.alpha_channel, -1);

        let mut spec = ImageSpec::new(100, 100, 3, DataFormat::F16);
        spec.default_channel_names();
        assert_eq!(spec.channel_names, vec!["R", "G", "B"]);
        assert_eq!(spec.alpha_channel, -1);

        let mut spec = ImageSpec::new(100, 100, 6, DataFormat::F16);
        spec.default_channel_names();
        assert_eq!(
            spec.channel_names,
            vec!["R", "G", "B", "A", "channel4", "channel5"]
        );
        assert_eq!(spec.alpha_channel, 3);
    }
}
