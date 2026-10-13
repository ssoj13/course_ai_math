//! CIECAM02 color appearance model — port of `cmscam02.c`.
//!
//! Bit-exact Rust port of Little-CMS `cmscam02.c` (CIECAM02). The C original threads a
//! `CAM02COLOR` working record through a fixed pipeline of stages and keeps the model
//! parameters in a `cmsCIECAM02` handle. Here the handle becomes an owned [`CIECAM02`]
//! struct (no `malloc`/`cmsHANDLE` indirection, no `cmsCIECAM02Done` — `Drop` is a no-op),
//! and the working record becomes [`Cam02Color`].
//!
//! Fidelity notes (do NOT "clean up"):
//! - The hue geometry uses the hardcoded literal `pi = 3.141592654`, **not** [`std::f64::consts::PI`].
//! - The CAT02 / HPE matrix coefficients and the composed HPE↔CAT02 products are transcribed
//!   verbatim so the compiler folds the exact same IEEE-754 doubles the C compiler does.
//! - The inverse correlate stage keeps every `/1403.0` constant exactly as published.
//!
//! Many thanks (per the C source) to Jordi Vilar for the original debugging.

use crate::types::color::CIEXYZ;

/// Average surround (`AVG_SURROUND`).
pub const AVG_SURROUND: u32 = 1;
/// Dim surround (`DIM_SURROUND`).
pub const DIM_SURROUND: u32 = 2;
/// Dark surround (`DARK_SURROUND`).
pub const DARK_SURROUND: u32 = 3;
/// Cut-sheet surround (`CUTSHEET_SURROUND`).
pub const CUTSHEET_SURROUND: u32 = 4;

/// Sentinel for `ViewingConditions::d_value` requesting the model compute the degree of
/// adaptation itself (`D_CALCULATE`, `-1`).
pub const D_CALCULATE: f64 = -1.0;

/// CIECAM02 appearance correlates (`cmsJCh`): lightness `J`, chroma `C`, hue angle `h`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Jch {
    /// Lightness correlate `J`.
    pub j: f64,
    /// Chroma correlate `C`.
    pub c: f64,
    /// Hue angle `h` (degrees).
    pub h: f64,
}

/// CAM02 viewing conditions (`cmsViewingConditions`).
///
/// Field-for-field the C struct: adopted white point, background relative luminance `Yb`,
/// adapting field luminance `La`, `surround` (one of the `*_SURROUND` constants) and the
/// degree of adaptation `d_value` (use [`D_CALCULATE`] to have the model derive it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewingConditions {
    /// Adopted white point (`whitePoint`).
    pub white_point: CIEXYZ,
    /// Relative luminance of the background (`Yb`).
    pub yb: f64,
    /// Luminance of the adapting field, cd/m² (`La`).
    pub la: f64,
    /// Surround: one of [`AVG_SURROUND`], [`DIM_SURROUND`], [`DARK_SURROUND`],
    /// [`CUTSHEET_SURROUND`] (`surround`).
    pub surround: u32,
    /// Degree of chromatic adaptation, or [`D_CALCULATE`] to compute it (`D_value`).
    pub d_value: f64,
}

/// Working color record threaded through the pipeline (`CAM02COLOR`).
///
/// The C code passes this by value between every stage; here it is `Copy` and each stage
/// consumes and returns it, mirroring `clr = Stage(clr, ...)` exactly. Not all fields feed
/// the public `J`/`C`/`h` output — the remaining CAM02 correlates (`Q`, `M`, `s`, `H`, …)
/// are still computed to preserve the reference pipeline verbatim.
#[derive(Debug, Clone, Copy, Default)]
#[allow(dead_code)] // full CAM02 record kept faithful to C; only J/C/h are surfaced.
struct Cam02Color {
    xyz: [f64; 3],
    rgb: [f64; 3],
    rgbc: [f64; 3],
    rgbp: [f64; 3],
    rgbpa: [f64; 3],
    a: f64,
    b: f64,
    h: f64,
    big_h: f64,
    big_a: f64,
    j: f64,
    q: f64,
    s: f64,
    c: f64,
    m: f64,
}

impl Cam02Color {
    /// `XYZtoCAT02`: linear XYZ → CAT02 cone responses (the MCAT02 matrix).
    fn xyz_to_cat02(mut self) -> Self {
        self.rgb[0] = (self.xyz[0] * 0.7328) + (self.xyz[1] * 0.4296) + (self.xyz[2] * -0.1624);
        self.rgb[1] = (self.xyz[0] * -0.7036) + (self.xyz[1] * 1.6975) + (self.xyz[2] * 0.0061);
        self.rgb[2] = (self.xyz[0] * 0.0030) + (self.xyz[1] * 0.0136) + (self.xyz[2] * 0.9834);
        self
    }

    /// `CAT02toHPE`: CAT02 adapted responses → Hunt–Pointer–Estévez space.
    ///
    /// `M` is the composed product `M_HPE * MCAT02^-1`, transcribed as the exact same
    /// arithmetic expressions as the C source so the folded coefficients match bit-for-bit.
    fn cat02_to_hpe(mut self) -> Self {
        let m: [f64; 9] = [
            (0.38971 * 1.096124) + (0.68898 * 0.454369) + (-0.07868 * -0.009628),
            (0.38971 * -0.278869) + (0.68898 * 0.473533) + (-0.07868 * -0.005698),
            (0.38971 * 0.182745) + (0.68898 * 0.072098) + (-0.07868 * 1.015326),
            (-0.22981 * 1.096124) + (1.18340 * 0.454369) + (0.04641 * -0.009628),
            (-0.22981 * -0.278869) + (1.18340 * 0.473533) + (0.04641 * -0.005698),
            (-0.22981 * 0.182745) + (1.18340 * 0.072098) + (0.04641 * 1.015326),
            -0.009628,
            -0.005698,
            1.015326,
        ];

        self.rgbp[0] = (self.rgbc[0] * m[0]) + (self.rgbc[1] * m[1]) + (self.rgbc[2] * m[2]);
        self.rgbp[1] = (self.rgbc[0] * m[3]) + (self.rgbc[1] * m[4]) + (self.rgbc[2] * m[5]);
        self.rgbp[2] = (self.rgbc[0] * m[6]) + (self.rgbc[1] * m[7]) + (self.rgbc[2] * m[8]);
        self
    }

    /// `HPEtoCAT02`: inverse of [`Cam02Color::cat02_to_hpe`] — HPE responses → CAT02 space.
    fn hpe_to_cat02(mut self) -> Self {
        let m: [f64; 9] = [
            (0.7328 * 1.910197) + (0.4296 * 0.370950),
            (0.7328 * -1.112124) + (0.4296 * 0.629054),
            (0.7328 * 0.201908) + (0.4296 * 0.000008) - 0.1624,
            (-0.7036 * 1.910197) + (1.6975 * 0.370950),
            (-0.7036 * -1.112124) + (1.6975 * 0.629054),
            (-0.7036 * 0.201908) + (1.6975 * 0.000008) + 0.0061,
            (0.0030 * 1.910197) + (0.0136 * 0.370950),
            (0.0030 * -1.112124) + (0.0136 * 0.629054),
            (0.0030 * 0.201908) + (0.0136 * 0.000008) + 0.9834,
        ];

        self.rgbc[0] = (self.rgbp[0] * m[0]) + (self.rgbp[1] * m[1]) + (self.rgbp[2] * m[2]);
        self.rgbc[1] = (self.rgbp[0] * m[3]) + (self.rgbp[1] * m[4]) + (self.rgbp[2] * m[5]);
        self.rgbc[2] = (self.rgbp[0] * m[6]) + (self.rgbp[1] * m[7]) + (self.rgbp[2] * m[8]);
        self
    }

    /// `CAT02toXYZ`: CAT02 responses → linear XYZ (MCAT02^-1).
    fn cat02_to_xyz(mut self) -> Self {
        self.xyz[0] = (self.rgb[0] * 1.096124) + (self.rgb[1] * -0.278869) + (self.rgb[2] * 0.182745);
        self.xyz[1] = (self.rgb[0] * 0.454369) + (self.rgb[1] * 0.473533) + (self.rgb[2] * 0.072098);
        self.xyz[2] = (self.rgb[0] * -0.009628) + (self.rgb[1] * -0.005698) + (self.rgb[2] * 1.015326);
        self
    }
}

/// CIECAM02 model state (`cmsCIECAM02`).
///
/// Built once from a [`ViewingConditions`] via [`CIECAM02::new`] (= `cmsCIECAM02Init`), then
/// used for any number of [`forward`](CIECAM02::forward) / [`reverse`](CIECAM02::reverse)
/// conversions. The malloc/handle indirection of the C API is dropped in favor of an owned
/// value; there is no explicit teardown (`cmsCIECAM02Done` becomes a no-op `Drop`).
#[derive(Debug, Clone, Copy)]
pub struct CIECAM02 {
    /// Adopted white after passing through the forward chain (`adoptedWhite`).
    adopted_white: Cam02Color,
    /// Adapting field luminance (`LA`).
    la: f64,
    /// Background relative luminance (`Yb`).
    yb: f64,
    /// Surround factor `F`.
    f: f64,
    /// Impact of surround `c`.
    c: f64,
    /// Chromatic induction factor `Nc`.
    nc: f64,
    /// Surround selector (`surround`).
    surround: u32,
    /// Background induction factor `n`.
    n: f64,
    /// Brightness/background factor `Nbb`.
    nbb: f64,
    /// Chromatic background factor `Ncb`.
    ncb: f64,
    /// Base exponential nonlinearity `z`.
    z: f64,
    /// Luminance-level adaptation factor `FL`.
    fl: f64,
    /// Degree of chromatic adaptation `D`.
    d: f64,
}

/// The truncated pi literal lcms2 uses throughout CIECAM02 (`3.141592654`), deliberately
/// **not** [`std::f64::consts::PI`] (`3.141592653589793`). The extra precision of the real
/// constant would shift the hue/appearance correlates, so we keep the exact lcms value for
/// bit-parity. The `approx_constant` lint is allowed here for that reason only.
#[allow(clippy::approx_constant)]
const CAM02_PI: f64 = 3.141592654;

impl CIECAM02 {
    /// The surround selector this model was built with (`cmsCIECAM02.surround`):
    /// [`AVG_SURROUND`], [`DIM_SURROUND`], [`DARK_SURROUND`] or [`CUTSHEET_SURROUND`].
    pub fn surround(&self) -> u32 {
        self.surround
    }

    /// `compute_n`: background induction factor = `Yb / whiteY`.
    fn compute_n(&self) -> f64 {
        self.yb / self.adopted_white.xyz[1]
    }

    /// `compute_z`: `1.48 + sqrt(n)`.
    fn compute_z(&self) -> f64 {
        1.48 + self.n.powf(0.5)
    }

    /// `computeNbb`: `0.725 * (1/n)^0.2`.
    fn compute_nbb(&self) -> f64 {
        0.725 * (1.0 / self.n).powf(0.2)
    }

    /// `computeFL`: luminance-level adaptation factor.
    fn compute_fl(&self) -> f64 {
        let k = 1.0 / ((5.0 * self.la) + 1.0);
        0.2 * k.powf(4.0) * (5.0 * self.la)
            + 0.1 * (1.0 - k.powf(4.0)).powf(2.0) * (5.0 * self.la).powf(1.0 / 3.0)
    }

    /// `computeD`: degree of adaptation derived from the adapting luminance.
    fn compute_d(&self) -> f64 {
        let temp = 1.0 - ((1.0 / 3.6) * ((-self.la - 42.0) / 92.0).exp());
        self.f * temp
    }

    /// `ChromaticAdaptation`: CAT02 von-Kries scaling toward the adopted white.
    fn chromatic_adaptation(&self, mut clr: Cam02Color) -> Cam02Color {
        for i in 0..3 {
            clr.rgbc[i] = ((self.adopted_white.xyz[1] * (self.d / self.adopted_white.rgb[i]))
                + (1.0 - self.d))
                * clr.rgb[i];
        }
        clr
    }

    /// `InverseChromaticAdaptation`: undo [`chromatic_adaptation`](Self::chromatic_adaptation).
    fn inverse_chromatic_adaptation(&self, mut clr: Cam02Color) -> Cam02Color {
        for i in 0..3 {
            clr.rgb[i] = clr.rgbc[i]
                / ((self.adopted_white.xyz[1] * self.d / self.adopted_white.rgb[i]) + 1.0 - self.d);
        }
        clr
    }

    /// `NonlinearCompression`: post-adaptation cone compression and achromatic response `A`.
    fn nonlinear_compression(&self, mut clr: Cam02Color) -> Cam02Color {
        for i in 0..3 {
            if clr.rgbp[i] < 0.0 {
                // C: (-1.0 * FL * rgbp/100)^0.42 and (-1.0 * 400 * temp); the `-1.0 *`
                // folds exactly into negation, so `-x` is bit-identical to `-1.0 * x`.
                let temp = (-self.fl * clr.rgbp[i] / 100.0).powf(0.42);
                clr.rgbpa[i] = (-400.0 * temp) / (temp + 27.13) + 0.1;
            } else {
                let temp = (self.fl * clr.rgbp[i] / 100.0).powf(0.42);
                clr.rgbpa[i] = (400.0 * temp) / (temp + 27.13) + 0.1;
            }
        }

        clr.big_a =
            (((2.0 * clr.rgbpa[0]) + clr.rgbpa[1] + (clr.rgbpa[2] / 20.0)) - 0.305) * self.nbb;
        clr
    }

    /// `ComputeCorrelates`: derive the appearance correlates (`J`, `C`, `h`, plus `Q`/`M`/`s`/`H`).
    ///
    /// Uses the hardcoded `pi = 3.141592654` literal for the degree↔radian conversions, as the
    /// C source does — deliberately not [`std::f64::consts::PI`].
    fn compute_correlates(&self, mut clr: Cam02Color) -> Cam02Color {
        let pi = CAM02_PI;

        let a = clr.rgbpa[0] - (12.0 * clr.rgbpa[1] / 11.0) + (clr.rgbpa[2] / 11.0);
        let b = (clr.rgbpa[0] + clr.rgbpa[1] - (2.0 * clr.rgbpa[2])) / 9.0;

        let r2d = 180.0 / pi;
        if a == 0.0 {
            if b == 0.0 {
                clr.h = 0.0;
            } else if b > 0.0 {
                clr.h = 90.0;
            } else {
                clr.h = 270.0;
            }
        } else if a > 0.0 {
            let temp = b / a;
            if b > 0.0 {
                clr.h = r2d * temp.atan();
            } else if b == 0.0 {
                clr.h = 0.0;
            } else {
                clr.h = (r2d * temp.atan()) + 360.0;
            }
        } else {
            let temp = b / a;
            clr.h = (r2d * temp.atan()) + 180.0;
        }

        let d2r = pi / 180.0;
        let e = ((12500.0 / 13.0) * self.nc * self.ncb) * ((clr.h * d2r + 2.0).cos() + 3.8);

        if clr.h < 20.14 {
            let temp = ((clr.h + 122.47) / 1.2) + ((20.14 - clr.h) / 0.8);
            clr.big_h = 300.0 + (100.0 * ((clr.h + 122.47) / 1.2)) / temp;
        } else if clr.h < 90.0 {
            let temp = ((clr.h - 20.14) / 0.8) + ((90.00 - clr.h) / 0.7);
            clr.big_h = (100.0 * ((clr.h - 20.14) / 0.8)) / temp;
        } else if clr.h < 164.25 {
            let temp = ((clr.h - 90.00) / 0.7) + ((164.25 - clr.h) / 1.0);
            clr.big_h = 100.0 + ((100.0 * ((clr.h - 90.00) / 0.7)) / temp);
        } else if clr.h < 237.53 {
            let temp = ((clr.h - 164.25) / 1.0) + ((237.53 - clr.h) / 1.2);
            clr.big_h = 200.0 + ((100.0 * ((clr.h - 164.25) / 1.0)) / temp);
        } else {
            let temp = ((clr.h - 237.53) / 1.2) + ((360.0 - clr.h + 20.14) / 0.8);
            clr.big_h = 300.0 + ((100.0 * ((clr.h - 237.53) / 1.2)) / temp);
        }

        clr.j = 100.0 * (clr.big_a / self.adopted_white.big_a).powf(self.c * self.z);

        clr.q = (4.0 / self.c)
            * (clr.j / 100.0).powf(0.5)
            * (self.adopted_white.big_a + 4.0)
            * self.fl.powf(0.25);

        let t = (e * ((a * a) + (b * b)).powf(0.5))
            / (clr.rgbpa[0] + clr.rgbpa[1] + ((21.0 / 20.0) * clr.rgbpa[2]));

        clr.c = t.powf(0.9) * (clr.j / 100.0).powf(0.5) * (1.64 - 0.29_f64.powf(self.n)).powf(0.73);

        clr.m = clr.c * self.fl.powf(0.25);
        clr.s = 100.0 * (clr.m / clr.q).powf(0.5);

        clr
    }

    /// `InverseCorrelates`: recover the post-compression responses from `J`/`C`/`h`.
    ///
    /// Every `/1403.0` denominator constant is kept exactly as published; `pi = 3.141592654`.
    fn inverse_correlates(&self, mut clr: Cam02Color) -> Cam02Color {
        let pi = CAM02_PI;
        let d2r = pi / 180.0;

        let t = (clr.c
            / ((clr.j / 100.0).powf(0.5) * (1.64 - 0.29_f64.powf(self.n)).powf(0.73)))
            .powf(1.0 / 0.9);
        let e = ((12500.0 / 13.0) * self.nc * self.ncb) * ((clr.h * d2r + 2.0).cos() + 3.8);

        clr.big_a = self.adopted_white.big_a * (clr.j / 100.0).powf(1.0 / (self.c * self.z));

        let p2 = (clr.big_a / self.nbb) + 0.305;

        if t <= 0.0 {
            // Special case from the spec notes: avoid divide-by-zero.
            clr.a = 0.0;
            clr.b = 0.0;
        } else {
            let hr = clr.h * d2r;
            let p1 = e / t;
            let p3 = 21.0 / 20.0;

            if hr.sin().abs() >= hr.cos().abs() {
                let p4 = p1 / hr.sin();
                clr.b = (p2 * (2.0 + p3) * (460.0 / 1403.0))
                    / (p4 + (2.0 + p3) * (220.0 / 1403.0) * (hr.cos() / hr.sin())
                        - (27.0 / 1403.0)
                        + p3 * (6300.0 / 1403.0));
                clr.a = clr.b * (hr.cos() / hr.sin());
            } else {
                let p5 = p1 / hr.cos();
                clr.a = (p2 * (2.0 + p3) * (460.0 / 1403.0))
                    / (p5 + (2.0 + p3) * (220.0 / 1403.0)
                        - ((27.0 / 1403.0) - p3 * (6300.0 / 1403.0)) * (hr.sin() / hr.cos()));
                clr.b = clr.a * (hr.sin() / hr.cos());
            }
        }

        clr.rgbpa[0] = ((460.0 / 1403.0) * p2)
            + ((451.0 / 1403.0) * clr.a)
            + ((288.0 / 1403.0) * clr.b);
        clr.rgbpa[1] = ((460.0 / 1403.0) * p2)
            - ((891.0 / 1403.0) * clr.a)
            - ((261.0 / 1403.0) * clr.b);
        clr.rgbpa[2] = ((460.0 / 1403.0) * p2)
            - ((220.0 / 1403.0) * clr.a)
            - ((6300.0 / 1403.0) * clr.b);
        clr
    }

    /// `InverseNonlinearity`: undo the post-adaptation cone compression.
    fn inverse_nonlinearity(&self, mut clr: Cam02Color) -> Cam02Color {
        for i in 0..3 {
            let c1 = if (clr.rgbpa[i] - 0.1) < 0.0 { -1.0 } else { 1.0 };
            clr.rgbp[i] = c1
                * (100.0 / self.fl)
                * ((27.13 * (clr.rgbpa[i] - 0.1).abs()) / (400.0 - (clr.rgbpa[i] - 0.1).abs()))
                    .powf(1.0 / 0.42);
        }
        clr
    }

    /// `cmsCIECAM02Init`: build the model from viewing conditions.
    ///
    /// Sets `F`/`c`/`Nc` from the surround, derives `n`/`z`/`Nbb`/`FL`, resolves `D` (computing
    /// it when [`D_CALCULATE`] is requested), sets `Ncb = Nbb`, then runs the adopted white
    /// through the forward chain up to the achromatic response so `adoptedWhite.A` is available
    /// to the correlate stages.
    pub fn new(vc: &ViewingConditions) -> Self {
        let mut adopted_white = Cam02Color::default();
        adopted_white.xyz[0] = vc.white_point.x;
        adopted_white.xyz[1] = vc.white_point.y;
        adopted_white.xyz[2] = vc.white_point.z;

        let (f, c, nc) = match vc.surround {
            CUTSHEET_SURROUND => (0.8, 0.41, 0.8),
            DARK_SURROUND => (0.8, 0.525, 0.8),
            DIM_SURROUND => (0.9, 0.59, 0.95),
            // Average surround (also the default for any unrecognized value).
            _ => (1.0, 0.69, 1.0),
        };

        let mut model = CIECAM02 {
            adopted_white,
            la: vc.la,
            yb: vc.yb,
            f,
            c,
            nc,
            surround: vc.surround,
            n: 0.0,
            nbb: 0.0,
            ncb: 0.0,
            z: 0.0,
            fl: 0.0,
            d: vc.d_value,
        };

        model.n = model.compute_n();
        model.z = model.compute_z();
        model.nbb = model.compute_nbb();
        model.fl = model.compute_fl();

        if model.d == D_CALCULATE {
            model.d = model.compute_d();
        }

        model.ncb = model.nbb;

        model.adopted_white = model.adopted_white.xyz_to_cat02();
        model.adopted_white = model.chromatic_adaptation(model.adopted_white);
        model.adopted_white = model.adopted_white.cat02_to_hpe();
        model.adopted_white = model.nonlinear_compression(model.adopted_white);

        model
    }

    /// `cmsCIECAM02Forward`: XYZ → appearance correlates `J`/`C`/`h`.
    pub fn forward(&self, xyz: &CIEXYZ) -> Jch {
        let mut clr = Cam02Color::default();
        clr.xyz[0] = xyz.x;
        clr.xyz[1] = xyz.y;
        clr.xyz[2] = xyz.z;

        clr = clr.xyz_to_cat02();
        clr = self.chromatic_adaptation(clr);
        clr = clr.cat02_to_hpe();
        clr = self.nonlinear_compression(clr);
        clr = self.compute_correlates(clr);

        Jch { j: clr.j, c: clr.c, h: clr.h }
    }

    /// `cmsCIECAM02Reverse`: appearance correlates `J`/`C`/`h` → XYZ.
    pub fn reverse(&self, jch: &Jch) -> CIEXYZ {
        let mut clr = Cam02Color { j: jch.j, c: jch.c, h: jch.h, ..Default::default() };

        clr = self.inverse_correlates(clr);
        clr = self.inverse_nonlinearity(clr);
        clr = clr.hpe_to_cat02();
        clr = self.inverse_chromatic_adaptation(clr);
        clr = clr.cat02_to_xyz();

        CIEXYZ { x: clr.xyz[0], y: clr.xyz[1], z: clr.xyz[2] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Standard-ish viewing condition: D65-relative white (Y=100), La=100, Yb=20, AVG surround.
    fn std_vc() -> ViewingConditions {
        ViewingConditions {
            white_point: CIEXYZ { x: 95.047, y: 100.0, z: 108.883 },
            yb: 20.0,
            la: 100.0,
            surround: AVG_SURROUND,
            d_value: D_CALCULATE,
        }
    }

    /// forward∘reverse must round-trip a mid-gray back to the original XYZ.
    #[test]
    fn round_trip_mid_gray() {
        let model = CIECAM02::new(&std_vc());
        let xyz = CIEXYZ { x: 19.01, y: 20.00, z: 21.78 };

        let jch = model.forward(&xyz);
        let back = model.reverse(&jch);

        // CIECAM02 forward/reverse is an approximate analytic inverse; residual error is
        // float accumulation through the many pow/trig stages (~1e-5), not a logic defect.
        assert!((back.x - xyz.x).abs() < 1e-3, "X: {} vs {}", back.x, xyz.x);
        assert!((back.y - xyz.y).abs() < 1e-3, "Y: {} vs {}", back.y, xyz.y);
        assert!((back.z - xyz.z).abs() < 1e-3, "Z: {} vs {}", back.z, xyz.z);
    }

    /// A brighter, more chromatic sample must also round-trip.
    #[test]
    fn round_trip_chromatic() {
        let model = CIECAM02::new(&std_vc());
        let xyz = CIEXYZ { x: 57.06, y: 43.06, z: 31.96 };

        let jch = model.forward(&xyz);
        let back = model.reverse(&jch);

        assert!((back.x - xyz.x).abs() < 1e-3, "X: {} vs {}", back.x, xyz.x);
        assert!((back.y - xyz.y).abs() < 1e-3, "Y: {} vs {}", back.y, xyz.y);
        assert!((back.z - xyz.z).abs() < 1e-3, "Z: {} vs {}", back.z, xyz.z);
    }

    /// The adopted white must map to lightness `J == 100` exactly: the achromatic response
    /// `A` of the white equals `adoptedWhite.A` bit-for-bit, so `J = 100*(A/Aw)^(cz) = 100`.
    /// (Chroma is NOT zero for a non-equal-energy white like D65: incomplete adaptation
    /// D<1 leaves residual chroma ∝ (1−D), which is correct CIECAM02 behavior.)
    #[test]
    fn white_maps_to_j100() {
        let vc = std_vc();
        let model = CIECAM02::new(&vc);
        let jch = model.forward(&vc.white_point);

        assert!((jch.j - 100.0).abs() < 1e-9, "J of white = {}", jch.j);
    }

    /// A fixed D value (skipping computeD) must be honored verbatim.
    #[test]
    fn explicit_d_value_round_trip() {
        let vc = ViewingConditions {
            white_point: CIEXYZ { x: 95.047, y: 100.0, z: 108.883 },
            yb: 20.0,
            la: 4.0,
            surround: DIM_SURROUND,
            d_value: 0.85,
        };
        let model = CIECAM02::new(&vc);
        assert_eq!(model.d, 0.85);

        let xyz = CIEXYZ { x: 30.0, y: 25.0, z: 15.0 };
        let back = model.reverse(&model.forward(&xyz));
        assert!((back.x - xyz.x).abs() < 1e-3, "X: {} vs {}", back.x, xyz.x);
        assert!((back.y - xyz.y).abs() < 1e-3, "Y: {} vs {}", back.y, xyz.y);
        assert!((back.z - xyz.z).abs() < 1e-3, "Z: {} vs {}", back.z, xyz.z);
    }
}
