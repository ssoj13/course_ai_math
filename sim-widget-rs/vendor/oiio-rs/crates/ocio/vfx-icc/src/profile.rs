//! ICC profile wrapper.

use crate::{IccError, IccResult, StandardProfile};
use lcms_rs::color::wtpnt::d50_xyy;
use lcms_rs::prelude::{
    Context, Profile as LcmsProfile, Signature, ToneCurve, colorspace, create_gray_profile,
    create_lab4_profile, create_xyz_profile,
};
use lcms_rs::profile::read_hl::{InfoType, get_profile_info_utf8};
use std::path::Path;

/// "No language" / "no country" selector for the multi-localized-unicode profile-info tags.
///
/// Mirrors C's `cmsNoLanguage`/`cmsNoCountry` (two NUL bytes), i.e. "give me the best-matching
/// translation, I don't care which locale it is".
const ANY_LOCALE: [u8; 2] = [0, 0];

/// An ICC color profile.
///
/// Represents a color space and its associated color management data.
/// Profiles can be loaded from files, created from raw ICC data, or
/// generated from standard specifications.
///
/// # Example
///
/// ```rust,no_run
/// use vfx_icc::Profile;
/// use std::path::Path;
///
/// // Load from file
/// let profile = Profile::from_file(Path::new("camera.icc")).unwrap();
///
/// // Create standard profile
/// let srgb = Profile::srgb().unwrap();
///
/// // Get profile info
/// println!("Description: {}", profile.description());
/// ```
pub struct Profile {
    /// Internal `lcms-rs` profile.
    pub(crate) inner: LcmsProfile,
}

impl Profile {
    /// Loads a profile from an ICC file.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the .icc or .icm file
    ///
    /// # Errors
    ///
    /// Returns error if file cannot be read or contains invalid data.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use vfx_icc::Profile;
    /// use std::path::Path;
    ///
    /// let profile = Profile::from_file(Path::new("monitor.icc")).unwrap();
    /// ```
    pub fn from_file(path: &Path) -> IccResult<Self> {
        let inner = LcmsProfile::open_from_file(Context::new(), path)
            .map_err(|e| IccError::LoadFailed(format!("{}: {}", path.display(), e)))?;
        Ok(Self { inner })
    }

    /// Creates a profile from raw ICC data.
    ///
    /// # Arguments
    ///
    /// * `data` - Raw ICC profile bytes
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use vfx_icc::Profile;
    ///
    /// let icc_data = std::fs::read("profile.icc").unwrap();
    /// let profile = Profile::from_icc(&icc_data).unwrap();
    /// ```
    pub fn from_icc(data: &[u8]) -> IccResult<Self> {
        let inner = LcmsProfile::open_from_mem(Context::new(), data)
            .map_err(|e| IccError::InvalidProfile(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Creates an sRGB profile.
    ///
    /// The standard IEC 61966-2-1 sRGB color space.
    ///
    /// # Errors
    ///
    /// See [`Profile::from_standard`]. (Fallible by signature because `lcms-rs` builds this
    /// profile rather than handing back a static one — never `.unwrap()`ed here, per the
    /// crate's error rule.)
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_icc::Profile;
    ///
    /// let srgb = Profile::srgb().unwrap();
    /// ```
    pub fn srgb() -> IccResult<Self> {
        Self::from_standard(StandardProfile::Srgb)
    }

    /// Creates a profile from a standard specification.
    ///
    /// # Errors
    ///
    /// Returns an error if the engine cannot build the standard's transfer curve or combine it
    /// with its white point and primaries into a profile (see [`StandardProfile::to_profile`]).
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_icc::{Profile, StandardProfile};
    ///
    /// let p3 = Profile::from_standard(StandardProfile::DisplayP3).unwrap();
    /// ```
    pub fn from_standard(standard: StandardProfile) -> IccResult<Self> {
        standard.to_profile()
    }

    /// Creates a linear sRGB profile (gamma 1.0).
    ///
    /// Uses sRGB primaries but with a linear transfer function.
    /// Suitable for compositing and rendering.
    ///
    /// # Errors
    ///
    /// See [`Profile::from_standard`].
    pub fn linear_srgb() -> IccResult<Self> {
        Self::from_standard(StandardProfile::LinearSrgb)
    }

    /// Creates an Adobe RGB (1998) profile.
    ///
    /// # Errors
    ///
    /// See [`Profile::from_standard`].
    pub fn adobe_rgb() -> IccResult<Self> {
        Self::from_standard(StandardProfile::AdobeRgb)
    }

    /// Creates a Display P3 profile.
    ///
    /// Wide-gamut profile used by Apple displays.
    ///
    /// # Errors
    ///
    /// See [`Profile::from_standard`].
    pub fn display_p3() -> IccResult<Self> {
        Self::from_standard(StandardProfile::DisplayP3)
    }

    /// Creates a DCI-P3 profile.
    ///
    /// Digital Cinema Initiative P3 with DCI white point.
    ///
    /// # Errors
    ///
    /// See [`Profile::from_standard`].
    pub fn dci_p3() -> IccResult<Self> {
        Self::from_standard(StandardProfile::DciP3)
    }

    /// Creates an ACES AP0 profile (linear).
    ///
    /// Academy Color Encoding System primary color space.
    /// Very wide gamut, suitable as a scene-referred working space.
    ///
    /// # Errors
    ///
    /// See [`Profile::from_standard`].
    pub fn aces_ap0() -> IccResult<Self> {
        Self::from_standard(StandardProfile::AcesAp0)
    }

    /// Creates an ACES AP1 / ACEScg profile (linear).
    ///
    /// ACES computer graphics working space.
    /// Practical wide gamut for CGI and compositing.
    ///
    /// # Errors
    ///
    /// See [`Profile::from_standard`].
    pub fn aces_ap1() -> IccResult<Self> {
        Self::from_standard(StandardProfile::AcesAp1)
    }

    /// Creates a Rec. 709 profile.
    ///
    /// ITU-R BT.709 HD television standard.
    ///
    /// # Errors
    ///
    /// See [`Profile::from_standard`].
    pub fn rec709() -> IccResult<Self> {
        Self::from_standard(StandardProfile::Rec709)
    }

    /// Creates a Rec. 2020 profile.
    ///
    /// ITU-R BT.2020 UHD/HDR television standard.
    ///
    /// # Errors
    ///
    /// See [`Profile::from_standard`].
    pub fn rec2020() -> IccResult<Self> {
        Self::from_standard(StandardProfile::Rec2020)
    }

    /// Creates a grayscale profile with the specified gamma.
    ///
    /// # Arguments
    ///
    /// * `gamma` - Transfer function gamma (e.g., 2.2)
    ///
    /// # Errors
    ///
    /// Returns [`IccError::InvalidGamma`] if `gamma` is not finite and strictly positive, or
    /// [`IccError::CreateFailed`] if the gray profile cannot be built from the resulting curve.
    ///
    /// The explicit `gamma` validation is kept deliberately. `ToneCurve::build_gamma` is
    /// fallible in `lcms-rs` (unlike the old C-FFI `ToneCurve::new`, which silently produced a
    /// null-pointer curve), but it rejects only unknown *curve types* — a degenerate gamma value
    /// still yields a curve. Validating here is what turns a nonsensical gamma into a precise,
    /// caller-facing [`IccError::InvalidGamma`] instead of a downstream colorimetry surprise.
    pub fn gray(gamma: f64) -> IccResult<Self> {
        if !gamma.is_finite() || gamma <= 0.0 {
            return Err(IccError::InvalidGamma(gamma));
        }
        let curve =
            ToneCurve::build_gamma(gamma).map_err(|e| IccError::CreateFailed(e.to_string()))?;
        let inner = create_gray_profile(Some(&d50_xyy()), Some(&curve))
            .map_err(|e| IccError::CreateFailed(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Creates a CIE XYZ profile.
    ///
    /// The standard CIE 1931 XYZ color space (D50 adapted).
    ///
    /// # Errors
    ///
    /// Returns [`IccError::CreateFailed`] if the identity XYZ pipeline cannot be built.
    pub fn xyz() -> IccResult<Self> {
        let inner = create_xyz_profile().map_err(|e| IccError::CreateFailed(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Creates a CIE Lab profile.
    ///
    /// CIE L*a*b* perceptually uniform color space (D50).
    ///
    /// # Errors
    ///
    /// Returns [`IccError::CreateFailed`] if the identity Lab v4 pipeline cannot be built.
    pub fn lab() -> IccResult<Self> {
        let inner = create_lab4_profile(Some(&d50_xyy()))
            .map_err(|e| IccError::CreateFailed(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Returns the profile description.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_icc::Profile;
    ///
    /// let srgb = Profile::srgb().unwrap();
    /// println!("{}", srgb.description());
    /// ```
    pub fn description(&self) -> String {
        self.info(InfoType::Description)
    }

    /// Returns the profile manufacturer.
    pub fn manufacturer(&self) -> String {
        self.info(InfoType::Manufacturer)
    }

    /// Returns the profile model.
    pub fn model(&self) -> String {
        self.info(InfoType::Model)
    }

    /// Returns the profile copyright.
    pub fn copyright(&self) -> String {
        self.info(InfoType::Copyright)
    }

    /// Reads one multi-localized-unicode info tag, or `""` when the tag is absent.
    ///
    /// An absent `desc`/`dmnd`/`dmdd`/`cprt` tag is normal for real-world profiles, so the empty
    /// string is documented, expected behaviour here — not a swallowed error (contrast with the
    /// profile *construction* paths, which must propagate).
    ///
    /// Takes `&self` but clones `inner` first: `lcms-rs` reads tags lazily and writes the decoded
    /// value back into the tag dictionary (exactly as C's `cmsReadTag` populates `TagPtrs[]`), so
    /// the accessor needs `&mut Profile`. Cloning is cheap and correct rather than wasteful — an
    /// `lcms_rs::Profile` clone duplicates only the tag dictionary and shares one
    /// `Arc<Mutex<dyn IoHandler>>` backing store with the original, so no file or buffer is
    /// copied. Do not "optimize" this into a borrow: it is what lets the public API stay `&self`.
    fn info(&self, kind: InfoType) -> String {
        let mut profile = self.inner.clone();
        get_profile_info_utf8(&mut profile, kind, ANY_LOCALE, ANY_LOCALE).unwrap_or_default()
    }

    /// Returns the color space signature.
    ///
    /// Spelled with the Little CMS 2 `cmsColorSpaceSignature` variant names (`RgbData`,
    /// `XYZData`, `LabData`, `GrayData`, ...) rather than the raw ICC 4CC. `lcms-rs` models
    /// signatures as an untyped [`Signature`] newtype, so the mapping is explicit here; keeping
    /// these exact strings preserves the crate's public output across the lcms2 -> lcms-rs swap.
    /// Unrecognized signatures fall back to their 4-character ICC tag (e.g. `"7CLR"`).
    pub fn color_space(&self) -> String {
        let sig = self.inner.color_space();
        let name = match sig {
            colorspace::XYZ => "XYZData",
            colorspace::LAB => "LabData",
            colorspace::LUV => "LuvData",
            colorspace::YCBCR => "YCbCrData",
            colorspace::YXY => "YxyData",
            colorspace::RGB => "RgbData",
            colorspace::GRAY => "GrayData",
            colorspace::HSV => "HsvData",
            colorspace::HLS => "HlsData",
            colorspace::CMYK => "CmykData",
            colorspace::CMY => "CmyData",
            colorspace::NAMED => "NamedData",
            _ => return sig.to_string(),
        };
        name.to_string()
    }

    /// Returns true if this profile's data color space is `sig`.
    fn is_space(&self, sig: Signature) -> bool {
        self.inner.color_space() == sig
    }

    /// Returns true if this is an RGB profile.
    pub fn is_rgb(&self) -> bool {
        self.is_space(colorspace::RGB)
    }

    /// Returns true if this is a CMYK profile.
    pub fn is_cmyk(&self) -> bool {
        self.is_space(colorspace::CMYK)
    }

    /// Returns true if this is a grayscale profile.
    pub fn is_gray(&self) -> bool {
        self.is_space(colorspace::GRAY)
    }

    /// Exports the profile as ICC data.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use vfx_icc::Profile;
    /// use std::fs;
    ///
    /// let profile = Profile::srgb().unwrap();
    /// let data = profile.to_icc().unwrap();
    /// fs::write("srgb.icc", data).unwrap();
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`IccError::CreateFailed`] if the profile cannot be serialized.
    pub fn to_icc(&self) -> IccResult<Vec<u8>> {
        // Cloned for the same lazy-tag-cache reason as `info()`: serializing forces every
        // still-on-disk tag to be read, which mutates the tag dictionary, so `save_to_mem` takes
        // `&mut self`. The clone shares the backing store (see `info`), so this is not a copy of
        // the profile bytes.
        let mut profile = self.inner.clone();
        profile
            .save_to_mem()
            .map_err(|e| IccError::CreateFailed(e.to_string()))
    }
}

impl std::fmt::Debug for Profile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Profile")
            .field("description", &self.description())
            .field("color_space", &self.color_space())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_srgb() {
        use crate::{Intent, Transform};
        let profile = Profile::srgb().unwrap();
        assert!(profile.is_rgb());
        assert!(!profile.description().is_empty());

        // Prove it is genuinely sRGB (correct TRC), not just "some RGB profile":
        // sRGB EOTF (IEC 61966-2-1) of code 0.5 = ((0.5+0.055)/1.055)^2.4 =
        // 0.214041. An identity / wrong-TRC profile would not produce this.
        let linear = Profile::linear_srgb().unwrap();
        let t = Transform::new(&profile, &linear, Intent::Perceptual).unwrap();
        let mut px = [[0.5f32, 0.5, 0.5]];
        t.apply(&mut px);
        assert!(
            (px[0][0] - 0.214041).abs() < 0.005,
            "sRGB EOTF(0.5) = {}",
            px[0][0]
        );
    }

    #[test]
    fn test_xyz() {
        let profile = Profile::xyz().unwrap();
        assert!(!profile.is_rgb());
        // Pin the actual colour space, not just "not RGB" (which a Lab/Gray/
        // broken profile would also satisfy). lcms2 reports CIE XYZ as XYZData.
        assert_eq!(profile.color_space(), "XYZData");
    }

    #[test]
    fn test_lab() {
        let profile = Profile::lab().unwrap();
        assert!(!profile.is_rgb());
        // CIE L*a*b* must be a Lab profile specifically (lcms2: LabData), not
        // merely non-RGB.
        assert_eq!(profile.color_space(), "LabData");
    }

    #[test]
    fn test_gray() {
        let profile = Profile::gray(2.2).unwrap();
        assert!(profile.is_gray());
        assert_eq!(profile.color_space(), "GrayData");

        // NOTE: the gamma-2.2 TRC cannot be exercised through the public
        // `Transform` API (it is fixed to RGB_FLT and the CMM refuses a 1-channel
        // gray <-> 3-channel RGB device link), so we assert the GrayData class
        // survives an ICC serialize/reload round-trip — a no-op `to_icc` that
        // dropped the colour space would fail this.
        let data = profile.to_icc().unwrap();
        let reloaded = Profile::from_icc(&data).unwrap();
        assert!(
            reloaded.is_gray(),
            "gray classification lost on ICC round-trip"
        );
    }

    /// `Profile::gray` must reject gamma values that cannot describe a usable transfer curve.
    /// `ToneCurve::build_gamma` validates the curve *type*, not the gamma *value*, so a
    /// degenerate gamma would otherwise sail through and produce silently wrong colorimetry.
    /// This is a genuinely reachable failure mode via the public API, unlike the
    /// standard-profile TRC/RGB-build errors which use fixed internal coefficients that cannot
    /// be driven to fail from outside the crate.
    #[test]
    fn test_gray_invalid_gamma() {
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            match Profile::gray(bad) {
                Err(IccError::InvalidGamma(g)) => {
                    assert!(
                        g == bad || (g.is_nan() && bad.is_nan()),
                        "gamma {g} != {bad}"
                    )
                }
                other => panic!("gamma {bad}: expected InvalidGamma, got {other:?}"),
            }
        }
    }

    #[test]
    fn test_to_icc() {
        use crate::{Intent, Transform};
        let profile = Profile::srgb().unwrap();
        let data = profile.to_icc().unwrap();
        assert!(!data.is_empty());

        // Round-trip and verify the COLORIMETRY survives, not just is_rgb():
        // the reloaded profile must still apply the sRGB EOTF (0.5 -> 0.214041),
        // proving to_icc/from_icc preserved the TRC + primaries.
        let reloaded = Profile::from_icc(&data).unwrap();
        assert!(reloaded.is_rgb());
        let linear = Profile::linear_srgb().unwrap();
        let t = Transform::new(&reloaded, &linear, Intent::Perceptual).unwrap();
        let mut px = [[0.5f32, 0.5, 0.5]];
        t.apply(&mut px);
        assert!(
            (px[0][0] - 0.214041).abs() < 0.005,
            "reloaded sRGB EOTF(0.5) = {}",
            px[0][0]
        );
    }
}
