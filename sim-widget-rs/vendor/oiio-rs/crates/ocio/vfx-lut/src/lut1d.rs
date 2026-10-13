//! 1-dimensional lookup table.
//!
//! A 1D LUT applies an independent transfer function to each color channel.
//! Common uses include:
//! - Gamma correction
//! - Log-to-linear conversion
//! - Contrast curves

use crate::{LutError, LutResult};

/// A 1-dimensional lookup table.
///
/// Stores a discrete transfer function that maps input values to output values.
/// Each color channel can have its own curve, or all channels can share one.
///
/// # Structure
///
/// - `size` entries per channel
/// - 1 or 3 channels (mono or RGB)
/// - Linear interpolation between entries
///
/// # Example
///
/// ```rust
/// use vfx_lut::Lut1D;
///
/// // Create a gamma 2.2 curve
/// let lut = Lut1D::gamma(256, 2.2);
///
/// // Apply to a value
/// let output = lut.apply(0.5);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Lut1D {
    /// LUT entries for red channel (or all channels if mono)
    pub r: Vec<f32>,
    /// LUT entries for green channel (None if mono)
    pub g: Option<Vec<f32>>,
    /// LUT entries for blue channel (None if mono)
    pub b: Option<Vec<f32>>,
    /// Input domain minimum per channel [R, G, B]
    pub domain_min: [f32; 3],
    /// Input domain maximum per channel [R, G, B]
    pub domain_max: [f32; 3],
}

impl Lut1D {
    /// Creates an identity (pass-through) 1D LUT.
    ///
    /// # Arguments
    ///
    /// * `size` - Number of entries (typically 256, 1024, or 4096)
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_lut::Lut1D;
    ///
    /// let lut = Lut1D::identity(256);
    /// assert!((lut.apply(0.5) - 0.5).abs() < 0.01);
    /// ```
    pub fn identity(size: usize) -> Self {
        let entries: Vec<f32> = (0..size).map(|i| i as f32 / (size - 1) as f32).collect();
        Self {
            r: entries,
            g: None,
            b: None,
            domain_min: [0.0, 0.0, 0.0],
            domain_max: [1.0, 1.0, 1.0],
        }
    }

    /// Creates a gamma curve LUT.
    ///
    /// # Arguments
    ///
    /// * `size` - Number of entries
    /// * `gamma` - Gamma exponent (e.g., 2.2)
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_lut::Lut1D;
    ///
    /// let lut = Lut1D::gamma(256, 2.2);
    /// ```
    pub fn gamma(size: usize, gamma: f32) -> Self {
        let entries: Vec<f32> = (0..size)
            .map(|i| {
                let t = i as f32 / (size - 1) as f32;
                t.powf(gamma)
            })
            .collect();
        Self {
            r: entries,
            g: None,
            b: None,
            domain_min: [0.0, 0.0, 0.0],
            domain_max: [1.0, 1.0, 1.0],
        }
    }

    /// Creates a LUT from raw data with a uniform domain.
    ///
    /// # Arguments
    ///
    /// * `data` - LUT entries [0, 1]
    /// * `domain_min` - Input domain minimum
    /// * `domain_max` - Input domain maximum
    pub fn from_data(data: Vec<f32>, domain_min: f32, domain_max: f32) -> LutResult<Self> {
        Self::from_data_per_channel(data, [domain_min; 3], [domain_max; 3])
    }

    /// Creates a LUT from raw data with per-channel domain.
    pub fn from_data_per_channel(
        data: Vec<f32>,
        domain_min: [f32; 3],
        domain_max: [f32; 3],
    ) -> LutResult<Self> {
        if data.is_empty() {
            return Err(LutError::InvalidSize("LUT size must be > 0".into()));
        }
        Ok(Self {
            r: data,
            g: None,
            b: None,
            domain_min,
            domain_max,
        })
    }

    /// Creates a 3-channel LUT from separate RGB data with uniform domain.
    pub fn from_rgb(
        r: Vec<f32>,
        g: Vec<f32>,
        b: Vec<f32>,
        domain_min: f32,
        domain_max: f32,
    ) -> LutResult<Self> {
        Self::from_rgb_per_channel(r, g, b, [domain_min; 3], [domain_max; 3])
    }

    /// Creates a 3-channel LUT from separate RGB data with per-channel domain.
    pub fn from_rgb_per_channel(
        r: Vec<f32>,
        g: Vec<f32>,
        b: Vec<f32>,
        domain_min: [f32; 3],
        domain_max: [f32; 3],
    ) -> LutResult<Self> {
        if r.is_empty() || g.is_empty() || b.is_empty() {
            return Err(LutError::InvalidSize("LUT size must be > 0".into()));
        }
        if r.len() != g.len() || r.len() != b.len() {
            return Err(LutError::InvalidSize(
                "RGB channels must have same size".into(),
            ));
        }
        Ok(Self {
            r,
            g: Some(g),
            b: Some(b),
            domain_min,
            domain_max,
        })
    }

    /// Returns the number of entries in the LUT.
    #[inline]
    pub fn size(&self) -> usize {
        self.r.len()
    }

    /// Returns true if this is a single-channel (mono) LUT.
    #[inline]
    pub fn is_mono(&self) -> bool {
        self.g.is_none()
    }

    /// Applies the LUT to a single value using linear interpolation.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_lut::Lut1D;
    ///
    /// let lut = Lut1D::gamma(256, 2.2);
    /// let output = lut.apply(0.5);
    /// ```
    pub fn apply(&self, value: f32) -> f32 {
        self.interpolate(&self.r, value, 0)
    }

    /// Applies the LUT to RGB values.
    ///
    /// If this is a mono LUT, the same curve is applied to all channels.
    /// Uses per-channel domain scaling.
    pub fn apply_rgb(&self, rgb: [f32; 3]) -> [f32; 3] {
        let r = self.interpolate(&self.r, rgb[0], 0);
        let g = self.interpolate(self.g.as_ref().unwrap_or(&self.r), rgb[1], 1);
        let b = self.interpolate(self.b.as_ref().unwrap_or(&self.r), rgb[2], 2);
        [r, g, b]
    }

    /// Linear interpolation in the LUT with per-channel domain.
    fn interpolate(&self, data: &[f32], value: f32, channel: usize) -> f32 {
        let size = data.len();
        if size == 0 {
            return value;
        }

        // Normalize to [0, 1] based on per-channel domain
        let range = self.domain_max[channel] - self.domain_min[channel];
        let t = if range.abs() < 1e-10 {
            0.0
        } else {
            (value - self.domain_min[channel]) / range
        };

        // Convert to index
        let idx_f = t * (size - 1) as f32;
        let idx0 = (idx_f.floor() as usize).min(size - 1);
        let idx1 = (idx0 + 1).min(size - 1);
        let frac = idx_f - idx0 as f32;

        // Linear interpolation
        data[idx0] * (1.0 - frac) + data[idx1] * frac
    }

    /// Inverts the LUT (approximation by reversing the mapping).
    ///
    /// Only works well for monotonic LUTs.
    pub fn invert(&self) -> LutResult<Self> {
        let inverted = self.invert_channel(&self.r)?;
        Ok(Self {
            r: inverted,
            g: self
                .g
                .as_ref()
                .map(|g| self.invert_channel(g))
                .transpose()?,
            b: self
                .b
                .as_ref()
                .map(|b| self.invert_channel(b))
                .transpose()?,
            domain_min: self.domain_min,
            domain_max: self.domain_max,
        })
    }

    fn invert_channel(&self, data: &[f32]) -> LutResult<Vec<f32>> {
        let size = data.len();
        let mut result = vec![0.0; size];
        let max_idx = (size - 1) as f32;

        for (i, out) in result.iter_mut().enumerate() {
            let target = i as f32 / max_idx;

            // Binary search for the interval containing target
            let mut lo = 0;
            let mut hi = size - 1;

            while lo < hi {
                let mid = (lo + hi) / 2;
                if data[mid] < target {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }

            // Linear interpolation between bracketing LUT entries (OCIO-style)
            if lo == 0 || data[lo] == target {
                *out = lo as f32 / max_idx;
            } else {
                let lo_prev = lo - 1;
                let range = data[lo] - data[lo_prev];
                let frac = if range.abs() < 1e-10 {
                    0.0
                } else {
                    (target - data[lo_prev]) / range
                };
                *out = (lo_prev as f32 + frac) / max_idx;
            }
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity() {
        let lut = Lut1D::identity(256);
        assert!((lut.apply(0.0) - 0.0).abs() < 0.01);
        assert!((lut.apply(0.5) - 0.5).abs() < 0.01);
        assert!((lut.apply(1.0) - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_gamma() {
        let lut = Lut1D::gamma(256, 2.0);
        // 0.5^2 = 0.25
        assert!((lut.apply(0.5) - 0.25).abs() < 0.01);
    }

    #[test]
    fn test_rgb() {
        let lut = Lut1D::identity(256);
        let result = lut.apply_rgb([0.5, 0.3, 0.8]);
        assert!((result[0] - 0.5).abs() < 0.01);
        assert!((result[1] - 0.3).abs() < 0.01);
        assert!((result[2] - 0.8).abs() < 0.01);
    }
}
