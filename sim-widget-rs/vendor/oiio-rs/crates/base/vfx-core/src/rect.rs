//! Rectangle and Region of Interest (ROI) types for image operations.
//!
//! This module provides geometric primitives for defining image regions,
//! commonly used in:
//! - Cropping and padding operations
//! - Tile-based processing
//! - Region of Interest (ROI) selection
//! - Bounding box calculations
//!
//! # Overview
//!
//! - [`Rect`] - Basic rectangle with origin and dimensions
//! - [`Roi`] - Region of Interest, optionally unbounded
//!
//! # Coordinate System
//!
//! All coordinates use the standard image convention:
//! - Origin (0, 0) is at the **top-left** corner
//! - X increases to the right
//! - Y increases downward
//!
//! ```text
//! (0,0) ────────► X
//!   │
//!   │   ┌──────────┐
//!   │   │  Image   │
//!   │   │  Region  │
//!   │   └──────────┘
//!   ▼
//!   Y
//! ```
//!
//! # Usage
//!
//! ```rust
//! use vfx_core::Rect;
//!
//! // Create a rectangle at (10, 20) with size 100x50
//! let rect = Rect::new(10, 20, 100, 50);
//!
//! // Check if a point is inside
//! assert!(rect.contains(15, 25));
//! assert!(!rect.contains(5, 25));
//!
//! // Get intersection with another rectangle
//! let other = Rect::new(50, 40, 100, 50);
//! if let Some(intersection) = rect.intersect(&other) {
//!     println!("Overlap: {}x{}", intersection.width, intersection.height);
//! }
//! ```
//!
//! # Dependencies
//!
//! None (pure Rust types)
//!
//! # Used By
//!
//! - [`crate::image::Image`] - Crop, copy regions
//! - [`crate::image::ImageView`] - View into sub-region
//! - `vfx-io` - Display/data window specification

use core::ops::Range;

/// A rectangle defined by origin (x, y) and dimensions (width, height).
///
/// Represents a rectangular region in 2D image space. All values are
/// in pixels, with (0, 0) at the top-left corner.
///
/// # Invariants
///
/// - `width` and `height` should be > 0 for a valid rectangle
/// - A rectangle with zero width or height is considered empty
///
/// # Example
///
/// ```rust
/// use vfx_core::Rect;
///
/// let rect = Rect::new(10, 20, 100, 50);
/// assert_eq!(rect.right(), 110);
/// assert_eq!(rect.bottom(), 70);
/// assert_eq!(rect.area(), 5000);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(C)]
pub struct Rect {
    /// X coordinate of the left edge (inclusive)
    pub x: u32,
    /// Y coordinate of the top edge (inclusive)
    pub y: u32,
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
}

impl Rect {
    /// Creates a new rectangle with the given origin and dimensions.
    ///
    /// # Arguments
    ///
    /// * `x` - Left edge X coordinate
    /// * `y` - Top edge Y coordinate
    /// * `width` - Width in pixels
    /// * `height` - Height in pixels
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::Rect;
    ///
    /// let rect = Rect::new(10, 20, 100, 50);
    /// ```
    #[inline]
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Creates a rectangle from origin (0, 0) with given dimensions.
    ///
    /// Convenience constructor for full-image rectangles.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::Rect;
    ///
    /// let rect = Rect::from_size(1920, 1080);
    /// assert_eq!(rect.x, 0);
    /// assert_eq!(rect.y, 0);
    /// ```
    #[inline]
    pub const fn from_size(width: u32, height: u32) -> Self {
        Self::new(0, 0, width, height)
    }

    /// Creates a rectangle from two corner points.
    ///
    /// The points are (x1, y1) top-left and (x2, y2) bottom-right.
    /// If coordinates are swapped, they will be normalized.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::Rect;
    ///
    /// let rect = Rect::from_corners(10, 20, 110, 70);
    /// assert_eq!(rect.width, 100);
    /// assert_eq!(rect.height, 50);
    /// ```
    #[inline]
    pub fn from_corners(x1: u32, y1: u32, x2: u32, y2: u32) -> Self {
        let (min_x, max_x) = if x1 <= x2 { (x1, x2) } else { (x2, x1) };
        let (min_y, max_y) = if y1 <= y2 { (y1, y2) } else { (y2, y1) };
        Self::new(min_x, min_y, max_x - min_x, max_y - min_y)
    }

    /// Returns the X coordinate of the right edge (exclusive).
    ///
    /// This is `x + width`, representing the first column NOT in the rectangle.
    #[inline]
    pub const fn right(&self) -> u32 {
        self.x + self.width
    }

    /// Returns the Y coordinate of the bottom edge (exclusive).
    ///
    /// This is `y + height`, representing the first row NOT in the rectangle.
    #[inline]
    pub const fn bottom(&self) -> u32 {
        self.y + self.height
    }

    /// Returns the area of the rectangle in pixels.
    #[inline]
    pub const fn area(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// Returns `true` if the rectangle has zero area.
    ///
    /// A rectangle is empty if either dimension is zero.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Returns `true` if the point (px, py) is inside this rectangle.
    ///
    /// The rectangle is inclusive on the left/top edges and exclusive
    /// on the right/bottom edges.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::Rect;
    ///
    /// let rect = Rect::new(10, 10, 100, 100);
    /// assert!(rect.contains(10, 10));   // Top-left corner included
    /// assert!(rect.contains(109, 109)); // Just inside
    /// assert!(!rect.contains(110, 110)); // On right/bottom edge, excluded
    /// ```
    #[inline]
    pub const fn contains(&self, px: u32, py: u32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }

    /// Returns `true` if this rectangle fully contains another.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::Rect;
    ///
    /// let outer = Rect::new(0, 0, 100, 100);
    /// let inner = Rect::new(10, 10, 50, 50);
    /// assert!(outer.contains_rect(&inner));
    /// ```
    #[inline]
    pub const fn contains_rect(&self, other: &Rect) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && other.right() <= self.right()
            && other.bottom() <= self.bottom()
    }

    /// Returns `true` if this rectangle overlaps with another.
    ///
    /// Two rectangles overlap if they share at least one pixel.
    /// Empty rectangles never overlap.
    #[inline]
    pub const fn overlaps(&self, other: &Rect) -> bool {
        if self.is_empty() || other.is_empty() {
            return false;
        }
        self.x < other.right()
            && self.right() > other.x
            && self.y < other.bottom()
            && self.bottom() > other.y
    }

    /// Returns the intersection of this rectangle with another.
    ///
    /// Returns `None` if the rectangles don't overlap.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::Rect;
    ///
    /// let a = Rect::new(0, 0, 100, 100);
    /// let b = Rect::new(50, 50, 100, 100);
    /// let intersection = a.intersect(&b).unwrap();
    /// assert_eq!(intersection, Rect::new(50, 50, 50, 50));
    /// ```
    #[inline]
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());

        if x < right && y < bottom {
            Some(Rect::new(x, y, right - x, bottom - y))
        } else {
            None
        }
    }

    /// Returns the bounding box that contains both rectangles.
    ///
    /// Also known as the union rectangle.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::Rect;
    ///
    /// let a = Rect::new(0, 0, 50, 50);
    /// let b = Rect::new(100, 100, 50, 50);
    /// let union = a.union(&b);
    /// assert_eq!(union, Rect::new(0, 0, 150, 150));
    /// ```
    #[inline]
    pub fn union(&self, other: &Rect) -> Rect {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());
        Rect::new(x, y, right - x, bottom - y)
    }

    /// Returns this rectangle translated by (dx, dy).
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::Rect;
    ///
    /// let rect = Rect::new(10, 20, 100, 50);
    /// let moved = rect.translate(5, 10);
    /// assert_eq!(moved, Rect::new(15, 30, 100, 50));
    /// ```
    #[inline]
    pub const fn translate(&self, dx: i32, dy: i32) -> Rect {
        // Saturating arithmetic to prevent underflow wrapping (clamp to 0)
        let new_x = (self.x as i32).saturating_add(dx);
        let new_y = (self.y as i32).saturating_add(dy);
        Rect::new(
            if new_x < 0 { 0 } else { new_x as u32 },
            if new_y < 0 { 0 } else { new_y as u32 },
            self.width,
            self.height,
        )
    }

    /// Returns this rectangle with inset (shrunk) edges.
    ///
    /// Positive values shrink the rectangle, negative values expand it.
    /// Returns `None` if the inset would make the rectangle invalid.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::Rect;
    ///
    /// let rect = Rect::new(0, 0, 100, 100);
    /// let inset = rect.inset(10).unwrap();
    /// assert_eq!(inset, Rect::new(10, 10, 80, 80));
    /// ```
    #[inline]
    pub fn inset(&self, amount: i32) -> Option<Rect> {
        let double = amount * 2;
        let new_width = self.width as i32 - double;
        let new_height = self.height as i32 - double;

        if new_width <= 0 || new_height <= 0 {
            return None;
        }

        // Saturating arithmetic to prevent underflow wrapping (clamp to 0)
        let new_x = (self.x as i32).saturating_add(amount);
        let new_y = (self.y as i32).saturating_add(amount);
        Some(Rect::new(
            if new_x < 0 { 0 } else { new_x as u32 },
            if new_y < 0 { 0 } else { new_y as u32 },
            new_width as u32,
            new_height as u32,
        ))
    }

    /// Clamps this rectangle to fit within bounds.
    ///
    /// Returns the portion of this rectangle that fits within the given
    /// bounds, or `None` if there's no overlap.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::Rect;
    ///
    /// let rect = Rect::new(900, 500, 200, 200);
    /// let clamped = rect.clamp_to(1920, 1080).unwrap();
    /// // Rect is clamped to image bounds
    /// assert!(clamped.right() <= 1920);
    /// assert!(clamped.bottom() <= 1080);
    /// ```
    #[inline]
    pub fn clamp_to(&self, max_width: u32, max_height: u32) -> Option<Rect> {
        let bounds = Rect::from_size(max_width, max_height);
        self.intersect(&bounds)
    }

    /// Returns an iterator over all (x, y) coordinates in this rectangle.
    ///
    /// Iterates row by row, left to right, top to bottom.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::Rect;
    ///
    /// let rect = Rect::new(0, 0, 2, 2);
    /// let coords: Vec<_> = rect.iter_coords().collect();
    /// assert_eq!(coords, vec![(0, 0), (1, 0), (0, 1), (1, 1)]);
    /// ```
    #[inline]
    pub fn iter_coords(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        (self.y..self.bottom()).flat_map(move |y| (self.x..self.right()).map(move |x| (x, y)))
    }
}

impl std::fmt::Display for Rect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Rect({}, {}, {}x{})",
            self.x, self.y, self.width, self.height
        )
    }
}

/// Region of Interest - optionally unbounded rectangle.
///
/// Unlike [`Rect`], an ROI can represent "the entire image" without
/// knowing the image dimensions. This is useful for operations that
/// should apply to the full image by default.
///
/// # States
///
/// - `Roi::Full` - Represents the entire image (unbounded)
/// - `Roi::Region(Rect)` - A specific bounded region
///
/// # Example
///
/// ```rust
/// use vfx_core::{Rect, Roi};
///
/// // Process entire image
/// let roi = Roi::Full;
///
/// // Process specific region
/// let roi = Roi::Region(Rect::new(100, 100, 500, 500));
///
/// // Resolve to actual bounds
/// let actual = roi.resolve(1920, 1080);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Roi {
    /// The entire image (unbounded)
    #[default]
    Full,
    /// A specific rectangular region
    Region(Rect),
}

impl Roi {
    /// Creates an ROI covering the full image.
    #[inline]
    pub const fn full() -> Self {
        Self::Full
    }

    /// Creates an ROI from a specific rectangle.
    #[inline]
    pub const fn region(rect: Rect) -> Self {
        Self::Region(rect)
    }

    /// Creates an ROI from coordinates and dimensions.
    #[inline]
    pub const fn from_xywh(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self::Region(Rect::new(x, y, width, height))
    }

    /// Returns `true` if this ROI represents the full image.
    #[inline]
    pub const fn is_full(&self) -> bool {
        matches!(self, Self::Full)
    }

    /// Resolves this ROI to an actual [`Rect`] given image dimensions.
    ///
    /// - `Roi::Full` resolves to `Rect::from_size(width, height)`
    /// - `Roi::Region(r)` returns `r` clamped to image bounds
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::{Rect, Roi};
    ///
    /// let roi = Roi::Full;
    /// assert_eq!(roi.resolve(1920, 1080), Rect::from_size(1920, 1080));
    ///
    /// let roi = Roi::from_xywh(100, 100, 500, 500);
    /// let resolved = roi.resolve(1920, 1080);
    /// assert_eq!(resolved, Rect::new(100, 100, 500, 500));
    /// ```
    #[inline]
    pub fn resolve(&self, width: u32, height: u32) -> Rect {
        match self {
            Self::Full => Rect::from_size(width, height),
            Self::Region(r) => r.clamp_to(width, height).unwrap_or(Rect::default()),
        }
    }
}

impl From<Rect> for Roi {
    fn from(rect: Rect) -> Self {
        Roi::Region(rect)
    }
}

impl std::fmt::Display for Roi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Roi::Full => write!(f, "Roi::Full"),
            Roi::Region(r) => write!(f, "Roi::{}", r),
        }
    }
}

// =============================================================================
// OIIO-Compatible ROI (Region of Interest) - Full 3D with channel bounds
// =============================================================================

/// Full 3D Region of Interest with channel bounds (matches OIIO ROI).
///
/// Unlike the simpler [`Roi`], this provides complete OIIO compatibility
/// with X, Y, Z dimensions and channel range specification.
///
/// # Coordinate Convention
///
/// All ranges are half-open intervals: [begin, end) - the begin is included,
/// the end is excluded.
///
/// # Example
///
/// ```rust
/// use vfx_core::Roi3D;
///
/// // Define a region: x=[100,200), y=[50,150), z=[0,1), channels=[0,4)
/// let roi = Roi3D::new(100..200, 50..150, 0..1, 0..4);
///
/// assert_eq!(roi.width(), 100);
/// assert_eq!(roi.height(), 100);
/// assert!(roi.contains(150, 100, 0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Roi3D {
    /// X begin (inclusive)
    pub xbegin: i32,
    /// X end (exclusive)
    pub xend: i32,
    /// Y begin (inclusive)
    pub ybegin: i32,
    /// Y end (exclusive)
    pub yend: i32,
    /// Z begin (inclusive, for 3D/volumetric images)
    pub zbegin: i32,
    /// Z end (exclusive)
    pub zend: i32,
    /// Channel begin (inclusive)
    pub chbegin: i32,
    /// Channel end (exclusive)
    pub chend: i32,
}

impl Default for Roi3D {
    fn default() -> Self {
        Self::all()
    }
}

impl Roi3D {
    /// Creates a new ROI from its four half-open `[begin, end)` ranges: x, y, z and channels.
    ///
    /// Reference: OIIO `ROI(xbegin, xend, ybegin, yend, zbegin, zend, chbegin, chend)`; each
    /// axis's begin/end pair is one [`Range`] here.
    #[inline]
    pub const fn new(x: Range<i32>, y: Range<i32>, z: Range<i32>, ch: Range<i32>) -> Self {
        Self {
            xbegin: x.start,
            xend: x.end,
            ybegin: y.start,
            yend: y.end,
            zbegin: z.start,
            zend: z.end,
            chbegin: ch.start,
            chend: ch.end,
        }
    }

    /// Creates a 2D ROI (z = [0,1), all channels).
    ///
    /// `chend` defaults to [`Self::CHEND_ALL`] rather than `i32::MAX`: callers routinely compute
    /// `roi.nchannels()` as `chend - chbegin`, and `i32::MAX - chbegin` overflows.
    #[inline]
    pub const fn new_2d(xbegin: i32, xend: i32, ybegin: i32, yend: i32) -> Self {
        Self::new(xbegin..xend, ybegin..yend, 0..1, 0..Self::CHEND_ALL)
    }

    /// Creates a 2D ROI with specific channel range.
    #[inline]
    pub const fn new_2d_with_channels(
        xbegin: i32,
        xend: i32,
        ybegin: i32,
        yend: i32,
        chbegin: i32,
        chend: i32,
    ) -> Self {
        Self::new(xbegin..xend, ybegin..yend, 0..1, chbegin..chend)
    }

    /// Creates a ROI from width and height (origin at 0,0).
    #[inline]
    pub const fn from_size(width: i32, height: i32) -> Self {
        Self::new_2d(0, width, 0, height)
    }

    /// Open-ended channel count, matching the reference's default `chend`.
    ///
    /// A deliberately large but *arithmetic-safe* sentinel: `chend - chbegin` must not overflow,
    /// which rules out `i32::MAX`. Consumers are expected to clamp with the buffer's real channel
    /// count (`min(buf.nchannels(), roi.nchannels())`).
    pub const CHEND_ALL: i32 = 10000;

    /// Creates an "all" ROI that matches everything.
    ///
    /// This is used to indicate "entire image" without knowing dimensions.
    #[inline]
    pub const fn all() -> Self {
        Self {
            xbegin: i32::MIN,
            xend: i32::MAX,
            ybegin: i32::MIN,
            yend: i32::MAX,
            zbegin: i32::MIN,
            zend: i32::MAX,
            chbegin: 0,
            chend: Self::CHEND_ALL,
        }
    }

    /// Returns true if this ROI represents "all" (undefined bounds).
    ///
    /// Keyed on `xbegin` alone, as in the reference. Requiring `xend == i32::MAX` too meant a
    /// half-initialised "all" ROI reported itself as *defined*, and then flowed into `union` and
    /// `intersection` as if its garbage bounds were real.
    #[inline]
    pub const fn is_all(&self) -> bool {
        self.xbegin == i32::MIN
    }

    /// Returns true if this ROI is defined (has valid, finite bounds).
    #[inline]
    pub const fn defined(&self) -> bool {
        !self.is_all()
    }

    /// Width of the ROI (xend - xbegin).
    ///
    /// Saturating: an "all" ROI spans `i32::MIN..i32::MAX`, whose true width is not representable
    /// and would panic on overflow in a debug build. Sizes are only meaningful on a
    /// [`Self::defined`] ROI; this returns `i32::MAX` rather than aborting on the undefined one.
    #[inline]
    pub const fn width(&self) -> i32 {
        self.xend.saturating_sub(self.xbegin)
    }

    /// Height of the ROI (yend - ybegin). Saturating — see [`Self::width`].
    #[inline]
    pub const fn height(&self) -> i32 {
        self.yend.saturating_sub(self.ybegin)
    }

    /// Depth of the ROI (zend - zbegin). Saturating — see [`Self::width`].
    #[inline]
    pub const fn depth(&self) -> i32 {
        self.zend.saturating_sub(self.zbegin)
    }

    /// Number of channels in the ROI.
    ///
    /// Beware: this defaults to a large open-ended value ([`Self::CHEND_ALL`]). To be meaningful,
    /// clamp it against the buffer: `min(buf.nchannels(), roi.nchannels())`.
    #[inline]
    pub const fn nchannels(&self) -> i32 {
        self.chend.saturating_sub(self.chbegin)
    }

    /// Total number of pixels in the ROI.
    ///
    /// Returns 0 for an "all" ROI (undefined dimensions).
    #[inline]
    pub fn npixels(&self) -> u64 {
        if self.is_all() {
            0
        } else {
            (self.width() as u64) * (self.height() as u64) * (self.depth() as u64)
        }
    }

    /// Returns true if the point (x, y, z) is inside this ROI.
    #[inline]
    pub const fn contains(&self, x: i32, y: i32, z: i32) -> bool {
        x >= self.xbegin
            && x < self.xend
            && y >= self.ybegin
            && y < self.yend
            && z >= self.zbegin
            && z < self.zend
    }

    /// Returns true if the point (x, y, z, ch) is inside this ROI including channel.
    #[inline]
    pub const fn contains_with_channel(&self, x: i32, y: i32, z: i32, ch: i32) -> bool {
        self.contains(x, y, z) && ch >= self.chbegin && ch < self.chend
    }

    /// Returns true if this ROI fully contains another ROI.
    #[inline]
    pub const fn contains_roi(&self, other: &Roi3D) -> bool {
        other.xbegin >= self.xbegin
            && other.xend <= self.xend
            && other.ybegin >= self.ybegin
            && other.yend <= self.yend
            && other.zbegin >= self.zbegin
            && other.zend <= self.zend
            && other.chbegin >= self.chbegin
            && other.chend <= self.chend
    }

    /// Returns the union of two ROIs (bounding box containing both).
    ///
    /// An undefined ("all") operand is *absorbing in the other direction* from what it looks
    /// like: the union of a real region with "no region specified" is the real region, not "all".
    /// Returning "all" here handed callers a region spanning `i32::MIN..i32::MAX`, which they then
    /// tried to iterate.
    pub fn union(&self, other: &Roi3D) -> Roi3D {
        match (self.is_all(), other.is_all()) {
            (true, true) => return Roi3D::all(),
            (true, false) => return *other,
            (false, true) => return *self,
            (false, false) => {}
        }
        Roi3D {
            xbegin: self.xbegin.min(other.xbegin),
            xend: self.xend.max(other.xend),
            ybegin: self.ybegin.min(other.ybegin),
            yend: self.yend.max(other.yend),
            zbegin: self.zbegin.min(other.zbegin),
            zend: self.zend.max(other.zend),
            chbegin: self.chbegin.min(other.chbegin),
            chend: self.chend.max(other.chend),
        }
    }

    /// Returns the intersection of two ROIs.
    ///
    /// Returns `None` if the ROIs don't overlap.
    pub fn intersection(&self, other: &Roi3D) -> Option<Roi3D> {
        // Same undefined-operand rule as `union`, and for the same reason: intersecting a real
        // region with "no region specified" is the real region. This used to work only by
        // accident -- `i32::MIN`/`i32::MAX` are the identities for `max`/`min`, so a fully
        // sentinel-valued "all" fell out correct. That accident stops holding for an ROI with
        // `xbegin == i32::MIN` but a real `xend`, which `is_all` now (correctly) treats as
        // undefined, so the guard is made explicit.
        match (self.is_all(), other.is_all()) {
            (true, true) => return Some(Roi3D::all()),
            (true, false) => return Some(*other),
            (false, true) => return Some(*self),
            (false, false) => {}
        }

        let result = Roi3D {
            xbegin: self.xbegin.max(other.xbegin),
            xend: self.xend.min(other.xend),
            ybegin: self.ybegin.max(other.ybegin),
            yend: self.yend.min(other.yend),
            zbegin: self.zbegin.max(other.zbegin),
            zend: self.zend.min(other.zend),
            chbegin: self.chbegin.max(other.chbegin),
            chend: self.chend.min(other.chend),
        };

        if result.xbegin < result.xend
            && result.ybegin < result.yend
            && result.zbegin < result.zend
            && result.chbegin < result.chend
        {
            Some(result)
        } else {
            None
        }
    }

    /// Converts to a simple Rect (loses z and channel info).
    #[inline]
    pub fn to_rect(&self) -> Rect {
        Rect::new(
            self.xbegin.max(0) as u32,
            self.ybegin.max(0) as u32,
            self.width().max(0) as u32,
            self.height().max(0) as u32,
        )
    }

    /// Creates from a simple Rect.
    #[inline]
    pub fn from_rect(r: &Rect) -> Self {
        Self::new_2d(
            r.x as i32,
            (r.x + r.width) as i32,
            r.y as i32,
            (r.y + r.height) as i32,
        )
    }

    // =========================================================================
    // Additional Utility Methods (OIIO Parity)
    // =========================================================================

    /// Returns true if this ROI overlaps with another.
    #[inline]
    pub fn overlaps(&self, other: &Roi3D) -> bool {
        self.intersection(other).is_some()
    }

    /// Translates the ROI by the given offset.
    #[inline]
    pub fn translate(&self, dx: i32, dy: i32, dz: i32) -> Self {
        if self.is_all() {
            return *self;
        }
        Self {
            xbegin: self.xbegin.saturating_add(dx),
            xend: self.xend.saturating_add(dx),
            ybegin: self.ybegin.saturating_add(dy),
            yend: self.yend.saturating_add(dy),
            zbegin: self.zbegin.saturating_add(dz),
            zend: self.zend.saturating_add(dz),
            chbegin: self.chbegin,
            chend: self.chend,
        }
    }

    /// Expands the ROI by the given amount on all sides.
    ///
    /// Negative values shrink the ROI.
    #[inline]
    pub fn expand(&self, amount: i32) -> Self {
        self.expand_xy(amount, amount)
    }

    /// Expands the ROI by different amounts in X and Y.
    #[inline]
    pub fn expand_xy(&self, x_amount: i32, y_amount: i32) -> Self {
        if self.is_all() {
            return *self;
        }
        Self {
            xbegin: self.xbegin.saturating_sub(x_amount),
            xend: self.xend.saturating_add(x_amount),
            ybegin: self.ybegin.saturating_sub(y_amount),
            yend: self.yend.saturating_add(y_amount),
            zbegin: self.zbegin,
            zend: self.zend,
            chbegin: self.chbegin,
            chend: self.chend,
        }
    }

    /// Expands the ROI in all dimensions.
    #[inline]
    pub fn expand_3d(&self, x_amount: i32, y_amount: i32, z_amount: i32) -> Self {
        if self.is_all() {
            return *self;
        }
        Self {
            xbegin: self.xbegin.saturating_sub(x_amount),
            xend: self.xend.saturating_add(x_amount),
            ybegin: self.ybegin.saturating_sub(y_amount),
            yend: self.yend.saturating_add(y_amount),
            zbegin: self.zbegin.saturating_sub(z_amount),
            zend: self.zend.saturating_add(z_amount),
            chbegin: self.chbegin,
            chend: self.chend,
        }
    }

    /// Clamps the ROI to the given image bounds.
    pub fn clamp(&self, image_width: i32, image_height: i32, image_depth: i32) -> Self {
        if self.is_all() {
            return Self::new(
                0..image_width,
                0..image_height,
                0..image_depth,
                self.chbegin..self.chend,
            );
        }
        Self {
            xbegin: self.xbegin.max(0).min(image_width),
            xend: self.xend.max(0).min(image_width),
            ybegin: self.ybegin.max(0).min(image_height),
            yend: self.yend.max(0).min(image_height),
            zbegin: self.zbegin.max(0).min(image_depth),
            zend: self.zend.max(0).min(image_depth),
            chbegin: self.chbegin,
            chend: self.chend,
        }
    }

    /// Returns true if the ROI is empty (zero area).
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.xbegin >= self.xend || self.ybegin >= self.yend || self.zbegin >= self.zend
    }

    /// Returns the center point of the ROI as (x, y, z).
    #[inline]
    pub fn center(&self) -> (i32, i32, i32) {
        (
            (self.xbegin + self.xend) / 2,
            (self.ybegin + self.yend) / 2,
            (self.zbegin + self.zend) / 2,
        )
    }

    /// Sets the channel range.
    #[inline]
    pub fn with_channels(mut self, chbegin: i32, chend: i32) -> Self {
        self.chbegin = chbegin;
        self.chend = chend;
        self
    }

    /// Sets the Z range.
    #[inline]
    pub fn with_depth(mut self, zbegin: i32, zend: i32) -> Self {
        self.zbegin = zbegin;
        self.zend = zend;
        self
    }

    /// Returns a 2D slice of this ROI at the given Z.
    #[inline]
    pub fn slice_2d(&self, z: i32) -> Self {
        Self {
            xbegin: self.xbegin,
            xend: self.xend,
            ybegin: self.ybegin,
            yend: self.yend,
            zbegin: z,
            zend: z + 1,
            chbegin: self.chbegin,
            chend: self.chend,
        }
    }

    /// Returns an iterator over all (x, y) coordinates in this 2D ROI.
    pub fn iter_xy(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        (self.ybegin..self.yend).flat_map(move |y| (self.xbegin..self.xend).map(move |x| (x, y)))
    }

    /// Returns an iterator over all (x, y, z) coordinates in this ROI.
    pub fn iter_xyz(&self) -> impl Iterator<Item = (i32, i32, i32)> + '_ {
        (self.zbegin..self.zend).flat_map(move |z| {
            (self.ybegin..self.yend)
                .flat_map(move |y| (self.xbegin..self.xend).map(move |x| (x, y, z)))
        })
    }

    /// Scales the ROI by the given factor.
    ///
    /// Both origin and size are scaled.
    pub fn scale(&self, factor: f32) -> Self {
        if self.is_all() {
            return *self;
        }
        Self {
            xbegin: (self.xbegin as f32 * factor) as i32,
            xend: (self.xend as f32 * factor) as i32,
            ybegin: (self.ybegin as f32 * factor) as i32,
            yend: (self.yend as f32 * factor) as i32,
            zbegin: self.zbegin,
            zend: self.zend,
            chbegin: self.chbegin,
            chend: self.chend,
        }
    }

    /// Creates ROI that covers the pixels needed for a filter of given radius.
    ///
    /// Useful when computing how much source data is needed for a filtered region.
    pub fn for_filter(&self, filter_radius: i32) -> Self {
        self.expand(filter_radius)
    }
}

impl std::fmt::Display for Roi3D {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_all() {
            write!(f, "Roi3D::All")
        } else {
            write!(
                f,
                "Roi3D([{},{}), [{},{}), [{},{}), ch[{},{})]",
                self.xbegin,
                self.xend,
                self.ybegin,
                self.yend,
                self.zbegin,
                self.zend,
                self.chbegin,
                self.chend
            )
        }
    }
}

/// Computes the union of two ROIs.
#[inline]
pub fn roi_union(a: &Roi3D, b: &Roi3D) -> Roi3D {
    a.union(b)
}

/// Computes the intersection of two ROIs.
#[inline]
pub fn roi_intersection(a: &Roi3D, b: &Roi3D) -> Option<Roi3D> {
    a.intersection(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rect_new() {
        let r = Rect::new(10, 20, 100, 50);
        assert_eq!(r.x, 10);
        assert_eq!(r.y, 20);
        assert_eq!(r.width, 100);
        assert_eq!(r.height, 50);
    }

    #[test]
    fn test_rect_edges() {
        let r = Rect::new(10, 20, 100, 50);
        assert_eq!(r.right(), 110);
        assert_eq!(r.bottom(), 70);
    }

    #[test]
    fn test_rect_area() {
        let r = Rect::new(0, 0, 100, 50);
        assert_eq!(r.area(), 5000);
    }

    #[test]
    fn test_rect_contains() {
        let r = Rect::new(10, 10, 100, 100);
        assert!(r.contains(10, 10));
        assert!(r.contains(50, 50));
        assert!(r.contains(109, 109));
        assert!(!r.contains(110, 110));
        assert!(!r.contains(5, 50));
    }

    #[test]
    fn test_rect_intersect() {
        let a = Rect::new(0, 0, 100, 100);
        let b = Rect::new(50, 50, 100, 100);
        let i = a.intersect(&b).unwrap();
        assert_eq!(i, Rect::new(50, 50, 50, 50));

        let c = Rect::new(200, 200, 50, 50);
        assert!(a.intersect(&c).is_none());
    }

    #[test]
    fn test_rect_union() {
        let a = Rect::new(0, 0, 50, 50);
        let b = Rect::new(100, 100, 50, 50);
        let u = a.union(&b);
        assert_eq!(u, Rect::new(0, 0, 150, 150));
    }

    #[test]
    fn test_rect_translate() {
        let r = Rect::new(10, 20, 100, 50);
        let t = r.translate(5, -10);
        assert_eq!(t, Rect::new(15, 10, 100, 50));
    }

    #[test]
    fn test_rect_inset() {
        let r = Rect::new(0, 0, 100, 100);
        let i = r.inset(10).unwrap();
        assert_eq!(i, Rect::new(10, 10, 80, 80));

        let small = Rect::new(0, 0, 10, 10);
        assert!(small.inset(10).is_none());
    }

    #[test]
    fn test_rect_iter_coords() {
        let r = Rect::new(0, 0, 2, 2);
        let coords: Vec<_> = r.iter_coords().collect();
        assert_eq!(coords, vec![(0, 0), (1, 0), (0, 1), (1, 1)]);
    }

    #[test]
    fn test_roi_full() {
        let roi = Roi::Full;
        assert!(roi.is_full());
        assert_eq!(roi.resolve(1920, 1080), Rect::from_size(1920, 1080));
    }

    #[test]
    fn test_roi_region() {
        let roi = Roi::from_xywh(100, 100, 500, 500);
        assert!(!roi.is_full());
        assert_eq!(roi.resolve(1920, 1080), Rect::new(100, 100, 500, 500));
    }

    #[test]
    fn test_roi_clamp() {
        let roi = Roi::from_xywh(900, 500, 2000, 2000);
        let resolved = roi.resolve(1920, 1080);
        assert_eq!(resolved.right(), 1920);
        assert_eq!(resolved.bottom(), 1080);
    }

    // Roi3D tests
    #[test]
    fn test_roi3d_new() {
        let roi = Roi3D::new(100..200, 50..150, 0..1, 0..4);
        assert_eq!(roi.width(), 100);
        assert_eq!(roi.height(), 100);
        assert_eq!(roi.depth(), 1);
        assert_eq!(roi.nchannels(), 4);
    }

    #[test]
    fn test_roi3d_all() {
        let roi = Roi3D::all();
        assert!(roi.is_all());
        assert!(!roi.defined());
        assert_eq!(roi.npixels(), 0);
    }

    #[test]
    fn test_roi3d_contains() {
        let roi = Roi3D::new(100..200, 50..150, 0..1, 0..4);
        assert!(roi.contains(100, 50, 0));
        assert!(roi.contains(150, 100, 0));
        assert!(!roi.contains(200, 150, 0)); // End is exclusive
        assert!(!roi.contains(99, 50, 0)); // Before begin
    }

    #[test]
    fn test_roi3d_contains_with_channel() {
        let roi = Roi3D::new(0..100, 0..100, 0..1, 0..3);
        assert!(roi.contains_with_channel(50, 50, 0, 0));
        assert!(roi.contains_with_channel(50, 50, 0, 2));
        assert!(!roi.contains_with_channel(50, 50, 0, 3)); // Channel end is exclusive
    }

    #[test]
    fn test_roi3d_union() {
        let a = Roi3D::new_2d(0, 100, 0, 100);
        let b = Roi3D::new_2d(50, 150, 50, 150);
        let u = a.union(&b);
        assert_eq!(u.xbegin, 0);
        assert_eq!(u.xend, 150);
        assert_eq!(u.ybegin, 0);
        assert_eq!(u.yend, 150);
    }

    #[test]
    fn test_roi3d_intersection() {
        let a = Roi3D::new_2d(0, 100, 0, 100);
        let b = Roi3D::new_2d(50, 150, 50, 150);
        let i = a.intersection(&b).unwrap();
        assert_eq!(i.xbegin, 50);
        assert_eq!(i.xend, 100);
        assert_eq!(i.ybegin, 50);
        assert_eq!(i.yend, 100);
    }

    #[test]
    fn test_roi3d_no_intersection() {
        let a = Roi3D::new_2d(0, 50, 0, 50);
        let b = Roi3D::new_2d(100, 150, 100, 150);
        assert!(a.intersection(&b).is_none());
    }

    #[test]
    fn test_roi3d_npixels() {
        let roi = Roi3D::new(0..100, 0..200, 0..3, 0..4);
        assert_eq!(roi.npixels(), 100 * 200 * 3);
    }

    #[test]
    fn test_roi3d_to_from_rect() {
        let rect = Rect::new(10, 20, 100, 50);
        let roi = Roi3D::from_rect(&rect);
        assert_eq!(roi.xbegin, 10);
        assert_eq!(roi.xend, 110);
        assert_eq!(roi.ybegin, 20);
        assert_eq!(roi.yend, 70);

        let back = roi.to_rect();
        assert_eq!(back, rect);
    }
}
