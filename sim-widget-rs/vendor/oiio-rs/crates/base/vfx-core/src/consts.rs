//! Single source for the numeric constants that this workspace states more than once.
//!
//! # Why this module exists
//!
//! A constant audit across the colour crates found the same concepts defined in five to seven
//! places each, and the copies had drifted:
//!
//! | Concept                     | Copies | Wrong |
//! |-----------------------------|--------|-------|
//! | `VIDEO_OETF_POWER`          | 6      | 5     |
//! | `MIN_CONTRAST`              | >= 7   | >= 6  |
//! | LogLin block (`M` in it)    | 6+     | 2     |
//!
//! The mechanism is legible and worth stating plainly, because it dictates the house style
//! below: **the copies that COMPUTE are right; the copies that flattened the expression into
//! a LITERAL are wrong.** `1.0 / (0.18 + SHIFT)` survives a refactor because it is
//! self-documenting and re-derives itself if `SHIFT` is corrected. `5.560976` loses its
//! provenance the moment it is typed, and then rots — and indeed the only two LogLin copies
//! in the workspace that spelled `M` as a literal are the only two that are wrong.
//!
//! # House rules for anything added here
//!
//! 1. **Compute, do not flatten.** Where the reference expresses a relationship, encode the
//!    relationship. A literal is acceptable only when the reference itself is a table.
//! 2. **Cite provenance.** Every constant names the reference file and line, or the vendor
//!    document, it came from. A constant nobody can source is a constant nobody can fix.
//! 3. **Match the reference's arithmetic width.** Where a value is reached by computation,
//!    compute it the way the consumers do, so the shared constant is bit-identical to the
//!    copies it replaces (see [`loglin::M`] vs
//!    [`exposure_contrast::VIDEO_OETF_POWER`], which resolve this differently and say why).
//! 4. **Transcendental constants come from `std`, never from a keyboard.** `LOG10_2`,
//!    `LOG2_E`, `PI` and friends are already correctly rounded in `std::f32::consts` /
//!    `std::f64::consts`. Hand-typing them costs an ulp for nothing — `log_op.rs:263` in
//!    `vfx-ocio` currently carries a hand-typed `LOG10_2` that is 1 ulp low, while the line
//!    above it correctly uses `std::f64::consts::LOG2_10`. There is deliberately no
//!    re-export of those here: `std` is already the single source, and wrapping it would
//!    just create a second name for it.
//!
//! # Placement
//!
//! `vfx-core` has no internal dependencies and every other crate in the workspace depends on
//! it, so it is the only home from which `vfx-ops`, `vfx-ocio`, `vfx-transfer`, `vfx-color`
//! and `vfx-math` can all reach these values without risking a cycle. Constants need no
//! dependencies of their own, so nothing is dragged along.

/// CIE standard-illuminant white points, and the two published conventions for them.
///
/// # The problem this solves
///
/// Two different XYZ triples for D65 exist in this workspace and both are legitimate:
///
/// - `vfx-ocio/builtin_transforms.rs` uses Z = 1.08906, derived from the D65 chromaticity
///   (0.3127, 0.3290). This is what OCIO does, so it is what reference-parity paths must use.
/// - ASTM E308 / ICC tabulate Z = 1.08883. (`vfx-math/adapt.rs` carried that value as its `D65`
///   until B13: adapting between tabulated whites while the NPMs use chromaticity whites leaves
///   white off white, so vfx-math no longer has XYZ white constants; RGB work takes whites from
///   `vfx_primaries::white_xyz`.)
///
/// They differ by ~2.3e-4 in Z. Neither is a bug; they answer different questions. But two
/// values under one name with no type-level distinction is exactly the drift mechanism this
/// module exists to stop, so the choice is made explicit: a caller must name the convention,
/// and the call site then says which one it meant.
///
/// ```
/// use vfx_core::consts::white::WhiteConvention;
///
/// // Reference-parity path: match OCIO bit for bit.
/// let parity = WhiteConvention::FromChromaticity.d65();
/// // Colorimetry path: match the published tables.
/// let astm = WhiteConvention::AstmTabulated.d65();
///
/// assert!(parity[2] > astm[2]);
/// ```
pub mod white {
    /// Convert a CIE (x, y) chromaticity to an XYZ triple normalised to Y = 1.
    ///
    /// `X = x / y`, `Y = 1`, `Z = (1 - x - y) / y`. This is the derivation OCIO applies in
    /// `ColorMatrixHelpers.cpp` when it builds white points from chromaticities, and it is
    /// why the derived and tabulated conventions below differ at all.
    ///
    /// Returns all-zeros for `y == 0`, which is not a physical chromaticity; callers reading
    /// chromaticities from a file should reject `y == 0` before getting here rather than
    /// relying on that.
    pub const fn xy_to_xyz(x: f64, y: f64) -> [f64; 3] {
        if y == 0.0 {
            return [0.0, 0.0, 0.0];
        }
        [x / y, 1.0, (1.0 - x - y) / y]
    }

    /// Which published convention a white point's XYZ triple is taken from.
    ///
    /// See the [module docs](self) for why both exist and which belongs where.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum WhiteConvention {
        /// Derived from the illuminant's CIE chromaticity via [`xy_to_xyz`].
        ///
        /// **Use on reference-parity paths.** This is OCIO's convention, so builtin
        /// transforms, ACES output transforms and anything cross-checked against OCIO
        /// goldens must use it or they will miss by ~2e-4 in Z.
        FromChromaticity,

        /// The directly tabulated values published in ASTM E308, as used by ICC.
        ///
        /// **Use in colorimetry.** Lab/Luv conversions, ICC profile handling and anything
        /// quoting a standard's own table should use these, because that is the number the
        /// standard prints.
        AstmTabulated,
    }

    /// D65 chromaticity (x, y) — ITU-R BT.709-6 / BT.2020-2 / sRGB (IEC 61966-2-1).
    pub const D65_XY: [f64; 2] = [0.3127, 0.3290];

    /// D50 chromaticity (x, y) — ICC PCS illuminant.
    pub const D50_XY: [f64; 2] = [0.34567, 0.35850];

    /// D60 / ACES chromaticity (x, y) — SMPTE ST 2065-1, the ACES adopted white.
    pub const D60_XY: [f64; 2] = [0.32168, 0.33767];

    /// DCI theatrical white chromaticity (x, y) — SMPTE RP 431-2.
    pub const DCI_XY: [f64; 2] = [0.3140, 0.3510];

    impl WhiteConvention {
        /// D65 (daylight, ~6500 K) as XYZ with Y = 1.
        ///
        /// - [`FromChromaticity`](Self::FromChromaticity): `[0.95045592705167, 1.0,
        ///   1.08905775075988]`, matching `vfx-ocio/builtin_transforms.rs:2338`.
        /// - [`AstmTabulated`](Self::AstmTabulated): `[0.95047, 1.0, 1.08883]` (ASTM E308).
        pub const fn d65(self) -> [f64; 3] {
            match self {
                Self::FromChromaticity => xy_to_xyz(D65_XY[0], D65_XY[1]),
                Self::AstmTabulated => [0.95047, 1.0, 1.08883],
            }
        }

        /// D50 (horizon light, ~5000 K) as XYZ with Y = 1.
        ///
        /// The two conventions differ in Z by ~2.2e-5 here — smaller than for D65, but the
        /// same split, and ICC work should say so explicitly rather than take whichever
        /// happened to be in scope.
        pub const fn d50(self) -> [f64; 3] {
            match self {
                Self::FromChromaticity => xy_to_xyz(D50_XY[0], D50_XY[1]),
                Self::AstmTabulated => [0.96422, 1.0, 0.82521],
            }
        }

        /// The same triple narrowed to `f32`, for the many call sites that carry `[f32; 3]`.
        ///
        /// Narrowing at the end rather than computing in `f32` keeps the derivation's
        /// precision; `x / y` in `f32` would lose bits before the cast.
        pub const fn d65_f32(self) -> [f32; 3] {
            let v = self.d65();
            [v[0] as f32, v[1] as f32, v[2] as f32]
        }
    }

    /// D60 / ACES white as XYZ with Y = 1: `[0.95264607456985, 1.0, 1.00882518435159]`.
    ///
    /// Single-convention on purpose. The `(0.95265, 1.0, 1.00883)` seen in older tables is the
    /// *same* convention rounded to five places, not a competing one, so D60 needs no
    /// [`WhiteConvention`] split. Matches
    /// `vfx-ocio/builtin_transforms.rs:2337`.
    pub const D60_ACES_XYZ: [f64; 3] = xy_to_xyz(D60_XY[0], D60_XY[1]);

    /// DCI theatrical white as XYZ with Y = 1: `[0.89458689458689, 1.0, 0.95441595441595]`.
    ///
    /// Single-convention: SMPTE RP 431-2 publishes the chromaticity, not a tabulated XYZ.
    /// Matches `vfx-ocio/builtin_transforms.rs:2339`.
    pub const DCI_XYZ: [f64; 3] = xy_to_xyz(DCI_XY[0], DCI_XY[1]);
}

/// OCIO's log-op floor: every log op (`LogOpCPU.cpp:393,426,650,688,891`, `LogOpGPU.cpp:20,130,
/// 272`; the allocation's LG2 and LogAffine / LogCamera are log ops) clamps its argument to
/// `std::numeric_limits<float>::min()` before the log, on the CPU and in the shaders. The
/// vfx-ocio CPU apply, its four GPU emitters and the vfx-compute OCIO shader use this one value
/// (OCIO-M12; the GPU used 1e-10, the vfx-compute shader a subnormal `1.175494e-38`).
pub const LOG_MIN: f32 = f32::MIN_POSITIVE;

/// Exposure/contrast operator constants.
///
/// Reference: OCIO `src/OpenColorIO/ops/exposurecontrast/ExposureContrastOpData.h`.
///
/// This is the worst-drifted family in the workspace: six definitions of
/// [`VIDEO_OETF_POWER`], of which five say `1/2.4`. The one correct copy is
/// `vfx-ops/exposure_contrast.rs`; the CPU path in `vfx-ocio` and all four GPU emitters
/// agree with each other and disagree with both the reference and `vfx-ops`. Because
/// CPU-ocio and GPU currently *match*, migrating them piecemeal would break CPU/GPU parity —
/// all five wrong sites have to move to this module together.
pub mod exposure_contrast {
    /// Video OETF power, an approximation to the BT.709 camera curve.
    ///
    /// `1 / 1.83`. Reference: `ExposureContrastOpData.h:19`, which states it as the double
    /// `0.54644808743169393`.
    ///
    /// Computed in `f64` and narrowed, rather than as `1.0f32 / 1.83f32`: the two differ by
    /// 1 ulp, and this form reproduces the reference's double exactly, which is also what
    /// the one correct existing copy (`vfx-ops/exposure_contrast.rs:14`) does.
    pub const VIDEO_OETF_POWER: f32 = (1.0_f64 / 1.83) as f32;

    /// Floor applied to contrast before use, avoiding a degenerate exponent.
    ///
    /// Reference: `ExposureContrastOpData.h:18` — `0.001`. Note this is **not** `0.0001`;
    /// six sites in the workspace currently say `0.0001`, which changes behaviour for
    /// contrast * gamma in the (0.0001, 0.001) range.
    pub const MIN_CONTRAST: f32 = 0.001;

    /// Floor applied to the pivot, avoiding division by zero.
    ///
    /// Reference: `ExposureContrastOpData.h` `EC::MIN_PIVOT`. The `vfx-ocio` CPU and GPU
    /// exposure/contrast paths apply no pivot floor at all today and divide by a raw pivot,
    /// so migrating them onto this constant also closes a division-by-zero at `pivot == 0`.
    pub const MIN_PIVOT: f32 = 0.001;

    /// Default log-space exposure step, Cineon-style.
    ///
    /// Reference: `ExposureContrastOpData.h:211-216`.
    pub const LOG_EXPOSURE_STEP: f32 = 0.088;

    /// Default log-space mid-grey position.
    ///
    /// Reference: `ExposureContrastOpData.h:211-216`.
    pub const LOG_MIDGRAY: f32 = 0.435;
}

/// The LogLin constant block shared by the grading operators.
///
/// Reference: OCIO `GradingToneOpCPU.cpp:1063-1070` (`LogLinConstants`).
///
/// Used by GradingTone, GradingRGBCurve and GradingHueCurve in the `Linear` style, on both
/// the `vfx-ops` and `vfx-ocio` sides, and re-emitted as shader text by all four GPU
/// backends — six-plus conceptual copies. All of them agree except the two that flattened
/// [`M`] into a literal.
pub mod loglin {
    /// Linear-side breakpoint below which the mapping is affine.
    ///
    /// Reference: `GradingToneOpCPU.cpp:1063-1070` (`0.0041318374739483946`, which rounds to exactly
    /// this `f32`).
    pub const XBRK: f32 = 0.004_131_837_3;

    /// Offset applied to the linear value before the log.
    ///
    /// Reference: `GradingToneOpCPU.cpp:1063-1070` (`0.000157849851665374`, which rounds to exactly
    /// this `f32`).
    pub const SHIFT: f32 = -0.000_157_849_85;

    /// Scale that maps 18% grey to 1.0 through the log leg: `1 / (0.18 + SHIFT)`.
    ///
    /// **Never write this as a literal.** Two sites in `vfx-ops` did — both say `5.560976`,
    /// and the correct value is `5.5604320`. Keeping the expression is what kept the other
    /// four copies (and the GPU shaders, which evaluate `1.0/(0.18+shift)` in shader text)
    /// honest.
    ///
    /// Evaluated in `f32`, matching both the reference and the shader-side computation, so
    /// CPU and GPU stay bit-identical. Computing in `f64` and narrowing would shift this by
    /// 1 ulp and silently break GPU parity.
    pub const M: f32 = 1.0 / (0.18 + SHIFT);

    /// Slope of the affine leg below [`XBRK`].
    ///
    /// Reference: `GradingToneOpCPU.cpp:1063-1070` (`363.034608563`, which rounds to exactly
    /// this `f32`).
    pub const GAIN: f32 = 363.034_6;

    /// Intercept of the affine leg below [`XBRK`].
    ///
    /// Reference: `GradingToneOpCPU.cpp:1063-1070`.
    pub const OFFS: f32 = -7.0;

    /// Log-side breakpoint corresponding to [`XBRK`].
    ///
    /// Reference: `GradingToneOpCPU.cpp:1063-1070`.
    pub const YBRK: f32 = -5.5;

    /// `1 / ln(2)`, converting the natural log to log2.
    ///
    /// The reference states the literal `1.4426950408889634`; `std` already holds the
    /// correctly rounded value, so per house rule 4 it is taken from there.
    pub const BASE2: f32 = std::f32::consts::LOG2_E;
}

/// ACES 1.x fixed-function operator constants, per version.
///
/// Reference: OCIO `FixedFunctionOpCPU.cpp` (RedMod / Glow), mirroring the aces-dev
/// `RRT/ODT` common library.
///
/// These currently live in `vfx-ops/fixed_function/aces1.rs` and **agree** with the
/// reference. They are here because agreement is not the same as safety: the workspace's
/// worst-drifted constants all started out agreeing across their copies. Version blocks stay
/// separate — ACES 0.3/0.7 and 1.0 genuinely differ, and collapsing them would be the same
/// mistake as collapsing the two D65 conventions.
pub mod aces1 {
    use std::f32::consts::PI;

    /// Saturation-weight noise floor, shared by every RedMod and Glow version.
    ///
    /// Reference: `FixedFunctionOpCPU.cpp`.
    pub const NOISE_LIMIT: f32 = 1e-2;

    /// ACES 0.3 / 0.7 Red Modifier.
    pub mod red_mod_03 {
        use super::PI;

        /// Red-channel scale factor.
        pub const SCALE: f32 = 0.85;
        /// `1 - SCALE`, computed so the two can never disagree.
        pub const ONE_MINUS_SCALE: f32 = 1.0 - SCALE;
        /// Hue pivot.
        pub const PIVOT: f32 = 0.03;
        /// Reciprocal hue width: `4 / (120 deg in radians)`.
        ///
        /// Kept as the expression rather than `1.9098593171027443` — the degree figure is
        /// the sourced quantity, the reciprocal is arithmetic.
        pub const INV_WIDTH: f32 = 4.0 / (120.0 * PI / 180.0);
    }

    /// ACES 1.0 Red Modifier.
    pub mod red_mod_10 {
        use super::PI;

        /// Red-channel scale factor.
        pub const SCALE: f32 = 0.82;
        /// `1 - SCALE`, computed so the two can never disagree.
        pub const ONE_MINUS_SCALE: f32 = 1.0 - SCALE;
        /// Hue pivot.
        pub const PIVOT: f32 = 0.03;
        /// Reciprocal hue width: `4 / (135 deg in radians)`.
        pub const INV_WIDTH: f32 = 4.0 / (135.0 * PI / 180.0);
    }

    /// ACES 0.3 / 0.7 Glow.
    pub mod glow_03 {
        /// Glow gain.
        pub const GLOW_GAIN: f32 = 0.075;
        /// Glow mid-point.
        pub const GLOW_MID: f32 = 0.1;
    }

    /// ACES 1.0 Glow.
    pub mod glow_10 {
        /// Glow gain.
        pub const GLOW_GAIN: f32 = 0.05;
        /// Glow mid-point.
        pub const GLOW_MID: f32 = 0.08;
    }
}

/// Narkowicz / Stephen Hill fitted ACES filmic curve coefficients.
///
/// `(x*(A*x+B)) / (x*(C*x+D)+E)` - the rational-fit RRT+ODT approximation (NOT OCIO's ACES2
/// RRT+ODT; see `vfx_ocio::aces2` for that). Used by `vfx-color/src/aces.rs`'s
/// `RrtParams::default`/`aces_fitted` and `vfx_color::tonemap::aces_narkowicz` (which vfx-ops,
/// vfx-warp and vfx-warp's generated display WGSL all evaluate). One SSOT so a future tweak cannot
/// land in only one copy.
pub mod narkowicz {
    /// Shoulder strength.
    pub const A: f32 = 2.51;
    /// Linear section strength.
    pub const B: f32 = 0.03;
    /// Linear angle.
    pub const C: f32 = 2.43;
    /// Toe strength.
    pub const D: f32 = 0.59;
    /// Toe numerator / denominator (both, per the reference fit).
    pub const E: f32 = 0.14;
}

/// Rec.709 luma weights.
///
/// Not redefined here — [`crate::pixel`] already holds the single definition, and adding a
/// second name for it would be the very thing this module exists to prevent. This
/// re-export exists so that the sites currently carrying their own copy
/// (`vfx-ocio/grading_hue_curve.rs:142-144`, and literals inside the GPU CDL/saturation
/// shader text) have one obvious import to migrate onto.
///
/// Reference: ITU-R BT.709-6.
pub mod luma {
    pub use crate::pixel::{REC709_LUMA, REC709_LUMA_B, REC709_LUMA_G, REC709_LUMA_R};
}

#[cfg(test)]
mod tests {
    use super::*;
    use white::WhiteConvention::{AstmTabulated, FromChromaticity};

    /// The derived white points must reproduce `builtin_transforms.rs:2337-2339` exactly,
    /// since parity paths will be migrated onto them.
    #[test]
    fn derived_whites_match_ocio_builtins() {
        let d65 = FromChromaticity.d65();
        assert!((d65[0] - 0.95045592705167).abs() < 1e-14);
        assert!((d65[2] - 1.08905775075988).abs() < 1e-14);

        assert!((white::D60_ACES_XYZ[0] - 0.95264607456985).abs() < 1e-14);
        assert!((white::D60_ACES_XYZ[2] - 1.00882518435159).abs() < 1e-14);

        assert!((white::DCI_XYZ[0] - 0.89458689458689).abs() < 1e-14);
        assert!((white::DCI_XYZ[2] - 0.95441595441595).abs() < 1e-14);
    }

    /// The whole point of the split: the two conventions must be distinguishable, and the
    /// difference must be the ~2.3e-4 in Z that motivated keeping both.
    #[test]
    fn d65_conventions_differ_as_documented() {
        let derived = FromChromaticity.d65();
        let astm = AstmTabulated.d65();
        assert_ne!(derived, astm);
        let dz = derived[2] - astm[2];
        assert!(
            (dz - 2.2775075988e-4).abs() < 1e-12,
            "unexpected delta {dz}"
        );
        // Y is normalised to 1 in both.
        assert_eq!(derived[1], 1.0);
        assert_eq!(astm[1], 1.0);
    }

    #[test]
    fn xy_to_xyz_is_self_consistent() {
        // Equal-energy illuminant E: (1/3, 1/3) -> (1, 1, 1).
        let e = white::xy_to_xyz(1.0 / 3.0, 1.0 / 3.0);
        for v in e {
            assert!((v - 1.0).abs() < 1e-12);
        }
        // Degenerate y is refused rather than producing infinities.
        assert_eq!(white::xy_to_xyz(0.3, 0.0), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn d65_f32_narrows_the_derived_value() {
        let wide = FromChromaticity.d65();
        let narrow = FromChromaticity.d65_f32();
        for (w, n) in wide.iter().zip(narrow.iter()) {
            assert!((*w - *n as f64).abs() < 1e-7);
        }
    }

    /// `VIDEO_OETF_POWER` is `1/1.83`, not `1/2.4`. Five of the six existing copies say the
    /// latter; pin the difference so a migration cannot quietly reintroduce it.
    #[test]
    fn video_oetf_power_is_one_over_1_83() {
        assert_eq!(
            exposure_contrast::VIDEO_OETF_POWER,
            0.546_448_087_431_693_9_f64 as f32
        );
        assert_ne!(exposure_contrast::VIDEO_OETF_POWER, 1.0_f32 / 2.4);
    }

    #[test]
    fn ec_floors_are_thousandths_not_ten_thousandths() {
        assert_eq!(exposure_contrast::MIN_CONTRAST, 0.001);
        assert_eq!(exposure_contrast::MIN_PIVOT, 0.001);
        assert_ne!(exposure_contrast::MIN_CONTRAST, 0.0001);
    }

    /// The defining case for "compute, do not flatten": the shipped literal is wrong in the
    /// fourth significant figure, and the computed form is right.
    #[test]
    fn loglin_m_is_computed_not_the_shipped_literal() {
        assert_eq!(loglin::M, 1.0 / (0.18 + loglin::SHIFT));
        assert_ne!(loglin::M, 5.560976);
        assert!((loglin::M - 5.560_432).abs() < 1e-6, "M = {}", loglin::M);
        // f32 arithmetic, matching the GPU shader text — not f64 narrowed.
        assert_ne!(loglin::M, (1.0_f64 / (0.18 + loglin::SHIFT as f64)) as f32);
    }

    #[test]
    fn loglin_base2_matches_the_reference_literal() {
        // The reference ships this as the decimal literal `1.4426950408889634`, which is exactly
        // `log2(e)` — so it is asserted through `f64::consts::LOG2_E` rather than a re-typed copy
        // of those digits. Same bits, but it names what the number IS, and it stops clippy's
        // deny-level `approx_constant` from aborting the whole lint run (see verify-wave.sh §7:
        // an aborted clippy used to report a flatteringly small warning count).
        assert_eq!(loglin::BASE2, std::f64::consts::LOG2_E as f32);
    }

    #[test]
    fn aces1_inv_widths_match_the_shipped_values() {
        assert_eq!(aces1::red_mod_03::INV_WIDTH, 1.9098593171027443_f64 as f32);
        assert_eq!(aces1::red_mod_10::INV_WIDTH, 1.6976527263135504_f64 as f32);
        // Both are within an ulp of the 0.15 / 0.18 the existing comments quote, and are the
        // same bits the existing `1.0 - SCALE` produces. Exact equality against the decimal
        // would fail (1.0f32 - 0.85f32 == 0.14999998) — which is precisely why the
        // subtraction is kept rather than flattened to a literal.
        assert!((aces1::red_mod_03::ONE_MINUS_SCALE - 0.15).abs() < 1e-7);
        assert!((aces1::red_mod_10::ONE_MINUS_SCALE - 0.18).abs() < 1e-7);
    }

    #[test]
    fn luma_reexport_is_the_same_constant_not_a_copy() {
        assert_eq!(luma::REC709_LUMA, crate::pixel::REC709_LUMA);
        assert_eq!(luma::REC709_LUMA_R, 0.2126);
    }
}
