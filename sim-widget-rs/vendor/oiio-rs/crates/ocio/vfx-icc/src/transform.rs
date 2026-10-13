//! Color transforms between ICC profiles.

use crate::{IccError, IccResult, Intent, Profile};
use lcms_rs::cmm::optimize::FLAGS_NOCACHE;
use lcms_rs::prelude::{Context, Transform as LcmsTransform, create_transform, formats};

/// A color transform between two ICC profiles.
///
/// Converts pixel data from the source color space to the destination
/// color space using the specified rendering intent.
///
/// # Example
///
/// ```rust
/// use vfx_icc::{Profile, Transform, Intent};
///
/// let srgb = Profile::srgb().unwrap();
/// let aces = Profile::aces_ap1().unwrap();
///
/// let transform = Transform::new(&srgb, &aces, Intent::Perceptual).unwrap();
///
/// let mut pixels = [[0.5f32, 0.3, 0.2]];
/// transform.apply(&mut pixels);
/// ```
pub struct Transform {
    inner: LcmsTransform,
}

impl Transform {
    /// Shared constructor for [`Transform::new`] and [`Transform::new_uncached`].
    ///
    /// Both profiles are cloned before linking. `create_transform` needs `&mut Profile` because
    /// `lcms-rs` decodes tags lazily and caches the decoded value back into the tag dictionary
    /// (the same mutation C's `cmsReadTag` performs on `TagPtrs[]`), while this crate's public
    /// API hands out `&Profile`. An `lcms_rs::Profile` clone duplicates only the tag dictionary
    /// and shares the `Arc<Mutex<dyn IoHandler>>` backing store with the original, so the clone
    /// is cheap — it does not copy the profile's bytes. Do not remove it in the belief that it
    /// is redundant work: it is what keeps `&self`/`&Profile` in the public signatures.
    fn build(source: &Profile, dest: &Profile, intent: Intent, flags: u32) -> IccResult<Self> {
        let mut src = source.inner.clone();
        let mut dst = dest.inner.clone();

        let inner = create_transform(
            &Context::new(),
            &mut src,
            formats::RGB_FLT,
            &mut dst,
            formats::RGB_FLT,
            intent.code(),
            flags,
        )
        .map_err(|e| IccError::TransformFailed(e.to_string()))?;

        Ok(Self { inner })
    }

    /// Runs the transform from `src` into `dst`, both viewed as raw bytes.
    ///
    /// `do_transform` is not an in-place operation (unlike the old FFI wrapper's
    /// `transform_in_place`), so every caller needs a distinct destination buffer. Callers that
    /// present an in-place API to their own users stage the output here and copy it back.
    fn run(&self, src: &[[f32; 3]], dst: &mut [[f32; 3]]) {
        self.inner.do_transform(
            bytemuck::cast_slice(src),
            bytemuck::cast_slice_mut(dst),
            src.len(),
        );
    }
    /// Creates a new transform between two profiles.
    ///
    /// Uses 32-bit float RGB format for both input and output.
    ///
    /// # Arguments
    ///
    /// * `source` - Source color profile
    /// * `dest` - Destination color profile
    /// * `intent` - Rendering intent
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_icc::{Profile, Transform, Intent};
    ///
    /// let camera = Profile::srgb().unwrap();
    /// let display = Profile::display_p3().unwrap();
    ///
    /// let transform = Transform::new(&camera, &display, Intent::Perceptual).unwrap();
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`IccError::TransformFailed`] if the two profiles cannot be linked.
    pub fn new(source: &Profile, dest: &Profile, intent: Intent) -> IccResult<Self> {
        Self::build(source, dest, intent, 0)
    }

    /// Creates a thread-safe transform without caching.
    ///
    /// This transform can be shared between threads safely.
    ///
    /// # Arguments
    ///
    /// * `source` - Source color profile
    /// * `dest` - Destination color profile
    /// * `intent` - Rendering intent
    ///
    /// # Errors
    ///
    /// Returns [`IccError::TransformFailed`] if the two profiles cannot be linked.
    pub fn new_uncached(source: &Profile, dest: &Profile, intent: Intent) -> IccResult<Self> {
        Self::build(source, dest, intent, FLAGS_NOCACHE)
    }

    /// Applies the transform to RGB pixels in-place.
    ///
    /// # Arguments
    ///
    /// * `pixels` - Array of RGB pixels [f32; 3]
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_icc::{Profile, Transform, Intent};
    ///
    /// let transform = Transform::new(
    ///     &Profile::srgb().unwrap(),
    ///     &Profile::aces_ap1().unwrap(),
    ///     Intent::Perceptual,
    /// ).unwrap();
    ///
    /// let mut pixels = [[0.5f32, 0.3, 0.2], [0.8, 0.6, 0.4]];
    /// transform.apply(&mut pixels);
    /// ```
    pub fn apply(&self, pixels: &mut [[f32; 3]]) {
        // Staged through a scratch buffer because the CMM writes to a separate destination;
        // `pixels` is both the source and the caller's output, so it cannot alias the target.
        let src = pixels.to_vec();
        self.run(&src, pixels);
    }

    /// Applies the transform to a single RGB pixel in-place.
    ///
    /// # Arguments
    ///
    /// * `rgb` - RGB pixel as [f32; 3]
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_icc::{Profile, Transform, Intent};
    ///
    /// let transform = Transform::new(
    ///     &Profile::srgb().unwrap(),
    ///     &Profile::aces_ap1().unwrap(),
    ///     Intent::Perceptual,
    /// ).unwrap();
    ///
    /// let mut pixel = [0.5f32, 0.3, 0.2];
    /// transform.apply_pixel(&mut pixel);
    /// ```
    pub fn apply_pixel(&self, rgb: &mut [f32; 3]) {
        let src = [*rgb];
        self.run(&src, std::slice::from_mut(rgb));
    }

    /// Transforms pixels from source to destination buffer.
    ///
    /// # Arguments
    ///
    /// * `source` - Source pixels
    /// * `dest` - Destination buffer (must be same length as source)
    ///
    /// # Errors
    ///
    /// Returns [`IccError::InvalidBuffer`] if `source` and `dest` differ in length.
    pub fn transform(&self, source: &[[f32; 3]], dest: &mut [[f32; 3]]) -> IccResult<()> {
        if source.len() != dest.len() {
            return Err(IccError::InvalidBuffer {
                reason: format!(
                    "source and dest must have the same length (source={}, dest={})",
                    source.len(),
                    dest.len()
                ),
            });
        }
        self.run(source, dest);
        Ok(())
    }

    /// Applies the transform to a flat f32 buffer.
    ///
    /// Buffer must contain RGB triplets (length divisible by 3).
    ///
    /// # Arguments
    ///
    /// * `data` - Flat RGB data as f32 values
    ///
    /// # Errors
    ///
    /// Returns [`IccError::InvalidBuffer`] if `data.len()` is not a multiple of 3.
    ///
    /// The check stays load-bearing after the move to `bytemuck`: `cast_slice_mut` *panics* on a
    /// length that does not divide evenly into the target type, so rejecting the shape up front
    /// is what turns a caller mistake into a typed error instead of an abort.
    pub fn apply_buffer(&self, data: &mut [f32]) -> IccResult<()> {
        if !data.len().is_multiple_of(3) {
            return Err(IccError::InvalidBuffer {
                reason: format!(
                    "length {} is not a multiple of 3 (RGB triplets)",
                    data.len()
                ),
            });
        }

        let pixels: &mut [[f32; 3]] = bytemuck::cast_slice_mut(data);
        let src = pixels.to_vec();
        self.run(&src, pixels);
        Ok(())
    }
}

impl std::fmt::Debug for Transform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transform").finish_non_exhaustive()
    }
}

/// Convenience function to convert RGB from one profile to another.
///
/// Creates a temporary transform and applies it.
/// For repeated conversions, prefer creating a [`Transform`] once.
///
/// # Example
///
/// ```rust
/// use vfx_icc::{Profile, Intent, convert_rgb};
///
/// let mut pixels = [[0.5f32, 0.3, 0.2]];
/// let aces = Profile::aces_ap1().unwrap();
/// convert_rgb(&mut pixels, &Profile::srgb().unwrap(), &aces, Intent::Perceptual).unwrap();
/// ```
pub fn convert_rgb(
    pixels: &mut [[f32; 3]],
    source: &Profile,
    dest: &Profile,
    intent: Intent,
) -> IccResult<()> {
    let transform = Transform::new(source, dest, intent)?;
    transform.apply(pixels);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_srgb_to_aces() {
        let srgb = Profile::srgb().unwrap();
        let aces = Profile::aces_ap1().unwrap();

        let transform = Transform::new(&srgb, &aces, Intent::Perceptual).unwrap();

        // Feed a fully saturated sRGB red so the gamut rotation is observable
        // (a neutral grey stays neutral under ANY primaries matrix, so it can't
        // distinguish a real transform from identity). sRGB EOTF maps code 1.0->
        // linear 1.0 and 0.0->0.0, so linear sRGB red is (1,0,0); converting to
        // linear ACEScg (AP1) yields the FIRST COLUMN of the canonical Bradford
        // sRGB->ACEScg matrix. Pinned to vfx_primaries::SRGB_TO_ACESCG_BRADFORD
        // column 0 = [0.6130974, 0.07019372, 0.02061559] (ACES TB-2014-004 /
        // Bradford D65->D60 CAT). lcms2 reproduces these to <1e-5.
        let mut pixels = [[1.0f32, 0.0, 0.0]];
        transform.apply(&mut pixels);

        assert!(
            (pixels[0][0] - 0.6130974).abs() < 1e-4,
            "R = {}",
            pixels[0][0]
        );
        assert!(
            (pixels[0][1] - 0.07019372).abs() < 1e-4,
            "G = {}",
            pixels[0][1]
        );
        assert!(
            (pixels[0][2] - 0.02061559).abs() < 1e-4,
            "B = {}",
            pixels[0][2]
        );
    }

    #[test]
    fn test_identity() {
        let srgb = Profile::srgb().unwrap();

        let transform = Transform::new(&srgb, &srgb, Intent::Perceptual).unwrap();

        let original = [0.5f32, 0.3, 0.2];
        let mut pixels = [original];
        transform.apply(&mut pixels);

        // Should be unchanged
        assert!((pixels[0][0] - original[0]).abs() < 0.01);
        assert!((pixels[0][1] - original[1]).abs() < 0.01);
        assert!((pixels[0][2] - original[2]).abs() < 0.01);
    }

    #[test]
    fn test_pixel() {
        let transform = Transform::new(
            &Profile::srgb().unwrap(),
            &Profile::linear_srgb().unwrap(),
            Intent::Perceptual,
        )
        .unwrap();

        let mut pixel = [0.5f32, 0.5, 0.5];
        transform.apply_pixel(&mut pixel);

        // sRGB EOTF (IEC 61966-2-1): for code 0.5 > 0.04045,
        // linear = ((0.5 + 0.055) / 1.055)^2.4 = 0.214041. Pin it, not just
        // "< 0.5" (which passes even for an identity/no-op transform).
        assert!((pixel[0] - 0.214041).abs() < 0.005, "linear = {}", pixel[0]);
    }

    #[test]
    fn test_convert_rgb() {
        let mut pixels = [[0.5f32, 0.3, 0.2]];
        convert_rgb(
            &mut pixels,
            &Profile::srgb().unwrap(),
            &Profile::display_p3().unwrap(),
            Intent::RelativeColorimetric,
        )
        .unwrap();

        // Display P3 is wider than sRGB (both D65, so no chromatic adaptation —
        // pure primaries rotation through the shared sRGB-style EOTF), so an
        // sRGB colour maps to slightly SMALLER P3 code values. lcms2 (Bradford
        // CAT) converts sRGB (0.5,0.3,0.2) -> Display P3
        // (0.472011, 0.309269, 0.218316). Pinned to the lcms2 reference output
        // — the old "0 < R < 1" passed for an identity transform too.
        assert!(
            (pixels[0][0] - 0.472011).abs() < 1e-3,
            "R = {}",
            pixels[0][0]
        );
        assert!(
            (pixels[0][1] - 0.309269).abs() < 1e-3,
            "G = {}",
            pixels[0][1]
        );
        assert!(
            (pixels[0][2] - 0.218316).abs() < 1e-3,
            "B = {}",
            pixels[0][2]
        );
    }

    /// The two buffer-shape checks were `assert!`/`assert_eq!` panics. `apply_buffer`'s is also
    /// what keeps `bytemuck::cast_slice_mut` off a length it would panic on, so it must still
    /// reject — via `Err`, not by aborting the process.
    #[test]
    fn test_buffer_shape_errors_instead_of_panicking() {
        let transform = Transform::new(
            &Profile::srgb().unwrap(),
            &Profile::linear_srgb().unwrap(),
            Intent::Perceptual,
        )
        .unwrap();

        for len in [1usize, 2, 4, 5, 7] {
            let mut buf = vec![0.5f32; len];
            assert!(
                matches!(
                    transform.apply_buffer(&mut buf),
                    Err(IccError::InvalidBuffer { .. })
                ),
                "length {len} should be rejected"
            );
        }

        let src = [[0.5f32, 0.5, 0.5]; 2];
        let mut dst = [[0.0f32, 0.0, 0.0]; 3];
        assert!(matches!(
            transform.transform(&src, &mut dst),
            Err(IccError::InvalidBuffer { .. })
        ));
    }

    #[test]
    fn test_buffer() {
        let transform = Transform::new(
            &Profile::srgb().unwrap(),
            &Profile::linear_srgb().unwrap(),
            Intent::Perceptual,
        )
        .unwrap();

        let mut buffer = [0.5f32, 0.5, 0.5, 0.8, 0.8, 0.8];
        transform.apply_buffer(&mut buffer).unwrap();

        // sRGB EOTF of code 0.5 = 0.214041 (IEC 61966-2-1); pin it instead of
        // "< 0.5" so an identity/no-op transform fails. (apply_buffer operates
        // on RGB triplets only — no alpha channel in this f32 path.)
        assert!(
            (buffer[0] - 0.214041).abs() < 0.005,
            "linear[0] = {}",
            buffer[0]
        );
        // Second triplet (code 0.8): ((0.8 + 0.055) / 1.055)^2.4 = 0.603827.
        assert!(
            (buffer[3] - 0.603827).abs() < 0.005,
            "linear[3] = {}",
            buffer[3]
        );
    }
}
