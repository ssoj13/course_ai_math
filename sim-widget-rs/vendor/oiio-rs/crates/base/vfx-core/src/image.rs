//! Image buffer types for VFX processing.
//!
//! This module provides the core image container types:
//! - [`Image`] - Owned image buffer with compile-time color space safety
//! - [`ImageView`] - Immutable borrowed view into an image region
//! - [`ImageViewMut`] - Mutable borrowed view into an image region
//!
//! # Design Philosophy
//!
//! The image types use generics for compile-time guarantees:
//! - `C: ColorSpace` - Color space marker (prevents mixing Srgb with AcesCg)
//! - `T: PixelFormat` - Pixel data type (u8, u16, f16, f32)
//!
//! This means you cannot accidentally pass an sRGB image to a function
//! expecting ACEScg - the compiler will catch it.
//!
//! # Memory Layout
//!
//! Images store pixels in **row-major** order, top-to-bottom:
//!
//! ```text
//! Memory: [R G B R G B R G B ...]  ← Row 0
//!         [R G B R G B R G B ...]  ← Row 1
//!         ...
//! ```
//!
//! For RGBA images, alpha is interleaved: `[R G B A R G B A ...]`
//!
//! # Usage
//!
//! ```rust
//! use vfx_core::{Image, AcesCg};
//!
//! // Create a 1920x1080 RGBA image in ACEScg color space
//! let mut img: Image<AcesCg, f32, 4> = Image::new(1920, 1080);
//!
//! // Set a pixel [R, G, B, A]
//! img.set_pixel(100, 100, [1.0, 0.5, 0.25, 1.0]);
//!
//! // Get a pixel
//! let px = img.pixel(100, 100);
//! assert_eq!(px[0], 1.0); // R channel
//! ```
//!
//! # Views
//!
//! Views provide zero-copy access to image regions:
//!
//! ```rust
//! use vfx_core::{Image, Rect, AcesCg};
//!
//! let img: Image<AcesCg, f32, 4> = Image::new(1920, 1080);
//!
//! // Create a view into a sub-region
//! let roi = Rect::new(100, 100, 500, 500);
//! let view = img.view(roi);
//!
//! // Iterate over pixels in the view
//! for (x, y, pixel) in view.pixels() {
//!     // process pixel
//! }
//! ```
//!
//! # Dependencies
//!
//! - [`crate::colorspace::ColorSpace`] - Color space marker traits
//! - [`crate::pixel::PixelFormat`] - Pixel data type trait
//! - [`crate::rect::Rect`] - Region definitions
//! - [`crate::error::Error`] - Error types
//! - [`rayon`] - Parallel iteration (optional)
//!
//! # Used By
//!
//! - `vfx-io` - Image loading/saving
//! - `vfx-ops` - Image processing operations
//! - `vfx-composite` - Compositing operations

use crate::{ColorSpace, Error, PixelFormat, Rect, Result, Roi};
use std::marker::PhantomData;
use std::sync::Arc;

/// Owned image buffer with compile-time color space and pixel format.
///
/// `Image<C, T, N>` stores pixel data in a contiguous buffer where:
/// - `C` - Color space marker type ([`AcesCg`](crate::AcesCg), [`Srgb`](crate::Srgb), etc.)
/// - `T` - Pixel component type ([`u8`], [`u16`], [`half::f16`], [`f32`])
/// - `N` - Number of channels (3 for RGB, 4 for RGBA)
///
/// # Memory Management
///
/// The pixel buffer is stored in an [`Arc<Vec<T>>`], enabling:
/// - Zero-copy cloning (shares underlying data)
/// - Thread-safe sharing for parallel processing
/// - Cheap view creation
///
/// To get a mutable exclusive copy, use [`make_mut`](Self::make_mut).
///
/// # Example
///
/// ```rust
/// use vfx_core::{Image, AcesCg};
///
/// // Create empty image
/// let mut img: Image<AcesCg, f32, 4> = Image::new(1920, 1080);
///
/// // Fill with color [R, G, B, A]
/// let red = [1.0, 0.0, 0.0, 1.0];
/// img.fill(red);
///
/// // Access pixels
/// let px = img.pixel(0, 0);
/// assert_eq!(px[0], 1.0); // Red channel
/// ```
#[derive(Clone)]
pub struct Image<C: ColorSpace, T: PixelFormat, const N: usize> {
    /// Pixel data buffer (Arc for cheap cloning)
    data: Arc<Vec<T>>,
    /// Image width in pixels
    width: u32,
    /// Image height in pixels
    height: u32,
    /// Bytes per row (may include padding)
    stride: usize,
    /// Color space marker
    _colorspace: PhantomData<C>,
}

impl<C: ColorSpace, T: PixelFormat, const N: usize> Image<C, T, N> {
    /// Creates a new image filled with zeros.
    ///
    /// # Arguments
    ///
    /// * `width` - Image width in pixels
    /// * `height` - Image height in pixels
    ///
    /// # Panics
    ///
    /// Panics if allocation fails (extremely large images).
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::{Image, Srgb};
    ///
    /// let img: Image<Srgb, u8, 3> = Image::new(1920, 1080);
    /// assert_eq!(img.width(), 1920);
    /// assert_eq!(img.height(), 1080);
    /// ```
    pub fn new(width: u32, height: u32) -> Self {
        let pixel_count = width as usize * height as usize * N;
        let data = vec![T::zero(); pixel_count];
        Self {
            data: Arc::new(data),
            width,
            height,
            stride: width as usize * N * std::mem::size_of::<T>(),
            _colorspace: PhantomData,
        }
    }

    /// Creates an image from existing pixel data.
    ///
    /// # Arguments
    ///
    /// * `width` - Image width
    /// * `height` - Image height
    /// * `data` - Pixel data (must have exactly width * height * N elements)
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidDimensions`] if data length doesn't match.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::{Image, Srgb};
    ///
    /// let pixels: Vec<f32> = vec![0.0; 100 * 100 * 4];
    /// let img: Image<Srgb, f32, 4> = Image::from_data(100, 100, pixels).unwrap();
    /// ```
    pub fn from_data(width: u32, height: u32, data: Vec<T>) -> Result<Self> {
        let expected = width as usize * height as usize * N;
        if data.len() != expected {
            return Err(Error::invalid_dimensions(
                width,
                height,
                format!("expected {} elements, got {}", expected, data.len()),
            ));
        }
        Ok(Self {
            data: Arc::new(data),
            width,
            height,
            stride: width as usize * N * std::mem::size_of::<T>(),
            _colorspace: PhantomData,
        })
    }

    /// Creates an image filled with a specific pixel value.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::{Image, Srgb};
    ///
    /// let white: Image<Srgb, f32, 3> = Image::filled(100, 100, [1.0, 1.0, 1.0]);
    /// ```
    pub fn filled(width: u32, height: u32, pixel: [T; N]) -> Self {
        let pixel_count = width as usize * height as usize;
        let mut data = Vec::with_capacity(pixel_count * N);
        for _ in 0..pixel_count {
            data.extend_from_slice(&pixel);
        }
        Self {
            data: Arc::new(data),
            width,
            height,
            stride: width as usize * N * std::mem::size_of::<T>(),
            _colorspace: PhantomData,
        }
    }

    /// Returns the image width in pixels.
    #[inline]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Returns the image height in pixels.
    #[inline]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Returns the image dimensions as (width, height).
    #[inline]
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Returns the number of channels per pixel.
    #[inline]
    pub const fn channels(&self) -> usize {
        N
    }

    /// Returns the stride (bytes per row).
    #[inline]
    pub fn stride(&self) -> usize {
        self.stride
    }

    /// Returns the total number of pixels.
    #[inline]
    pub fn pixel_count(&self) -> usize {
        self.width as usize * self.height as usize
    }

    /// Returns a rectangle covering the entire image.
    #[inline]
    pub fn bounds(&self) -> Rect {
        Rect::from_size(self.width, self.height)
    }

    /// Returns `true` if the image has zero area.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Returns a reference to the raw pixel data.
    #[inline]
    pub fn data(&self) -> &[T] {
        &self.data
    }

    /// Returns a mutable reference to the pixel data.
    ///
    /// If the data is shared (Arc refcount > 1), this will clone the data
    /// to ensure exclusive access (copy-on-write).
    #[inline]
    pub fn data_mut(&mut self) -> &mut [T] {
        Arc::make_mut(&mut self.data).as_mut_slice()
    }

    /// Ensures this image has exclusive ownership of its data.
    ///
    /// Call this before extensive mutations to avoid repeated CoW clones.
    #[inline]
    pub fn make_mut(&mut self) {
        let _ = Arc::make_mut(&mut self.data);
    }

    /// Returns the byte offset for pixel at (x, y).
    #[inline]
    fn pixel_offset(&self, x: u32, y: u32) -> usize {
        (y as usize * self.width as usize + x as usize) * N
    }

    /// Returns the pixel at (x, y).
    ///
    /// # Panics
    ///
    /// Panics if (x, y) is out of bounds.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::{Image, Srgb};
    ///
    /// let img: Image<Srgb, f32, 3> = Image::filled(10, 10, [1.0, 0.5, 0.25]);
    /// let px = img.pixel(5, 5);
    /// assert_eq!(px, [1.0, 0.5, 0.25]);
    /// ```
    #[inline]
    pub fn pixel(&self, x: u32, y: u32) -> [T; N] {
        debug_assert!(x < self.width && y < self.height, "pixel out of bounds");
        let offset = self.pixel_offset(x, y);
        let mut result = [T::zero(); N];
        result.copy_from_slice(&self.data[offset..offset + N]);
        result
    }

    /// Returns the pixel at (x, y), or `None` if out of bounds.
    #[inline]
    pub fn get_pixel(&self, x: u32, y: u32) -> Option<[T; N]> {
        if x < self.width && y < self.height {
            Some(self.pixel(x, y))
        } else {
            None
        }
    }

    /// Sets the pixel at (x, y).
    ///
    /// # Panics
    ///
    /// Panics if (x, y) is out of bounds.
    #[inline]
    pub fn set_pixel(&mut self, x: u32, y: u32, pixel: [T; N]) {
        debug_assert!(x < self.width && y < self.height, "pixel out of bounds");
        let offset = self.pixel_offset(x, y);
        let data = Arc::make_mut(&mut self.data);
        data[offset..offset + N].copy_from_slice(&pixel);
    }

    /// Fills the entire image with a pixel value.
    pub fn fill(&mut self, pixel: [T; N]) {
        let data = Arc::make_mut(&mut self.data);
        for chunk in data.as_chunks_mut::<N>().0 {
            chunk.copy_from_slice(&pixel);
        }
    }

    /// Returns a row of pixels as a slice.
    ///
    /// # Panics
    ///
    /// Panics if y >= height.
    #[inline]
    pub fn row(&self, y: u32) -> &[T] {
        debug_assert!(y < self.height, "row out of bounds");
        let start = y as usize * self.width as usize * N;
        let end = start + self.width as usize * N;
        &self.data[start..end]
    }

    /// Returns a mutable row of pixels.
    ///
    /// # Panics
    ///
    /// Panics if y >= height.
    #[inline]
    pub fn row_mut(&mut self, y: u32) -> &mut [T] {
        debug_assert!(y < self.height, "row out of bounds");
        let start = y as usize * self.width as usize * N;
        let end = start + self.width as usize * N;
        &mut self.data_mut()[start..end]
    }

    /// Creates an immutable view into a region of this image.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::{Image, Rect, Srgb};
    ///
    /// let img: Image<Srgb, f32, 4> = Image::new(1920, 1080);
    /// let roi = Rect::new(100, 100, 500, 500);
    /// let view = img.view(roi);
    /// ```
    pub fn view(&self, region: impl Into<Roi>) -> ImageView<'_, C, T, N> {
        let rect = region.into().resolve(self.width, self.height);
        ImageView {
            image: self,
            region: rect,
        }
    }

    /// Creates a mutable view into a region of this image.
    pub fn view_mut(&mut self, region: impl Into<Roi>) -> ImageViewMut<'_, C, T, N> {
        let rect = region.into().resolve(self.width, self.height);
        ImageViewMut {
            image: self,
            region: rect,
        }
    }

    /// Iterates over all pixels with their coordinates.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::{Image, Srgb};
    ///
    /// let img: Image<Srgb, f32, 3> = Image::filled(10, 10, [1.0, 0.0, 0.0]);
    /// for (x, y, pixel) in img.pixels() {
    ///     assert_eq!(pixel, [1.0, 0.0, 0.0]);
    /// }
    /// ```
    pub fn pixels(&self) -> impl Iterator<Item = (u32, u32, [T; N])> + '_ {
        (0..self.height).flat_map(move |y| (0..self.width).map(move |x| (x, y, self.pixel(x, y))))
    }

    /// Applies a function to each pixel in place.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::{Image, Srgb};
    ///
    /// let mut img: Image<Srgb, f32, 3> = Image::filled(10, 10, [0.5, 0.5, 0.5]);
    /// img.map_pixels(|px| [px[0] * 2.0, px[1] * 2.0, px[2] * 2.0]);
    /// ```
    pub fn map_pixels<F>(&mut self, f: F)
    where
        F: Fn([T; N]) -> [T; N],
    {
        let data = Arc::make_mut(&mut self.data);
        for chunk in data.as_chunks_mut::<N>().0 {
            let mut pixel = [T::zero(); N];
            pixel.copy_from_slice(chunk);
            let result = f(pixel);
            chunk.copy_from_slice(&result);
        }
    }

    /// Converts to a different color space (reinterpret, no transform).
    ///
    /// This is a zero-cost type-level conversion. The pixel data is unchanged;
    /// only the type marker is updated. Use this when you know the data is
    /// already in the target color space.
    ///
    /// For actual color space conversion, use `vfx-color::convert()`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::{Image, Srgb, LinearSrgb};
    ///
    /// let srgb: Image<Srgb, f32, 3> = Image::new(100, 100);
    /// // Reinterpret as linear (no actual conversion!)
    /// let linear: Image<LinearSrgb, f32, 3> = srgb.reinterpret();
    /// ```
    pub fn reinterpret<C2: ColorSpace>(self) -> Image<C2, T, N> {
        Image {
            data: self.data,
            width: self.width,
            height: self.height,
            stride: self.stride,
            _colorspace: PhantomData,
        }
    }

    /// Converts to a different pixel format.
    ///
    /// Each channel value is converted using [`PixelFormat::to_f32`] and
    /// [`PixelFormat::from_f32`].
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_core::{Image, Srgb};
    ///
    /// let float_img: Image<Srgb, f32, 3> = Image::filled(10, 10, [1.0, 0.5, 0.0]);
    /// let byte_img: Image<Srgb, u8, 3> = float_img.convert_format();
    /// let px = byte_img.pixel(0, 0);
    /// assert_eq!(px, [255, 128, 0]);
    /// ```
    pub fn convert_format<T2: PixelFormat>(&self) -> Image<C, T2, N> {
        let mut result = Image::<C, T2, N>::new(self.width, self.height);
        {
            let out_data = Arc::make_mut(&mut result.data);
            for (src, dst) in self.data.as_chunks::<N>().0.iter().zip(out_data.as_chunks_mut::<N>().0.iter_mut()) {
                for i in 0..N {
                    dst[i] = T2::from_f32(src[i].to_f32());
                }
            }
        }
        result
    }
}

impl<C: ColorSpace, T: PixelFormat, const N: usize> std::fmt::Debug for Image<C, T, N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Image")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("channels", &N)
            .field("colorspace", &C::NAME)
            .field("format", &std::any::type_name::<T>())
            .finish()
    }
}

/// Immutable view into a region of an image.
///
/// A view provides zero-copy access to a rectangular portion of an [`Image`].
/// It borrows the image data and can be used for read-only operations.
///
/// # Example
///
/// ```rust
/// use vfx_core::{Image, Rect, Srgb};
///
/// let img: Image<Srgb, f32, 4> = Image::new(1920, 1080);
/// let roi = Rect::new(100, 100, 500, 500);
/// let view = img.view(roi);
///
/// // Read pixels from the view
/// for (x, y, pixel) in view.pixels() {
///     // coordinates are relative to the view origin
/// }
/// ```
pub struct ImageView<'a, C: ColorSpace, T: PixelFormat, const N: usize> {
    image: &'a Image<C, T, N>,
    region: Rect,
}

impl<'a, C: ColorSpace, T: PixelFormat, const N: usize> ImageView<'a, C, T, N> {
    /// Returns the view width.
    #[inline]
    pub fn width(&self) -> u32 {
        self.region.width
    }

    /// Returns the view height.
    #[inline]
    pub fn height(&self) -> u32 {
        self.region.height
    }

    /// Returns the view dimensions.
    #[inline]
    pub fn dimensions(&self) -> (u32, u32) {
        (self.region.width, self.region.height)
    }

    /// Returns the region this view covers.
    #[inline]
    pub fn region(&self) -> Rect {
        self.region
    }

    /// Returns the pixel at (x, y) relative to the view origin.
    ///
    /// # Panics
    ///
    /// Panics if coordinates are outside the view bounds.
    #[inline]
    pub fn pixel(&self, x: u32, y: u32) -> [T; N] {
        debug_assert!(x < self.region.width && y < self.region.height);
        self.image.pixel(self.region.x + x, self.region.y + y)
    }

    /// Gets a pixel, returning `None` if out of bounds.
    #[inline]
    pub fn get_pixel(&self, x: u32, y: u32) -> Option<[T; N]> {
        if x < self.region.width && y < self.region.height {
            Some(self.pixel(x, y))
        } else {
            None
        }
    }

    /// Iterates over pixels in this view.
    ///
    /// Coordinates are relative to the view origin.
    pub fn pixels(&self) -> impl Iterator<Item = (u32, u32, [T; N])> + '_ {
        (0..self.region.height)
            .flat_map(move |y| (0..self.region.width).map(move |x| (x, y, self.pixel(x, y))))
    }

    /// Creates a sub-view within this view.
    pub fn subview(&self, region: Rect) -> Option<ImageView<'a, C, T, N>> {
        // Clamp region to view bounds
        let clamped = region.clamp_to(self.region.width, self.region.height)?;
        let absolute = Rect::new(
            self.region.x + clamped.x,
            self.region.y + clamped.y,
            clamped.width,
            clamped.height,
        );
        Some(ImageView {
            image: self.image,
            region: absolute,
        })
    }
}

impl<C: ColorSpace, T: PixelFormat, const N: usize> std::fmt::Debug for ImageView<'_, C, T, N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageView")
            .field("region", &self.region)
            .field("colorspace", &C::NAME)
            .finish()
    }
}

/// Mutable view into a region of an image.
///
/// Like [`ImageView`], but allows modifying pixels. Mutably borrows the image.
///
/// # Example
///
/// ```rust
/// use vfx_core::{Image, Rect, Srgb};
///
/// let mut img: Image<Srgb, f32, 4> = Image::new(1920, 1080);
/// let roi = Rect::new(100, 100, 500, 500);
/// let mut view = img.view_mut(roi);
///
/// // Modify pixels in the view
/// view.set_pixel(0, 0, [1.0, 0.0, 0.0, 1.0]);
/// ```
pub struct ImageViewMut<'a, C: ColorSpace, T: PixelFormat, const N: usize> {
    image: &'a mut Image<C, T, N>,
    region: Rect,
}

impl<'a, C: ColorSpace, T: PixelFormat, const N: usize> ImageViewMut<'a, C, T, N> {
    /// Returns the view width.
    #[inline]
    pub fn width(&self) -> u32 {
        self.region.width
    }

    /// Returns the view height.
    #[inline]
    pub fn height(&self) -> u32 {
        self.region.height
    }

    /// Returns the view dimensions.
    #[inline]
    pub fn dimensions(&self) -> (u32, u32) {
        (self.region.width, self.region.height)
    }

    /// Returns the region this view covers.
    #[inline]
    pub fn region(&self) -> Rect {
        self.region
    }

    /// Returns the pixel at (x, y) relative to the view origin.
    #[inline]
    pub fn pixel(&self, x: u32, y: u32) -> [T; N] {
        debug_assert!(x < self.region.width && y < self.region.height);
        self.image.pixel(self.region.x + x, self.region.y + y)
    }

    /// Sets the pixel at (x, y) relative to the view origin.
    #[inline]
    pub fn set_pixel(&mut self, x: u32, y: u32, pixel: [T; N]) {
        debug_assert!(x < self.region.width && y < self.region.height);
        self.image
            .set_pixel(self.region.x + x, self.region.y + y, pixel);
    }

    /// Fills the entire view with a pixel value.
    pub fn fill(&mut self, pixel: [T; N]) {
        for y in 0..self.region.height {
            for x in 0..self.region.width {
                self.set_pixel(x, y, pixel);
            }
        }
    }

    /// Applies a function to each pixel in the view.
    pub fn map_pixels<F>(&mut self, f: F)
    where
        F: Fn([T; N]) -> [T; N],
    {
        for y in 0..self.region.height {
            for x in 0..self.region.width {
                let px = self.pixel(x, y);
                self.set_pixel(x, y, f(px));
            }
        }
    }

    /// Copies pixels from a source view.
    ///
    /// Both views must have the same dimensions.
    pub fn copy_from(&mut self, src: &ImageView<'_, C, T, N>) -> Result<()> {
        if self.dimensions() != src.dimensions() {
            return Err(Error::dimension_mismatch(
                self.dimensions(),
                src.dimensions(),
            ));
        }
        for y in 0..self.region.height {
            for x in 0..self.region.width {
                self.set_pixel(x, y, src.pixel(x, y));
            }
        }
        Ok(())
    }
}

impl<C: ColorSpace, T: PixelFormat, const N: usize> std::fmt::Debug for ImageViewMut<'_, C, T, N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageViewMut")
            .field("region", &self.region)
            .field("colorspace", &C::NAME)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Srgb;

    #[test]
    fn test_image_new() {
        let img: Image<Srgb, f32, 3> = Image::new(100, 100);
        assert_eq!(img.width(), 100);
        assert_eq!(img.height(), 100);
        assert_eq!(img.channels(), 3);
        assert_eq!(img.pixel_count(), 10000);
    }

    #[test]
    fn test_image_filled() {
        let img: Image<Srgb, f32, 3> = Image::filled(10, 10, [1.0, 0.5, 0.25]);
        assert_eq!(img.pixel(0, 0), [1.0, 0.5, 0.25]);
        assert_eq!(img.pixel(9, 9), [1.0, 0.5, 0.25]);
    }

    #[test]
    fn test_image_set_get_pixel() {
        let mut img: Image<Srgb, f32, 4> = Image::new(10, 10);
        img.set_pixel(5, 5, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(img.pixel(5, 5), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(img.pixel(0, 0), [0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn test_image_fill() {
        let mut img: Image<Srgb, f32, 3> = Image::new(10, 10);
        img.fill([0.5, 0.5, 0.5]);
        for (_, _, px) in img.pixels() {
            assert_eq!(px, [0.5, 0.5, 0.5]);
        }
    }

    #[test]
    fn test_image_map_pixels() {
        let mut img: Image<Srgb, f32, 3> = Image::filled(10, 10, [0.5, 0.5, 0.5]);
        img.map_pixels(|px| [px[0] * 2.0, px[1] * 2.0, px[2] * 2.0]);
        assert_eq!(img.pixel(0, 0), [1.0, 1.0, 1.0]);
    }

    #[test]
    fn test_image_from_data() {
        let data = vec![1.0f32; 100 * 100 * 4];
        let img: Image<Srgb, f32, 4> = Image::from_data(100, 100, data).unwrap();
        assert_eq!(img.pixel(50, 50), [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn test_image_from_data_wrong_size() {
        let data = vec![1.0f32; 100]; // Wrong size
        let result: Result<Image<Srgb, f32, 4>> = Image::from_data(100, 100, data);
        assert!(result.is_err());
    }

    #[test]
    fn test_image_convert_format() {
        let float_img: Image<Srgb, f32, 3> = Image::filled(10, 10, [1.0, 0.5, 0.0]);
        let byte_img: Image<Srgb, u8, 3> = float_img.convert_format();
        let px = byte_img.pixel(0, 0);
        assert_eq!(px[0], 255);
        assert!((px[1] as i32 - 128).abs() <= 1); // ~0.5
        assert_eq!(px[2], 0);
    }

    #[test]
    fn test_image_view() {
        let img: Image<Srgb, f32, 3> = Image::filled(100, 100, [1.0, 0.0, 0.0]);
        let view = img.view(Rect::new(10, 10, 50, 50));
        assert_eq!(view.width(), 50);
        assert_eq!(view.height(), 50);
        assert_eq!(view.pixel(0, 0), [1.0, 0.0, 0.0]);
    }

    #[test]
    fn test_image_view_mut() {
        let mut img: Image<Srgb, f32, 3> = Image::new(100, 100);
        {
            let mut view = img.view_mut(Rect::new(10, 10, 50, 50));
            view.fill([0.0, 1.0, 0.0]);
        }
        // Inside the view region
        assert_eq!(img.pixel(10, 10), [0.0, 1.0, 0.0]);
        assert_eq!(img.pixel(59, 59), [0.0, 1.0, 0.0]);
        // Outside the view region
        assert_eq!(img.pixel(0, 0), [0.0, 0.0, 0.0]);
        assert_eq!(img.pixel(60, 60), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn test_image_row() {
        let img: Image<Srgb, f32, 3> = Image::filled(10, 10, [1.0, 0.5, 0.25]);
        let row = img.row(5);
        assert_eq!(row.len(), 30); // 10 pixels * 3 channels
        assert_eq!(&row[0..3], &[1.0, 0.5, 0.25]);
    }

    #[test]
    fn test_image_clone_cow() {
        let img1: Image<Srgb, f32, 3> = Image::filled(10, 10, [1.0, 0.0, 0.0]);
        let mut img2 = img1.clone(); // Shares data

        // Modify img2 - triggers copy-on-write
        img2.set_pixel(0, 0, [0.0, 1.0, 0.0]);

        // img1 unchanged, img2 modified
        assert_eq!(img1.pixel(0, 0), [1.0, 0.0, 0.0]);
        assert_eq!(img2.pixel(0, 0), [0.0, 1.0, 0.0]);
    }
}
