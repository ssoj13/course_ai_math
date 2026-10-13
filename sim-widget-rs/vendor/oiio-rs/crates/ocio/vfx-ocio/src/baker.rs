//! LUT baking for OCIO processors.
//!
//! Converts color transform chains into lookup tables for use in
//! applications that don't support full OCIO processing.
//!
//! # Example
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use vfx_ocio::{Config, Baker};
//!
//! let config = Config::from_file("config.ocio")?;
//! let processor = config.processor("ACEScg", "sRGB")?;
//!
//! // Bake to 3D LUT
//! let baker = Baker::new(&processor);
//! let lut = baker.bake_lut_3d(33)?;
//!
//! // Export to .cube file
//! baker.write_cube_3d("output.cube", &lut)?;
//! # Ok(())
//! # }
//! ```

use std::io::Write;
use std::path::Path;

use crate::error::{OcioError, OcioResult};
use crate::processor::Processor;

/// LUT export format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LutFormat {
    /// Resolve/DaVinci .cube format (default).
    #[default]
    Cube,
    /// Autodesk/Lustre .3dl format.
    Autodesk3dl,
    /// Sony Pictures Imageworks .spi1d format (1D only).
    Spi1d,
    /// Sony Pictures Imageworks .spi3d format (3D only).
    Spi3d,
    /// Common LUT Format .clf (ACES Academy).
    Clf,
}

impl LutFormat {
    /// Returns the file extension for this format.
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Cube => "cube",
            Self::Autodesk3dl => "3dl",
            Self::Spi1d => "spi1d",
            Self::Spi3d => "spi3d",
            Self::Clf => "clf",
        }
    }

    /// Parses format from file extension.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "cube" => Some(Self::Cube),
            "3dl" => Some(Self::Autodesk3dl),
            "spi1d" => Some(Self::Spi1d),
            "spi3d" => Some(Self::Spi3d),
            "clf" => Some(Self::Clf),
            _ => None,
        }
    }

    /// Returns all supported formats.
    pub fn all() -> &'static [Self] {
        &[
            Self::Cube,
            Self::Autodesk3dl,
            Self::Spi1d,
            Self::Spi3d,
            Self::Clf,
        ]
    }
}

/// Distribution of the sample points a 3D LUT is baked over.
///
/// A 3D LUT is a uniform grid: entry `i` of `size` sits at normalised coordinate
/// `t = i / (size - 1)`. The allocation is the map between that coordinate and the SCENE
/// value the entry was sampled at — OCIO's "allocation", the same concept that
/// [`AllocationTransform`](crate::AllocationTransform) and a config's `allocationvars:`
/// carry.
///
/// # Why `Lg2` exists
///
/// [`Uniform`](Self::Uniform) over `[0, 1]` is right for display-referred input and is
/// what [`Baker::bake_lut_3d`] does, but scene-linear HDR runs far past 1.0 (PQ reaches
/// 100.0 at 10 000 nits, ALEXA LogC ~55, HLG 10). A consumer that samples with its input
/// clamped to `[0, 1]` collapses every one of those values onto the top LUT sample,
/// flattening the entire highlight range into a single colour.
///
/// Widening the domain uniformly is not the fix. `Uniform { min: 0.0, max: 100.0 }` at
/// size 33 puts its second sample at 100/32 = 3.125 — three stops above diffuse white —
/// so the whole shadow range, where the eye is most sensitive, is served by one linear
/// segment out of black. `Lg2 { min: -10.0, max: 7.0 }` puts its second sample at
/// ~0.0014 because it spends samples per STOP rather than per unit: ~2 samples per stop
/// across 0.001 .. 128. Covering the highlights without starving the shadows is the
/// whole reason this variant exists.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Allocation {
    /// Uniform over `[min, max]` — what [`Baker::bake_lut_3d_with_domain`] already does.
    Uniform {
        /// Scene value at LUT coordinate 0.0.
        min: f32,
        /// Scene value at LUT coordinate 1.0.
        max: f32,
    },
    /// Logarithmic: `t = (log2(max(x, 2^min)) - min) / (max - min)`.
    ///
    /// `min` and `max` are EXPONENTS, not scene values: `Lg2 { min: -10.0, max: 7.0 }`
    /// covers `2^-10 .. 2^7`, i.e. 0.000977 .. 128.
    Lg2 {
        /// Exponent of the scene value at LUT coordinate 0.0 (the value is `2^min`).
        min: f32,
        /// Exponent of the scene value at LUT coordinate 1.0 (the value is `2^max`).
        max: f32,
    },
}

impl Allocation {
    /// Scene value -> normalised LUT coordinate in `[0, 1]`.
    ///
    /// `Lg2` SATURATES at the bottom: an input at or below `2^min` — including 0.0 and
    /// every negative, which have no logarithm at all — returns exactly 0.0 instead of
    /// the `-inf`/NaN a bare `log2` would hand to a texture sampler. Above the domain the
    /// coordinate is deliberately NOT clamped: it simply exceeds 1.0, which the sampler
    /// clamps anyway, and leaving it keeps [`to_value`](Self::to_value) an exact
    /// inverse over the whole range where the map is invertible.
    pub fn to_coord(self, v: f32) -> f32 {
        match self {
            Self::Uniform { min, max } => (v - min) / (max - min),
            Self::Lg2 { min, max } => {
                // The log2(0) guard is applied to the SCENE value, so the floor is the
                // domain's own bottom sample rather than an arbitrary epsilon — that is
                // what makes `to_coord(2^min)` come out at exactly 0.0.
                (v.max(min.exp2()).log2() - min) / (max - min)
            }
        }
    }

    /// Normalised LUT coordinate -> scene value; the inverse of
    /// [`to_coord`](Self::to_coord) wherever that map is invertible (i.e. everywhere for
    /// `Uniform`, and at or above `2^min` for `Lg2`).
    pub fn to_value(self, t: f32) -> f32 {
        match self {
            Self::Uniform { min, max } => min + t * (max - min),
            Self::Lg2 { min, max } => (min + t * (max - min)).exp2(),
        }
    }

    /// The allocation a COLOUR SPACE declares for itself, if it declared a usable one.
    ///
    /// THIS IS THE ONE A BAKER SHOULD USUALLY USE. OCIO configs carry `allocation` /
    /// `allocationvars` per colour space precisely so that a tool baking that space into a texture
    /// knows how to distribute its samples — a scene-linear space such as ACEScg declares `lg2`,
    /// a display-referred one declares `uniform`. Asking a human instead would be asking them to
    /// re-state a fact the config already contains, and to get it wrong for half their material.
    ///
    /// `None` when the space declares nothing usable (no vars, or fewer than the two OCIO requires).
    /// The caller should then fall back to `Uniform { min: 0.0, max: 1.0 }`, which is what an
    /// unallocated bake has always done — returning a guessed log range here would silently change
    /// every existing bake.
    ///
    /// OCIO allows a THIRD var, a linear offset applied before the log. It is deliberately not
    /// honoured: [`Allocation`] has no offset term, and quietly dropping it while accepting the
    /// space would misplace every sample. A space that declares one is refused here — better an
    /// honest fallback than a subtly wrong distribution.
    pub fn from_colorspace(cs: &crate::colorspace::ColorSpace) -> Option<Self> {
        use crate::colorspace::{AllocationType, Encoding};

        // 1. AN EXPLICIT `allocationvars` WINS. This is OCIO v1's mechanism and it states the range
        //    exactly, so a config that bothered to say it must be believed over any inference.
        let info = cs.allocation();
        if let [min, max] = *info.vars.as_slice() {
            if min.is_finite() && max.is_finite() && max > min {
                return match info.alloc_type {
                    AllocationType::Uniform => Some(Self::Uniform { min, max }),
                    AllocationType::Log2 => Some(Self::Lg2 { min, max }),
                    // An unrecognised allocation name: no distribution to trust.
                    AllocationType::Unknown => None,
                };
            }
            return None;
        }
        // Any other var count is refused rather than salvaged: THREE means a linear offset, which
        // `Allocation` has no term for, and dropping it would misplace every sample.
        if !info.vars.is_empty() {
            return None;
        }

        // 2. NO VARS: fall back to what OCIO v2 says instead. `allocationvars` is a v1 mechanism and
        //    v2 configs largely dropped it — the shipped ACES studio config declares NONE at all,
        //    describing spaces with `encoding:` instead. Inferring from that is the difference
        //    between this whole mechanism working on a modern config and silently doing nothing.
        match cs.encoding() {
            // Scene-referred values are unbounded above: this is exactly the case a [0,1] LUT
            // cannot hold. -10..7 covers 0.001 .. 128, clearing PQ's 100.0 at 10 000 nits.
            Encoding::SceneLinear => Some(Self::Lg2 {
                min: -10.0,
                max: 7.0,
            }),
            // Everything else is display-referred or already encoded into [0,1] — including `Log`
            // (log-encoded pixels ARE normalised) and `Hdr` (PQ/HLG are display encodings, not
            // scene values). A uniform axis is right, and saying so beats returning None because it
            // records that we KNOW rather than that we gave up.
            Encoding::DisplayLinear | Encoding::Log | Encoding::Sdr | Encoding::Hdr => {
                Some(Self::Uniform { min: 0.0, max: 1.0 })
            }
            // Genuinely unknown, or not colour at all: let the caller decide.
            Encoding::Data | Encoding::Unknown => None,
        }
    }

    /// The `(min, max)` bounds, whatever the variant. Both are exponents for `Lg2`.
    fn bounds(self) -> (f32, f32) {
        match self {
            Self::Uniform { min, max } | Self::Lg2 { min, max } => (min, max),
        }
    }
}

/// Baked 1D LUT data.
#[derive(Debug, Clone)]
pub struct BakedLut1D {
    /// LUT size (number of entries).
    pub size: usize,
    /// Input domain minimum.
    pub domain_min: f32,
    /// Input domain maximum.
    pub domain_max: f32,
    /// LUT data as RGB triplets (size * 3 values).
    pub data: Vec<f32>,
}

/// Baked 3D LUT data.
#[derive(Debug, Clone)]
pub struct BakedLut3D {
    /// LUT size per dimension (e.g., 33 for 33x33x33).
    pub size: usize,
    /// Input domain minimum per channel.
    pub domain_min: [f32; 3],
    /// Input domain maximum per channel.
    pub domain_max: [f32; 3],
    /// How the sample points are DISTRIBUTED between `domain_min` and `domain_max`.
    ///
    /// A consumer must map its input through [`Allocation::to_coord`] before sampling,
    /// or it reads the wrong entry. `domain_min`/`domain_max` alone cannot say this:
    /// they give the two ends, not the spacing between them.
    ///
    /// Bakes made through [`Baker::bake_lut_3d`] and [`Baker::bake_lut_3d_with_domain`]
    /// report `Uniform { min: domain_min[0], max: domain_max[0] }`. That is an exact
    /// description when the three channels share a domain, and a RED-channel
    /// approximation when they do not — the per-channel `domain_min`/`domain_max` stay
    /// the authority for that case.
    pub allocation: Allocation,
    /// LUT data as RGB triplets (size^3 * 3 values).
    /// Ordered: B varies fastest, then G, then R.
    pub data: Vec<f32>,
}

/// LUT baker for converting processors to lookup tables.
#[derive(Debug)]
pub struct Baker<'a> {
    processor: &'a Processor,
    /// Input shaper LUT size (for 3D LUT input linearization).
    shaper_size: usize,
    /// Use shaper for 3D LUT (extends dynamic range).
    use_shaper: bool,
}

impl<'a> Baker<'a> {
    /// Creates a new baker for the given processor.
    pub fn new(processor: &'a Processor) -> Self {
        Self {
            processor,
            shaper_size: 4096,
            use_shaper: false,
        }
    }

    /// Sets the shaper LUT size for 3D LUT baking.
    ///
    /// Default is 4096. Only used when `with_shaper(true)` is set.
    pub fn shaper_size(mut self, size: usize) -> Self {
        self.shaper_size = size;
        self
    }

    /// Enables/disables shaper LUT for extended dynamic range.
    ///
    /// When enabled, bakes a 1D shaper LUT alongside the 3D LUT
    /// to handle HDR or log-encoded input ranges.
    pub fn with_shaper(mut self, enabled: bool) -> Self {
        self.use_shaper = enabled;
        self
    }

    /// Bakes the processor to a 1D LUT.
    ///
    /// # Arguments
    ///
    /// * `size` - Number of LUT entries (typical: 1024, 4096, 65536)
    ///
    /// # Returns
    ///
    /// A `BakedLut1D` containing the sampled transform.
    pub fn bake_lut_1d(&self, size: usize) -> OcioResult<BakedLut1D> {
        self.bake_lut_1d_with_domain(size, 0.0, 1.0)
    }

    /// Bakes the processor to a 1D LUT with custom domain.
    ///
    /// # Arguments
    ///
    /// * `size` - Number of LUT entries
    /// * `domain_min` - Input minimum value
    /// * `domain_max` - Input maximum value
    pub fn bake_lut_1d_with_domain(
        &self,
        size: usize,
        domain_min: f32,
        domain_max: f32,
    ) -> OcioResult<BakedLut1D> {
        if size < 2 {
            return Err(OcioError::Validation("LUT size must be at least 2".into()));
        }

        let mut data = Vec::with_capacity(size * 3);
        let range = domain_max - domain_min;

        for i in 0..size {
            let t = i as f32 / (size - 1) as f32;
            let input = domain_min + t * range;

            // Process each channel independently for 1D LUT
            let mut pixel = [[input, input, input]];
            self.processor.apply_rgb(&mut pixel);

            data.push(pixel[0][0]);
            data.push(pixel[0][1]);
            data.push(pixel[0][2]);
        }

        Ok(BakedLut1D {
            size,
            domain_min,
            domain_max,
            data,
        })
    }

    /// Bakes the processor to a 3D LUT.
    ///
    /// # Arguments
    ///
    /// * `size` - LUT size per dimension (typical: 17, 33, 65)
    ///
    /// # Returns
    ///
    /// A `BakedLut3D` containing the sampled transform.
    pub fn bake_lut_3d(&self, size: usize) -> OcioResult<BakedLut3D> {
        self.bake_lut_3d_with_domain(size, [0.0, 0.0, 0.0], [1.0, 1.0, 1.0])
    }

    /// Bakes the processor to a 3D LUT with custom domain.
    ///
    /// # Arguments
    ///
    /// * `size` - LUT size per dimension
    /// * `domain_min` - Input minimum per channel
    /// * `domain_max` - Input maximum per channel
    pub fn bake_lut_3d_with_domain(
        &self,
        size: usize,
        domain_min: [f32; 3],
        domain_max: [f32; 3],
    ) -> OcioResult<BakedLut3D> {
        if size < 2 {
            return Err(OcioError::Validation("LUT size must be at least 2".into()));
        }

        let total = size * size * size;
        let mut data = Vec::with_capacity(total * 3);

        let range = [
            domain_max[0] - domain_min[0],
            domain_max[1] - domain_min[1],
            domain_max[2] - domain_min[2],
        ];

        // Standard OCIO ordering: B varies fastest, then G, then R
        for r in 0..size {
            let tr = r as f32 / (size - 1) as f32;
            let red = domain_min[0] + tr * range[0];

            for g in 0..size {
                let tg = g as f32 / (size - 1) as f32;
                let green = domain_min[1] + tg * range[1];

                for b in 0..size {
                    let tb = b as f32 / (size - 1) as f32;
                    let blue = domain_min[2] + tb * range[2];

                    let mut pixel = [[red, green, blue]];
                    self.processor.apply_rgb(&mut pixel);

                    data.push(pixel[0][0]);
                    data.push(pixel[0][1]);
                    data.push(pixel[0][2]);
                }
            }
        }

        Ok(BakedLut3D {
            size,
            domain_min,
            domain_max,
            // The honest description of what this loop just did: even steps between the
            // two ends. Only the red channel's bounds fit the single-range shape; see
            // the field's docs.
            allocation: Allocation::Uniform {
                min: domain_min[0],
                max: domain_max[0],
            },
            data,
        })
    }

    /// Bakes the processor to a 3D LUT whose samples follow an [`Allocation`].
    ///
    /// The grid is sampled at `alloc.to_value(t)` for `t` stepping uniformly over
    /// `[0, 1]`, so a consumer that maps its input through [`Allocation::to_coord`]
    /// before sampling reads the entry that was baked for that value.
    ///
    /// This is the bake to use for scene-linear HDR: see [`Allocation`] for why a
    /// uniform domain either clamps the highlights or starves the shadows.
    ///
    /// # Arguments
    ///
    /// * `size` - LUT size per dimension (typical: 17, 33, 65)
    /// * `alloc` - Sample distribution, shared by all three axes
    ///
    /// # Errors
    ///
    /// [`OcioError::Validation`] if `size < 2`, or if the allocation's range is empty,
    /// inverted or non-finite — each of which would make the sample positions NaN or
    /// infinite rather than merely inaccurate.
    pub fn bake_lut_3d_with_allocation(
        &self,
        size: usize,
        alloc: Allocation,
    ) -> OcioResult<BakedLut3D> {
        if size < 2 {
            return Err(OcioError::Validation("LUT size must be at least 2".into()));
        }

        let (min, max) = alloc.bounds();
        // `is_finite` also rejects NaN, so `max <= min` below cannot be reached with an
        // unordered comparison.
        if !min.is_finite() || !max.is_finite() || max <= min {
            return Err(OcioError::Validation(format!(
                "allocation range must be finite and non-empty, got [{min}, {max}]"
            )));
        }

        // One axis shared by R, G and B: the three grid directions must land on the same
        // scene values, and computing it once makes that structural instead of hoped-for.
        let axis: Vec<f32> = (0..size)
            .map(|i| alloc.to_value(i as f32 / (size - 1) as f32))
            .collect();

        let total = size * size * size;
        let mut data = Vec::with_capacity(total * 3);

        // Standard OCIO ordering: B varies fastest, then G, then R
        for &red in &axis {
            for &green in &axis {
                for &blue in &axis {
                    let mut pixel = [[red, green, blue]];
                    self.processor.apply_rgb(&mut pixel);

                    data.push(pixel[0][0]);
                    data.push(pixel[0][1]);
                    data.push(pixel[0][2]);
                }
            }
        }

        // Keep the domain fields truthful for consumers that only read them: they are the
        // scene values at the grid's two ends. They do not describe the spacing between,
        // which is exactly what `allocation` is for.
        let lo = alloc.to_value(0.0);
        let hi = alloc.to_value(1.0);

        Ok(BakedLut3D {
            size,
            domain_min: [lo; 3],
            domain_max: [hi; 3],
            allocation: alloc,
            data,
        })
    }

    /// Writes a 1D LUT to a .cube file.
    pub fn write_cube_1d(&self, path: impl AsRef<Path>, lut: &BakedLut1D) -> OcioResult<()> {
        let mut file = std::fs::File::create(path)?;

        writeln!(file, "# Created by vfx-ocio Baker")?;
        writeln!(file, "TITLE \"Baked 1D LUT\"")?;
        writeln!(file)?;
        writeln!(file, "LUT_1D_SIZE {}", lut.size)?;
        writeln!(
            file,
            "DOMAIN_MIN {:.10} {:.10} {:.10}",
            lut.domain_min, lut.domain_min, lut.domain_min
        )?;
        writeln!(
            file,
            "DOMAIN_MAX {:.10} {:.10} {:.10}",
            lut.domain_max, lut.domain_max, lut.domain_max
        )?;
        writeln!(file)?;

        for i in 0..lut.size {
            let r = lut.data[i * 3];
            let g = lut.data[i * 3 + 1];
            let b = lut.data[i * 3 + 2];
            writeln!(file, "{:.10} {:.10} {:.10}", r, g, b)?;
        }

        Ok(())
    }

    /// Writes a 3D LUT to a .cube file.
    pub fn write_cube_3d(&self, path: impl AsRef<Path>, lut: &BakedLut3D) -> OcioResult<()> {
        let mut file = std::fs::File::create(path)?;

        writeln!(file, "# Created by vfx-ocio Baker")?;
        writeln!(file, "TITLE \"Baked 3D LUT\"")?;
        writeln!(file)?;
        writeln!(file, "LUT_3D_SIZE {}", lut.size)?;
        writeln!(
            file,
            "DOMAIN_MIN {:.10} {:.10} {:.10}",
            lut.domain_min[0], lut.domain_min[1], lut.domain_min[2]
        )?;
        writeln!(
            file,
            "DOMAIN_MAX {:.10} {:.10} {:.10}",
            lut.domain_max[0], lut.domain_max[1], lut.domain_max[2]
        )?;
        writeln!(file)?;

        // `BakedLut3D::data` is Blue-fastest (`bake_lut_3d_with_domain`'s own
        // "Standard OCIO ordering" generation loop; `write_spi3d` below reads
        // it the same way). The `.cube` FILE format is Red-fastest on disk
        // (`FileFormatIridasCube.cpp`, matched by this crate's own
        // `vfx_lut::cube::read_3d`, which reorders on the way IN) — writing
        // `lut.data` straight out with no reorder produced a scrambled
        // `.cube` for any non-identity, non-separable LUT. Reorder once
        // here, the write-side mirror of `read_3d`'s read-side reorder.
        let triplets: Vec<[f32; 3]> = lut
            .data
            .as_chunks::<3>().0.iter()
            .map(|c| [c[0], c[1], c[2]])
            .collect();
        // Tagged (P1 hardening, `OrderedLut3dData`): this call site asserts
        // `triplets` is Blue-fastest and asks for Red-fastest (`.cube`'s
        // on-disk order) out.
        let red_fastest = vfx_lut::OrderedLut3dData::blue_fastest(triplets, lut.size)
            .into_order(vfx_lut::Lut3dOrder::RedFastest);
        for [r, g, b] in red_fastest {
            writeln!(file, "{:.10} {:.10} {:.10}", r, g, b)?;
        }

        Ok(())
    }

    /// Returns whether shaper LUT is enabled.
    pub fn has_shaper(&self) -> bool {
        self.use_shaper
    }

    /// Returns the shaper LUT size.
    pub fn get_shaper_size(&self) -> usize {
        self.shaper_size
    }

    // ========================================================================
    // Additional export formats (OCIO parity)
    // ========================================================================

    /// Writes a 3D LUT to a .3dl file (Autodesk/Lustre format).
    ///
    /// The .3dl format uses 10-bit integer values (0-1023).
    pub fn write_3dl(&self, path: impl AsRef<Path>, lut: &BakedLut3D) -> OcioResult<()> {
        let mut file = std::fs::File::create(path)?;

        // Header
        writeln!(file, "# Created by vfx-ocio Baker")?;
        writeln!(file, "# 3dl format (Autodesk/Lustre)")?;
        writeln!(file)?;

        // Mesh definition: shaper points (we use identity for mesh)
        // Format: 3 values per line representing the input mesh points
        let mesh_size = lut.size;
        for i in 0..mesh_size {
            let val = (i as f32 / (mesh_size - 1) as f32 * 1023.0).round() as i32;
            writeln!(file, "{} {} {}", val, val, val)?;
        }

        writeln!(file)?;

        // 3D LUT data
        let total = lut.size * lut.size * lut.size;
        for i in 0..total {
            let r = (lut.data[i * 3].clamp(0.0, 1.0) * 4095.0).round() as i32;
            let g = (lut.data[i * 3 + 1].clamp(0.0, 1.0) * 4095.0).round() as i32;
            let b = (lut.data[i * 3 + 2].clamp(0.0, 1.0) * 4095.0).round() as i32;
            writeln!(file, " {} {} {}", r, g, b)?;
        }

        Ok(())
    }

    /// Writes a 1D LUT to a .spi1d file (Sony Pictures Imageworks format).
    pub fn write_spi1d(&self, path: impl AsRef<Path>, lut: &BakedLut1D) -> OcioResult<()> {
        let mut file = std::fs::File::create(path)?;

        writeln!(file, "Version 1")?;
        writeln!(file, "From {} {}", lut.domain_min, lut.domain_max)?;
        writeln!(file, "Length {}", lut.size)?;
        writeln!(file, "Components 3")?;
        writeln!(file, "{{")?;

        for i in 0..lut.size {
            let r = lut.data[i * 3];
            let g = lut.data[i * 3 + 1];
            let b = lut.data[i * 3 + 2];
            writeln!(file, "    {:>15.10e} {:>15.10e} {:>15.10e}", r, g, b)?;
        }

        writeln!(file, "}}")?;
        Ok(())
    }

    /// Writes a 3D LUT to a .spi3d file (Sony Pictures Imageworks format).
    pub fn write_spi3d(&self, path: impl AsRef<Path>, lut: &BakedLut3D) -> OcioResult<()> {
        let mut file = std::fs::File::create(path)?;

        writeln!(file, "SPILUT 1.0")?;
        writeln!(file, "3 3")?;
        writeln!(file, "{} {} {}", lut.size, lut.size, lut.size)?;

        // SPI3D ordering: B varies fastest, then G, then R (same as our internal order)
        for r in 0..lut.size {
            for g in 0..lut.size {
                for b in 0..lut.size {
                    let idx = (r * lut.size * lut.size + g * lut.size + b) * 3;
                    writeln!(
                        file,
                        "{} {} {} {:>15.10e} {:>15.10e} {:>15.10e}",
                        r,
                        g,
                        b,
                        lut.data[idx],
                        lut.data[idx + 1],
                        lut.data[idx + 2]
                    )?;
                }
            }
        }

        Ok(())
    }

    /// Writes a 3D LUT to a .clf (Common LUT Format) file (ACES Academy), through the one CLF
    /// writer (`vfx_lut::write_clf`). A non-unit domain becomes a leading `Range`/`Matrix` and an
    /// `Lg2` allocation a leading `Log` node, so the file samples the grid where it was baked.
    pub fn write_clf(&self, path: impl AsRef<Path>, lut: &BakedLut3D) -> OcioResult<()> {
        let mut pl = clf_process_list("vfx-ocio-baked", "Baked 3D LUT from vfx-ocio");
        pl.nodes.extend(clf_lut3d_nodes(lut)?);
        Ok(vfx_lut::write_clf(path.as_ref(), &pl)?)
    }

    /// Writes a 1D LUT to a .clf (Common LUT Format) file.
    pub fn write_clf_1d(&self, path: impl AsRef<Path>, lut: &BakedLut1D) -> OcioResult<()> {
        let mut pl = clf_process_list("vfx-ocio-baked-1d", "Baked 1D LUT from vfx-ocio");
        pl.nodes.push(clf_lut1d_node(lut)?);
        Ok(vfx_lut::write_clf(path.as_ref(), &pl)?)
    }

    /// Writes a combined shaper + 3D LUT to a .clf file.
    ///
    /// This is useful for transforms that need extended dynamic range.
    pub fn write_clf_with_shaper(
        &self,
        path: impl AsRef<Path>,
        shaper: &BakedLut1D,
        lut: &BakedLut3D,
    ) -> OcioResult<()> {
        let mut pl = clf_process_list("vfx-ocio-shaper-lut", "Shaper + 3D LUT from vfx-ocio");
        pl.nodes.push(clf_lut1d_node(shaper)?);
        pl.nodes.extend(clf_lut3d_nodes(lut)?);
        Ok(vfx_lut::write_clf(path.as_ref(), &pl)?)
    }
}

fn clf_process_list(id: &str, description: &str) -> vfx_lut::ProcessList {
    let mut pl = vfx_lut::ProcessList::new(id);
    pl.description = Some(description.into());
    pl
}

/// A baked 1D LUT as a CLF node (the writer emits its domain).
fn clf_lut1d_node(lut: &BakedLut1D) -> OcioResult<vfx_lut::ProcessNode> {
    let channel = |c: usize| {
        (0..lut.size)
            .map(|i| lut.data[i * 3 + c])
            .collect::<Vec<_>>()
    };
    Ok(vfx_lut::ProcessNode::Lut1D {
        lut: vfx_lut::Lut1D::from_rgb(
            channel(0),
            channel(1),
            channel(2),
            lut.domain_min,
            lut.domain_max,
        )?,
        in_depth: Default::default(),
        out_depth: Default::default(),
    })
}

/// A baked 3D LUT as CLF nodes: for an `Lg2` allocation a `LinToLog` node mapping
/// `2^min .. 2^max` onto `0 .. 1` first (`Allocation::to_coord`), else the LUT with its domain.
fn clf_lut3d_nodes(lut: &BakedLut3D) -> OcioResult<Vec<vfx_lut::ProcessNode>> {
    let data: Vec<[f32; 3]> = lut
        .data
        .as_chunks::<3>().0.iter()
        .map(|c| [c[0], c[1], c[2]])
        .collect();
    let grid = vfx_lut::Lut3D::from_data(data, lut.size)?;
    let mut nodes = Vec::new();
    let grid = match lut.allocation {
        Allocation::Lg2 { min, max } => {
            let (min, max) = (min as f64, max as f64);
            nodes.push(vfx_lut::ProcessNode::Log(vfx_lut::clf::LogParams {
                style: vfx_lut::clf::LogStyle::LinToLog,
                base: 2.0,
                log_side_slope: [1.0 / (max - min); 3],
                log_side_offset: [-min / (max - min); 3],
                ..Default::default()
            }));
            grid
        }
        Allocation::Uniform { .. } => grid.with_domain(lut.domain_min, lut.domain_max),
    };
    nodes.push(vfx_lut::ProcessNode::Lut3D {
        lut: grid,
        in_depth: Default::default(),
        out_depth: Default::default(),
    });
    Ok(nodes)
}

impl BakedLut1D {
    /// Returns the LUT data as a slice.
    pub fn as_slice(&self) -> &[f32] {
        &self.data
    }

    /// Returns RGB value at index.
    pub fn get(&self, index: usize) -> Option<[f32; 3]> {
        if index >= self.size {
            return None;
        }
        Some([
            self.data[index * 3],
            self.data[index * 3 + 1],
            self.data[index * 3 + 2],
        ])
    }

    /// Samples the LUT at a normalized position [0,1].
    pub fn sample(&self, t: f32) -> [f32; 3] {
        let t = t.clamp(0.0, 1.0);
        let pos = t * (self.size - 1) as f32;
        let idx = (pos as usize).min(self.size - 2);
        let frac = pos - idx as f32;

        let a = self.get(idx).unwrap();
        let b = self.get(idx + 1).unwrap();

        [
            a[0] + (b[0] - a[0]) * frac,
            a[1] + (b[1] - a[1]) * frac,
            a[2] + (b[2] - a[2]) * frac,
        ]
    }
}

impl BakedLut3D {
    /// Returns the LUT data as a slice.
    pub fn as_slice(&self) -> &[f32] {
        &self.data
    }

    /// Returns RGB value at (r, g, b) indices.
    pub fn get(&self, r: usize, g: usize, b: usize) -> Option<[f32; 3]> {
        if r >= self.size || g >= self.size || b >= self.size {
            return None;
        }
        let idx = (r * self.size * self.size + g * self.size + b) * 3;
        Some([self.data[idx], self.data[idx + 1], self.data[idx + 2]])
    }

    /// Total number of entries (size^3).
    pub fn total_entries(&self) -> usize {
        self.size * self.size * self.size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transform::{CdlTransform, MatrixTransform, Transform, TransformDirection};

    /// A colour space's own `allocationvars` are what a baker should follow — the config states the
    /// distribution so no tool has to guess it. Pin the mapping, including the cases that must NOT
    /// be accepted: silently mishandling one would change how every sample is placed.
    #[test]
    fn allocation_reads_what_a_colour_space_declares() {
        use crate::colorspace::{AllocationInfo, AllocationType, ColorSpace, Encoding};

        let with_vars = |alloc_type, vars: &[f32]| {
            ColorSpace::builder("probe")
                .allocation(AllocationInfo {
                    alloc_type,
                    vars: vars.to_vec(),
                })
                .build()
        };
        let with_encoding = |e| ColorSpace::builder("probe").encoding(e).build();

        // An EXPLICIT v1-style declaration wins and is taken literally.
        assert_eq!(
            Allocation::from_colorspace(&with_vars(AllocationType::Log2, &[-8.0, 5.0])),
            Some(Allocation::Lg2 {
                min: -8.0,
                max: 5.0
            })
        );
        assert_eq!(
            Allocation::from_colorspace(&with_vars(AllocationType::Uniform, &[0.0, 1.0])),
            Some(Allocation::Uniform { min: 0.0, max: 1.0 })
        );

        // NO VARS: infer from OCIO v2's `encoding`. THIS is the path that actually runs on a modern
        // config — the shipped ACES studio config declares no `allocationvars` ANYWHERE, so without
        // it the whole mechanism would fall back to uniform and change nothing at all.
        assert_eq!(
            Allocation::from_colorspace(&with_encoding(Encoding::SceneLinear)),
            Some(Allocation::Lg2 {
                min: -10.0,
                max: 7.0
            }),
            "scene-referred values are unbounded above and NEED the log axis"
        );
        for normalised in [
            Encoding::DisplayLinear,
            Encoding::Sdr,
            // PQ/HLG are DISPLAY encodings: those pixel values are already in [0,1].
            Encoding::Hdr,
            // Log-encoded pixels are normalised too.
            Encoding::Log,
        ] {
            assert_eq!(
                Allocation::from_colorspace(&with_encoding(normalised)),
                Some(Allocation::Uniform { min: 0.0, max: 1.0 }),
                "{normalised:?} is already normalised"
            );
        }
        for silent in [Encoding::Data, Encoding::Unknown] {
            assert_eq!(
                Allocation::from_colorspace(&with_encoding(silent)),
                None,
                "{silent:?} says nothing — let the caller decide"
            );
        }

        // REFUSED, each for its own reason. These do NOT fall through to the encoding: a malformed
        // EXPLICIT declaration is a config error, and inferring past it would hide it.
        for (why, bad) in [
            (
                "one var is not a range",
                with_vars(AllocationType::Log2, &[1.0]),
            ),
            (
                "three vars means a linear offset we cannot express",
                with_vars(AllocationType::Log2, &[-8.0, 5.0, 0.0039]),
            ),
            (
                "inverted range",
                with_vars(AllocationType::Uniform, &[1.0, 0.0]),
            ),
            (
                "empty range",
                with_vars(AllocationType::Uniform, &[0.5, 0.5]),
            ),
            (
                "non-finite bound",
                with_vars(AllocationType::Log2, &[f32::NAN, 5.0]),
            ),
        ] {
            assert_eq!(
                Allocation::from_colorspace(&bad),
                None,
                "must refuse: {why}"
            );
        }
    }

    /// THE SAME FORMULA NOW LIVES IN TWO PLACES: [`Allocation::to_coord`] here, and
    /// `ProcessorOp::Allocation`'s forward branch in `processor/apply.rs` (mirrored again as WGSL
    /// in `gpu/wgsl.rs`). They must not drift, or a LUT baked through this allocation would be
    /// sampled with a coordinate the processor disagrees about — a smoothly wrong picture.
    ///
    /// Rather than merge the types (the transform carries an untyped `vars: Vec<f64>`, which is
    /// OCIO's own on-disk shape and deliberately weaker than this enum), pin that they AGREE.
    ///
    /// Probes sit ABOVE the floor on purpose: that is the one place the two legitimately differ.
    /// `ProcessorOp` clamps the linear value at a fixed `1e-10`, this enum at `2^min` — because a
    /// LUT COORDINATE must land in `[0,1]`, and a `1e-10` floor makes `to_coord(0.0)` return about
    /// -1.366 under `Lg2{-10,7}`, i.e. a negative texture coordinate.
    #[test]
    fn allocation_agrees_with_the_processor_op_above_the_floor() {
        use crate::transform::{AllocationTransform, AllocationType};

        const MIN: f32 = -10.0;
        const MAX: f32 = 7.0;
        let alloc = Allocation::Lg2 { min: MIN, max: MAX };
        let t = Transform::Allocation(AllocationTransform {
            allocation: AllocationType::Log2,
            vars: vec![f64::from(MIN), f64::from(MAX)],
            direction: TransformDirection::Forward,
        });
        let Ok(proc) = Processor::from_transform(&t, TransformDirection::Forward) else {
            panic!("the allocation transform must build a processor")
        };

        // Every probe is at or above 2^-10, where the two floors cannot separate them.
        for &v in &[0.001_f32, 0.0018, 0.18, 1.0, 4.0, 18.0, 100.0, 128.0] {
            let mut pixel = [[v, v, v]];
            proc.apply_rgb(&mut pixel);
            let theirs = pixel[0][0];
            let ours = alloc.to_coord(v);
            assert!(
                (ours - theirs).abs() < 1e-5,
                "allocation drift at {v}: baker {ours} vs ProcessorOp {theirs}"
            );
        }

        // The documented DIVERGENCE is real, not accidental: at 0.0 the processor's 1e-10 floor
        // yields a NEGATIVE coordinate, which is precisely why the baker does not copy it. If this
        // ever starts clamping, the two have converged and the divergence note above is stale.
        let mut zero = [[0.0_f32, 0.0, 0.0]];
        proc.apply_rgb(&mut zero);
        assert!(
            zero[0][0] < 0.0,
            "ProcessorOp is expected to go negative at 0.0"
        );
        assert_eq!(alloc.to_coord(0.0), 0.0, "the baker must saturate at 0.0");
    }

    fn create_test_processor() -> Processor {
        // Simple CDL: slope 1.1, offset 0.01 with NoClamp for full range baking
        let cdl = Transform::Cdl(CdlTransform {
            slope: [1.1, 1.0, 0.9],
            offset: [0.01, 0.0, -0.01],
            power: [1.0, 1.0, 1.0],
            saturation: 1.0,
            style: crate::transform::CdlStyle::NoClamp,
            ..Default::default()
        });
        Processor::from_transform(&cdl, TransformDirection::Forward).unwrap()
    }

    /// The CLF files go through `vfx_lut::write_clf` and sample where they were baked: loaded back
    /// as a FileTransform they reproduce the processor at the grid points, for a shifted 1D domain,
    /// a per-channel 3D domain, an `Lg2` allocation and a shaper. The hand-written writer dropped
    /// the domain and the allocation (every file assumed `[0, 1]`).
    #[test]
    fn clf_files_reproduce_the_processor_at_the_grid() {
        let exp = Transform::Exponent(crate::transform::ExponentTransform {
            name: String::new(),
            value: [2.2, 1.8, 2.6, 1.0],
            negative_style: crate::transform::NegativeStyle::Clamp,
            direction: TransformDirection::Forward,
        });
        let proc = Processor::from_transform(&exp, TransformDirection::Forward).unwrap();
        let baker = Baker::new(&proc);
        let dir = tempfile::tempdir().unwrap();
        let check = |name: &str, path: &std::path::Path, inputs: &[[f32; 3]]| {
            let file =
                Processor::from_transform(&Transform::file(path), TransformDirection::Forward)
                    .unwrap();
            for &x in inputs {
                let (mut want, mut got) = ([x], [x]);
                proc.apply_rgb(&mut want);
                file.apply_rgb(&mut got);
                for c in 0..3 {
                    assert!(
                        (got[0][c] - want[0][c]).abs() < 1e-4 * want[0][c].abs().max(1.0),
                        "{name} {x:?}: {got:?} vs {want:?}"
                    );
                }
            }
        };

        let lut1 = baker.bake_lut_1d_with_domain(9, 0.5, 2.5).unwrap();
        let p1 = dir.path().join("a.clf");
        baker.write_clf_1d(&p1, &lut1).unwrap();
        check("1d", &p1, &[[0.75, 1.25, 2.5], [0.5, 1.0, 2.25]]);

        let lut3 = baker
            .bake_lut_3d_with_domain(5, [0.0, 1.0, 0.0], [2.0, 2.0, 4.0])
            .unwrap();
        let p3 = dir.path().join("b.clf");
        baker.write_clf(&p3, &lut3).unwrap();
        check("3d domain", &p3, &[[0.5, 1.25, 3.0], [2.0, 1.0, 0.0]]);

        let lg2 = baker
            .bake_lut_3d_with_allocation(
                5,
                Allocation::Lg2 {
                    min: -4.0,
                    max: 4.0,
                },
            )
            .unwrap();
        let pl = dir.path().join("c.clf");
        baker.write_clf(&pl, &lg2).unwrap();
        check("3d lg2", &pl, &[[0.25, 1.0, 4.0], [16.0, 0.0625, 1.0]]);

        let shaper = baker.bake_lut_1d(5).unwrap();
        let identity =
            Processor::from_transform(&Transform::group(vec![]), TransformDirection::Forward)
                .unwrap();
        let cube = Baker::new(&identity).bake_lut_3d(2).unwrap();
        let ps = dir.path().join("d.clf");
        baker.write_clf_with_shaper(&ps, &shaper, &cube).unwrap();
        check("shaper", &ps, &[[0.25, 0.5, 1.0], [0.0, 0.75, 0.5]]);
    }

    #[test]
    fn bake_1d_lut() {
        let proc = create_test_processor();
        let baker = Baker::new(&proc);
        let lut = baker.bake_lut_1d(256).unwrap();

        assert_eq!(lut.size, 256);
        assert_eq!(lut.data.len(), 256 * 3);

        // Check first entry (black + offset) with NoClamp style
        // R: 0 * 1.1 + 0.01 = 0.01
        // G: 0 * 1.0 + 0.0 = 0.0
        // B: 0 * 0.9 + (-0.01) = -0.01 (negatives pass through unchanged in NoClamp)
        let first = lut.get(0).unwrap();
        assert!((first[0] - 0.01).abs() < 0.001);
        assert!((first[1] - 0.0).abs() < 0.001);
        assert!((first[2] - (-0.01)).abs() < 0.001);

        // Check last entry (white * slope + offset)
        // R: 1.0 * 1.1 + 0.01 = 1.11
        // G: 1.0 * 1.0 + 0.0 = 1.0
        // B: 1.0 * 0.9 + (-0.01) = 0.89
        let last = lut.get(255).unwrap();
        assert!((last[0] - 1.11).abs() < 0.01);
        assert!((last[1] - 1.0).abs() < 0.01);
        assert!((last[2] - 0.89).abs() < 0.01);
    }

    #[test]
    fn bake_3d_lut() {
        let proc = create_test_processor();
        let baker = Baker::new(&proc);
        let lut = baker.bake_lut_3d(17).unwrap();

        assert_eq!(lut.size, 17);
        assert_eq!(lut.total_entries(), 17 * 17 * 17);
        assert_eq!(lut.data.len(), 17 * 17 * 17 * 3);

        // Check corner: black (0,0,0) with NoClamp style
        // R: 0 * 1.1 + 0.01 = 0.01
        // G: 0 * 1.0 + 0.0 = 0.0
        // B: 0 * 0.9 + (-0.01) = -0.01 (negatives pass through unchanged)
        let black = lut.get(0, 0, 0).unwrap();
        assert!((black[0] - 0.01).abs() < 0.001);
        assert!((black[1] - 0.0).abs() < 0.001);
        assert!((black[2] - (-0.01)).abs() < 0.001);

        // Check corner: white (16,16,16)
        // R: 1.0 * 1.1 + 0.01 = 1.11
        // G: 1.0 * 1.0 + 0.0 = 1.0
        // B: 1.0 * 0.9 + (-0.01) = 0.89
        let white = lut.get(16, 16, 16).unwrap();
        assert!((white[0] - 1.11).abs() < 0.01);
        assert!((white[1] - 1.0).abs() < 0.01);
        assert!((white[2] - 0.89).abs() < 0.01);
    }

    #[test]
    fn bake_1d_with_domain() {
        let proc = create_test_processor();
        let baker = Baker::new(&proc);
        let lut = baker.bake_lut_1d_with_domain(64, -0.1, 1.5).unwrap();

        assert_eq!(lut.domain_min, -0.1);
        assert_eq!(lut.domain_max, 1.5);
    }

    #[test]
    fn bake_3d_with_domain() {
        let proc = create_test_processor();
        let baker = Baker::new(&proc);
        let lut = baker
            .bake_lut_3d_with_domain(9, [-0.1, -0.1, -0.1], [1.5, 1.5, 1.5])
            .unwrap();

        assert_eq!(lut.domain_min, [-0.1, -0.1, -0.1]);
        assert_eq!(lut.domain_max, [1.5, 1.5, 1.5]);
    }

    #[test]
    fn sample_1d_lut() {
        let proc = create_test_processor();
        let baker = Baker::new(&proc);
        let lut = baker.bake_lut_1d(256).unwrap();

        // Sample at 0.5
        let mid = lut.sample(0.5);
        // Expected: 0.5 * 1.1 + 0.01 = 0.56 for red
        assert!((mid[0] - 0.56).abs() < 0.01);
    }

    #[test]
    fn invalid_lut_size() {
        let proc = create_test_processor();
        let baker = Baker::new(&proc);

        assert!(baker.bake_lut_1d(1).is_err());
        assert!(baker.bake_lut_1d(0).is_err());
        assert!(baker.bake_lut_3d(1).is_err());
    }

    #[test]
    fn identity_processor_1d() {
        // Identity matrix should produce identity LUT
        let identity = Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix: [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            offset: [0.0, 0.0, 0.0, 0.0],
            direction: TransformDirection::Forward,
        });
        let proc = Processor::from_transform(&identity, TransformDirection::Forward).unwrap();
        let baker = Baker::new(&proc);
        let lut = baker.bake_lut_1d(64).unwrap();

        for i in 0..64 {
            let t = i as f32 / 63.0;
            let rgb = lut.get(i).unwrap();
            assert!((rgb[0] - t).abs() < 0.0001);
            assert!((rgb[1] - t).abs() < 0.0001);
            assert!((rgb[2] - t).abs() < 0.0001);
        }
    }

    // ========================================================================
    // Allocation
    // ========================================================================

    /// Deep shadow to bright highlight, every value inside `Lg2 { -10, 7 }`
    /// (2^-10 = 0.000977 .. 2^7 = 128) so the log map is invertible on all of them.
    const ALLOC_PROBES: [f32; 7] = [0.001, 0.01, 0.18, 1.0, 5.0, 55.0, 100.0];

    fn alloc_cases() -> [Allocation; 2] {
        [
            Allocation::Uniform {
                min: 0.0,
                max: 100.0,
            },
            Allocation::Lg2 {
                min: -10.0,
                max: 7.0,
            },
        ]
    }

    #[test]
    fn allocation_to_value_inverts_to_coord() {
        for alloc in alloc_cases() {
            for v in ALLOC_PROBES {
                let back = alloc.to_value(alloc.to_coord(v));
                assert!(
                    (back - v).abs() <= v * 1e-5,
                    "{alloc:?}: to_value(to_coord({v})) = {back}"
                );
            }
        }
    }

    #[test]
    fn allocation_to_coord_is_monotonic() {
        for alloc in alloc_cases() {
            let mut prev = f32::NEG_INFINITY;
            for v in ALLOC_PROBES {
                let t = alloc.to_coord(v);
                assert!(
                    t > prev,
                    "{alloc:?}: to_coord({v}) = {t} did not increase past {prev}"
                );
                prev = t;
            }
        }
    }

    /// The endpoints must be EXACT, not merely close: they are what makes the first and
    /// last LUT entries addressable at all.
    #[test]
    fn allocation_endpoints_are_exact() {
        let uniform = Allocation::Uniform {
            min: 0.0,
            max: 100.0,
        };
        assert_eq!(uniform.to_coord(0.0), 0.0);
        assert_eq!(uniform.to_coord(100.0), 1.0);
        assert_eq!(uniform.to_value(0.0), 0.0);
        assert_eq!(uniform.to_value(1.0), 100.0);

        // For Lg2 the endpoints are the SCENE values 2^min and 2^max — exact powers of
        // two, so both directions are representable without rounding.
        let lg2 = Allocation::Lg2 {
            min: -10.0,
            max: 7.0,
        };
        assert_eq!(lg2.to_value(0.0), 0.000_976_562_5);
        assert_eq!(lg2.to_value(1.0), 128.0);
        assert_eq!(lg2.to_coord(0.000_976_562_5), 0.0);
        assert_eq!(lg2.to_coord(128.0), 1.0);
    }

    /// `log2(0)` is `-inf` and negatives have no logarithm at all. Both must saturate at
    /// the bottom of the domain rather than poison the coordinate — this is the trap the
    /// `max(x, 2^min)` in `to_coord` exists for.
    #[test]
    fn allocation_lg2_saturates_below_domain() {
        let lg2 = Allocation::Lg2 {
            min: -10.0,
            max: 7.0,
        };
        for v in [0.0, -0.0, -1.0, 1e-30, f32::MIN] {
            let t = lg2.to_coord(v);
            assert!(t.is_finite(), "to_coord({v}) = {t} is not finite");
            assert_eq!(t, 0.0, "to_coord({v}) = {t}, expected the domain floor");
        }
    }

    /// The reason `Lg2` exists, stated as a number: at size 33 a uniform 0..100 domain
    /// cannot place a single sample in the shadows, while lg2's second sample is still
    /// three stops below 1% grey.
    #[test]
    fn allocation_lg2_beats_uniform_on_shadow_resolution() {
        const SIZE: usize = 33;
        let second = 1.0 / (SIZE - 1) as f32;

        let uniform = Allocation::Uniform {
            min: 0.0,
            max: 100.0,
        }
        .to_value(second);
        let lg2 = Allocation::Lg2 {
            min: -10.0,
            max: 7.0,
        }
        .to_value(second);

        // 100 / 32 = 3.125 — already three stops ABOVE diffuse white.
        assert!(
            (uniform - 3.125).abs() < 1e-4,
            "uniform second sample = {uniform}"
        );
        // 2^(-10 + 17/32) = 2^-9.46875 ~ 0.0014113 — still deep in the shadows.
        assert!(
            (lg2 - 0.001_411_3).abs() < 1e-6,
            "lg2 second sample = {lg2}"
        );
        assert!(
            lg2 * 1000.0 < uniform,
            "lg2's second sample ({lg2}) should be >1000x closer to black than uniform's ({uniform})"
        );
    }

    /// Pins that the bake samples WHERE it claims to: with an identity processor the
    /// entry at grid index `i` must be the scene value the allocation puts at that
    /// coordinate. If the bake ever reverted to stepping the domain linearly, every entry
    /// but the two ends would move.
    #[test]
    fn bake_3d_with_allocation_samples_at_to_value() {
        let identity = Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix: [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            offset: [0.0, 0.0, 0.0, 0.0],
            direction: TransformDirection::Forward,
        });
        let proc = Processor::from_transform(&identity, TransformDirection::Forward).unwrap();
        let alloc = Allocation::Lg2 {
            min: -10.0,
            max: 7.0,
        };
        const SIZE: usize = 9;

        let lut = Baker::new(&proc)
            .bake_lut_3d_with_allocation(SIZE, alloc)
            .unwrap();

        assert_eq!(lut.size, SIZE);
        assert_eq!(lut.allocation, alloc);
        assert_eq!(lut.data.len(), SIZE * SIZE * SIZE * 3);

        for i in 0..SIZE {
            let expect = alloc.to_value(i as f32 / (SIZE - 1) as f32);
            let rgb = lut.get(i, i, i).unwrap();
            for c in rgb {
                assert!(
                    (c - expect).abs() <= expect * 1e-5,
                    "grid index {i}: got {c}, allocation puts that coordinate at {expect}"
                );
            }
        }

        // The domain fields report the grid's ENDS, so a consumer reading only them still
        // gets a truthful range.
        assert_eq!(lut.domain_min, [0.000_976_562_5; 3]);
        assert_eq!(lut.domain_max, [128.0; 3]);
    }

    /// `Uniform` is not new behaviour — it must reproduce `bake_lut_3d_with_domain`
    /// exactly, or the two entry points disagree about what a LUT means. The bounds are
    /// binary-exact so the comparison tests the sampling, not float rounding.
    #[test]
    fn bake_3d_with_uniform_allocation_matches_domain_bake() {
        let proc = create_test_processor();
        let baker = Baker::new(&proc);

        let by_domain = baker
            .bake_lut_3d_with_domain(9, [-0.25; 3], [1.5; 3])
            .unwrap();
        let by_alloc = baker
            .bake_lut_3d_with_allocation(
                9,
                Allocation::Uniform {
                    min: -0.25,
                    max: 1.5,
                },
            )
            .unwrap();

        assert_eq!(by_domain.data, by_alloc.data);
        assert_eq!(by_alloc.domain_min, [-0.25; 3]);
        assert_eq!(by_alloc.domain_max, [1.5; 3]);
    }

    /// The pre-existing entry points must keep DESCRIBING themselves, so a consumer can
    /// apply one rule (`to_coord`) to every LUT this baker produces.
    #[test]
    fn legacy_bakes_report_a_uniform_allocation() {
        let proc = create_test_processor();
        let baker = Baker::new(&proc);

        assert_eq!(
            baker.bake_lut_3d(9).unwrap().allocation,
            Allocation::Uniform { min: 0.0, max: 1.0 }
        );
        assert_eq!(
            baker
                .bake_lut_3d_with_domain(9, [-0.25; 3], [1.5; 3])
                .unwrap()
                .allocation,
            Allocation::Uniform {
                min: -0.25,
                max: 1.5
            }
        );
    }

    /// An empty or inverted range divides by zero and a NaN/inf bound makes every sample
    /// non-finite. Both must be refused up front rather than baked into a LUT that looks
    /// plausible until someone samples it.
    #[test]
    fn bake_3d_with_allocation_rejects_degenerate_input() {
        let proc = create_test_processor();
        let baker = Baker::new(&proc);
        let good = Allocation::Lg2 {
            min: -10.0,
            max: 7.0,
        };

        assert!(baker.bake_lut_3d_with_allocation(1, good).is_err());
        assert!(
            baker
                .bake_lut_3d_with_allocation(9, Allocation::Uniform { min: 1.0, max: 1.0 })
                .is_err()
        );
        assert!(
            baker
                .bake_lut_3d_with_allocation(
                    9,
                    Allocation::Lg2 {
                        min: 7.0,
                        max: -10.0
                    }
                )
                .is_err()
        );
        assert!(
            baker
                .bake_lut_3d_with_allocation(
                    9,
                    Allocation::Uniform {
                        min: 0.0,
                        max: f32::NAN
                    }
                )
                .is_err()
        );
        assert!(
            baker
                .bake_lut_3d_with_allocation(
                    9,
                    Allocation::Uniform {
                        min: 0.0,
                        max: f32::INFINITY
                    }
                )
                .is_err()
        );
        // A good allocation at a good size must still succeed, or the assertions above
        // would pass for a function that rejected everything.
        assert!(baker.bake_lut_3d_with_allocation(9, good).is_ok());
    }

    #[test]
    fn identity_processor_3d() {
        let identity = Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix: [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            offset: [0.0, 0.0, 0.0, 0.0],
            direction: TransformDirection::Forward,
        });
        let proc = Processor::from_transform(&identity, TransformDirection::Forward).unwrap();
        let baker = Baker::new(&proc);
        let lut = baker.bake_lut_3d(9).unwrap();

        // Check diagonal (r=g=b should produce r=g=b)
        for i in 0..9 {
            let t = i as f32 / 8.0;
            let rgb = lut.get(i, i, i).unwrap();
            assert!((rgb[0] - t).abs() < 0.0001);
            assert!((rgb[1] - t).abs() < 0.0001);
            assert!((rgb[2] - t).abs() < 0.0001);
        }
    }

    /// `docs/ROADMAP.md` P0 #1: `write_cube_3d` used to write `BakedLut3D::data`
    /// (Blue-fastest, per this struct's own doc comment and `.get()`'s
    /// `idx = r*size²+g*size+b` — B fastest) straight out to a `.cube`
    /// file with no reorder, but `.cube` is Red-fastest on disk. An
    /// identity/diagonal LUT can't catch this (order-invariant on the
    /// diagonal), so build an asymmetric (R<->B swap) `BakedLut3D` by
    /// hand and check the round trip through `vfx_lut::cube::read_3d`
    /// (already independently verified correct) at an off-diagonal point.
    #[test]
    fn write_cube_3d_survives_asymmetric_roundtrip() {
        let size = 3usize;
        let mut data = vec![0.0f32; size * size * size * 3];
        for r in 0..size {
            for g in 0..size {
                for b in 0..size {
                    let rf = r as f32 / (size - 1) as f32;
                    let gf = g as f32 / (size - 1) as f32;
                    let bf = b as f32 / (size - 1) as f32;
                    // Blue-fastest, matching BakedLut3D::get's own formula.
                    let idx = (r * size * size + g * size + b) * 3;
                    data[idx] = bf;
                    data[idx + 1] = gf;
                    data[idx + 2] = rf; // swap R and B
                }
            }
        }
        let baked = BakedLut3D {
            size,
            domain_min: [0.0; 3],
            domain_max: [1.0; 3],
            allocation: Allocation::Uniform { min: 0.0, max: 1.0 },
            data,
        };

        let identity = Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix: [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            offset: [0.0; 4],
            direction: TransformDirection::Forward,
        });
        let proc = Processor::from_transform(&identity, TransformDirection::Forward).unwrap();
        let baker = Baker::new(&proc);

        // `tempfile::tempdir()`: per-call unique, auto-cleaned — the fixed
        // filename in the shared OS temp dir collided under
        // concurrent/parallel test runs, and the manual `remove_file` never
        // ran on an earlier panic.
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let path = temp_dir.path().join("test_write_cube_3d_asymmetric.cube");
        baker.write_cube_3d(&path, &baked).expect("write failed");
        let loaded = vfx_lut::cube::read_3d(&path).expect("read failed");

        assert_eq!(loaded.size, size);
        // Off-diagonal vertex r=2 (1.0), g=1 (0.5), b=0 (0.0): expect the
        // swapped output (0.0, 0.5, 1.0) at the SAME grid coordinate.
        let idx = size * (1 + size * 2); // (r=2,g=1,b=0), Blue-fastest
        assert_eq!(
            loaded.data[idx],
            [0.0, 0.5, 1.0],
            "a Blue-fastest-vs-Red-fastest mismatch in write_cube_3d would silently \
             swap this to [1.0, 0.5, 0.0]"
        );
    }
}
