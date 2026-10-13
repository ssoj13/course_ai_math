//! Opt-in "randomise" for attribute rows: a seedable PCG and the per-field sampling policy.
//!
//! Randomise is a DCC habit (jitter a parameter, explore a look), not something every editor
//! wants, so a row only offers it when its host set a [`Random`] policy on the field
//! ([`crate::AttrField::with_random`]): a "Randomise" entry in the row menu, and `Rs` / `Ra`
//! buttons when the host enables them in [`crate::AttrGridConfig::actions`].
//! No `rand` dependency: the generator is a dozen lines and reproducible from a seed.

/// Seedable PCG-XSH-RR 64/32 generator (O'Neill 2014): fast and statistically solid.
pub(crate) struct Pcg {
    state: u64,
    inc: u64,
}

impl Pcg {
    /// Seed the generator. Any `u64` works; distinct seeds give distinct streams (the caller
    /// seeds from the frame clock and the row key, so every click differs).
    pub(crate) fn new(seed: u64) -> Self {
        // Standard PCG seeding: set the (odd) stream constant, prime the state.
        let mut p = Pcg {
            state: 0,
            inc: (seed.rotate_left(31) << 1) | 1,
        };
        p.next_u32();
        p.state = p.state.wrapping_add(seed ^ 0x9e37_79b9_7f4a_7c15);
        p.next_u32();
        p
    }

    /// Next raw 32-bit value.
    pub(crate) fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(6364136223846793005).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        xorshifted.rotate_right((old >> 59) as u32)
    }

    /// Uniform `f64` in `[0, 1)`.
    pub(crate) fn next_f64(&mut self) -> f64 {
        f64::from(self.next_u32()) / (f64::from(u32::MAX) + 1.0)
    }
}

/// How a field is randomised: the distribution of its numeric samples (per channel for a
/// vector). Booleans flip on a sample >= 0.5; a choice picks uniformly whatever the policy.
#[derive(Clone, Debug, PartialEq)]
pub enum Random {
    /// Uniform over an inclusive numeric range.
    Uniform(std::ops::RangeInclusive<f64>),
    /// Gaussian with the given mean and standard deviation.
    Normal { mean: f64, std: f64 },
}

impl Random {
    /// Uniform over `range`.
    pub fn uniform(range: std::ops::RangeInclusive<f64>) -> Self {
        Random::Uniform(range)
    }

    /// Draw one sample.
    pub(crate) fn sample(&self, rng: &mut Pcg) -> f64 {
        match self {
            Random::Uniform(r) => r.start() + rng.next_f64() * (r.end() - r.start()),
            Random::Normal { mean, std } => {
                // Box-Muller; u1 is kept off 0 so the log stays finite.
                let u1 = rng.next_f64().max(1e-12);
                let u2 = rng.next_f64();
                mean + std * (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
            }
        }
    }
}
