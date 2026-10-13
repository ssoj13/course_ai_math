//! Float to fraction conversion, as OpenImageIO's `fmath.h`.

/// `float_to_rational` (`fmath.h:1312-1331`): `f` as `num/den` by scaling by ten until the
/// fraction part is below 1e-5 (so 52.83 is 5283/100). OIIO's "perfect inverse" branch returns
/// `den = (int)f`, i.e. 0 for any `f < 1`; the intended `den = 1/f` is used here.
///
/// Non-positive and non-finite values give `(0, 1)`. Used by the EXIF / XMP encoders and the DPX
/// writer's pixel aspect ratio.
pub fn float_to_rational(mut f: f32) -> (u32, u32) {
    if f <= 0.0 || !f.is_finite() {
        return (0, 1);
    }
    let inv = 1.0 / f64::from(f);
    if inv.fract() == 0.0 && inv <= f64::from(u32::MAX) {
        return (1, inv as u32);
    }
    let (mut num, mut den) = (f as u32, 1u32);
    while (f - num as f32).abs() > 0.00001 && den < 1_000_000 {
        den *= 10;
        f *= 10.0;
        num = f as u32;
    }
    (num, den)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_examples() {
        assert_eq!(float_to_rational(1.25), (125, 100));
        assert_eq!(float_to_rational(0.0), (0, 1));
        assert_eq!(float_to_rational(0.5), (1, 2));
        assert_eq!(float_to_rational(1.0), (1, 1));
    }
}
