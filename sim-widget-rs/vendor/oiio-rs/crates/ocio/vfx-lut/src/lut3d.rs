//! 3-dimensional lookup table.
//!
//! A 3D LUT maps RGB input to RGB output through a cube of color values.
//! Common uses include:
//! - Color grading / Look development
//! - Display calibration
//! - Color space conversion

use crate::{Interpolation, LutError, LutResult};
use vfx_core::runner::{default_runner, rows};

/// Pixels per job of the `_par` batch helpers (and the size below which they stay serial).
const PAR_BAND: usize = 1024;
use wide::f32x4;

/// Reorders a `size^3`-entry 3D LUT cube between OCIO's internal
/// **Blue-fastest** memory order (`Lut3D`'s own contract:
/// `index = b + size*(g + size*r)`, matching
/// `Lut3DOpData::setArrayFromRedFastestOrder`'s `blueFastIdx`) and the
/// **Red-fastest** order most on-disk formats and this crate's own
/// `Lut3DTransform` use (`.cube` files, `Lut3DOpData`'s `redFastIdx`).
///
/// This is the **single conversion point** for the R/B axis-order mismatch
/// documented in `docs/ROADMAP.md` P0 #1 — every caller that needs to move
/// data between the two conventions (compiling a `.cube`-sourced LUT into a
/// GPU/CPU-ready op buffer, wrapping an inline `Lut3DTransform`'s data into
/// a `Lut3D`, writing a `.cube` file from baked Blue-fastest data) should
/// call this rather than re-deriving the index arithmetic — that
/// re-derivation, done wrong at even one of those call sites, silently
/// swaps the R and B axes of every non-identity, non-separable LUT.
///
/// Self-inverse: `swap_red_blue_fastest_order` applied twice returns the
/// original data (it is its own algebraic inverse, not a distinct "reverse"
/// function — see the derivation in the vfx-ocio bughunt report), so the
/// same function converts in EITHER direction.
///
/// # Panics
/// Panics if `data.len() != size * size * size` (a malformed cube should be
/// caught by the caller before reaching here, same as `Lut3D::from_data`).
pub fn swap_red_blue_fastest_order(data: &[[f32; 3]], size: usize) -> Vec<[f32; 3]> {
    assert_eq!(
        data.len(),
        size * size * size,
        "swap_red_blue_fastest_order: data.len() must be size^3"
    );
    let mut out = vec![[0.0f32; 3]; data.len()];
    for r in 0..size {
        for g in 0..size {
            for b in 0..size {
                let blue_fastest_idx = b + size * (g + size * r);
                let red_fastest_idx = r + size * (g + size * b);
                out[red_fastest_idx] = data[blue_fastest_idx];
            }
        }
    }
    out
}

/// Which axis varies fastest in a flattened `size^3`-entry 3D LUT array.
/// See [`swap_red_blue_fastest_order`]'s doc comment for what each order
/// means and where it comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lut3dOrder {
    /// `index = b + size*(g + size*r)` — `vfx_lut::Lut3D`'s own contract,
    /// matching OCIO's internal `Lut3DOpData`.
    BlueFastest,
    /// `index = r + size*(g + size*b)` — `.cube` files on disk,
    /// `Lut3DTransform::data`, `ProcessorOp::Lut3d.lut`, and the GPU texture
    /// upload.
    RedFastest,
}

/// A flattened `size^3`-entry 3D LUT array tagged with its own memory
/// order.
///
/// This is the P1 hardening `docs/ROADMAP.md` (P0 #1 follow-up) called for:
/// "the order is a comment [or a call you must remember to make]" was a
/// real, live trap — the axis-swap bug this crate fixed for P0 #1 came from
/// exactly that (a call site building a Red-fastest buffer straight from
/// Blue-fastest data with no reorder, invisible because both are
/// `Vec<[f32; 3]>`, so nothing at the type level distinguished them).
/// `OrderedLut3dData` makes the order an explicit, checked field instead of
/// an implicit convention: [`Self::into_order`] is a no-op when the data is
/// already in the requested order and calls [`swap_red_blue_fastest_order`]
/// exactly once when it isn't, so a call site can ask for the order it
/// needs without having to remember whether a swap is required THIS time.
///
/// This is a runtime-checked marker, not a compile-time phantom type (which
/// would need `ProcessorOp::Lut3d`'s raw `Vec<f32>` field — used pervasively
/// by CPU apply and all four GPU emitters — to itself carry the order
/// parameter; a bigger, riskier refactor not attempted in this pass). It
/// still closes the specific failure mode that bit P0 #1: a call site that
/// forgets to reorder now has to explicitly construct a WRONG tag to do so
/// (`Self::red_fastest`/`Self::blue_fastest` name which order the caller
/// asserts the data is already in), rather than silently reusing untagged
/// bytes.
#[derive(Debug, Clone)]
pub struct OrderedLut3dData {
    /// The flattened `size^3`-entry cube.
    pub data: Vec<[f32; 3]>,
    /// Cube size (`data.len() == size^3`).
    pub size: usize,
    /// Which order `data` is currently in.
    pub order: Lut3dOrder,
}

impl OrderedLut3dData {
    /// Tags `data` as already Blue-fastest (`vfx_lut::Lut3D`'s contract).
    /// Does NOT reorder — this asserts what the caller already knows about
    /// where `data` came from.
    pub fn blue_fastest(data: Vec<[f32; 3]>, size: usize) -> Self {
        debug_assert_eq!(data.len(), size * size * size);
        Self {
            data,
            size,
            order: Lut3dOrder::BlueFastest,
        }
    }

    /// Tags `data` as already Red-fastest (`.cube`/`ProcessorOp::Lut3d`'s
    /// contract). Does NOT reorder.
    pub fn red_fastest(data: Vec<[f32; 3]>, size: usize) -> Self {
        debug_assert_eq!(data.len(), size * size * size);
        Self {
            data,
            size,
            order: Lut3dOrder::RedFastest,
        }
    }

    /// Returns the data in `want` order, reordering via
    /// [`swap_red_blue_fastest_order`] exactly once if `self.order != want`,
    /// or returning `self.data` unchanged (no copy) if it already matches.
    pub fn into_order(self, want: Lut3dOrder) -> Vec<[f32; 3]> {
        if self.order == want {
            self.data
        } else {
            swap_red_blue_fastest_order(&self.data, self.size)
        }
    }
}

/// Interpolates a 3D lattice of `size^3` RGB entries at lattice coordinates `p`.
///
/// This is the one 3D-LUT interpolation kernel in the stack: [`Lut3D::apply`], vfx-ocio's
/// `Lut3DTransform::apply` and its processor's Lut3D op all call it, each with a `fetch` for its
/// own memory order (`fetch(r, g, b)` returns the entry at lattice indices `r, g, b`).
///
/// `p` is in lattice units, `[0, size - 1]` per axis; values outside are clamped and NaN reads as
/// 0, as in OCIO's `Lut3DOpCPU.cpp` renderers. `Tetrahedral` is OCIO's `Lut3DTetrahedralRenderer`
/// (the six-tetrahedron split on the ordering of the fractional parts); `Linear` is trilinear;
/// `Nearest` rounds to the closest lattice point. Note OCIO v2 itself evaluates a Lut3D's
/// `INTERP_NEAREST` as trilinear (`Lut3DOpData::GetConcreteInterpolation`); mapping OCIO
/// interpolation names to a method is the caller's job.
///
/// `size` must be at least 2.
#[inline]
pub fn interpolate_lattice(
    size: usize,
    p: [f32; 3],
    method: Interpolation,
    fetch: impl Fn(usize, usize, usize) -> [f32; 3],
) -> [f32; 3] {
    debug_assert!(size >= 2, "3D LUT needs at least 2 points per axis");
    let n = (size - 1) as f32;
    // `f32::max` returns the non-NaN operand, so NaN becomes 0 like OCIO's `Clamp`.
    let p = p.map(|v| v.max(0.0).min(n));
    if method == Interpolation::Nearest {
        let i = p.map(|v| v.round() as usize);
        return fetch(i[0], i[1], i[2]);
    }
    // Lower corner, kept one below the top so the upper corner is always in range; at the top
    // edge the fraction becomes 1.0 and selects the upper corner exactly.
    let lo = p.map(|v| (v.floor() as usize).min(size - 2));
    let [fr, fg, fb] = [
        p[0] - lo[0] as f32,
        p[1] - lo[1] as f32,
        p[2] - lo[2] as f32,
    ];
    let ld = |dr: usize, dg: usize, db: usize| -> f32x4 {
        let c = fetch(lo[0] + dr, lo[1] + dg, lo[2] + db);
        f32x4::from([c[0], c[1], c[2], 0.0])
    };
    let (vr, vg, vb) = (f32x4::splat(fr), f32x4::splat(fg), f32x4::splat(fb));
    let c000 = ld(0, 0, 0);
    let c111 = ld(1, 1, 1);
    let v = if method == Interpolation::Tetrahedral {
        if fr > fg {
            if fg > fb {
                let (c100, c110) = (ld(1, 0, 0), ld(1, 1, 0));
                c000 + vr * (c100 - c000) + vg * (c110 - c100) + vb * (c111 - c110)
            } else if fr > fb {
                let (c100, c101) = (ld(1, 0, 0), ld(1, 0, 1));
                c000 + vr * (c100 - c000) + vb * (c101 - c100) + vg * (c111 - c101)
            } else {
                let (c001, c101) = (ld(0, 0, 1), ld(1, 0, 1));
                c000 + vb * (c001 - c000) + vr * (c101 - c001) + vg * (c111 - c101)
            }
        } else if fb > fg {
            let (c001, c011) = (ld(0, 0, 1), ld(0, 1, 1));
            c000 + vb * (c001 - c000) + vg * (c011 - c001) + vr * (c111 - c011)
        } else if fb > fr {
            let (c010, c011) = (ld(0, 1, 0), ld(0, 1, 1));
            c000 + vg * (c010 - c000) + vb * (c011 - c010) + vr * (c111 - c011)
        } else {
            let (c010, c110) = (ld(0, 1, 0), ld(1, 1, 0));
            c000 + vg * (c010 - c000) + vr * (c110 - c010) + vb * (c111 - c110)
        }
    } else {
        let one = f32x4::splat(1.0);
        let lerp = |a: f32x4, b: f32x4, t: f32x4| a * (one - t) + b * t;
        let b0 = lerp(c000, ld(0, 0, 1), vb);
        let b1 = lerp(ld(0, 1, 0), ld(0, 1, 1), vb);
        let b2 = lerp(ld(1, 0, 0), ld(1, 0, 1), vb);
        let b3 = lerp(ld(1, 1, 0), c111, vb);
        lerp(lerp(b0, b1, vg), lerp(b2, b3, vg), vr)
    };
    let a = v.to_array();
    [a[0], a[1], a[2]]
}

/// A 3-dimensional lookup table.
///
/// Stores a cube of RGB values indexed by input RGB. Standard sizes are
/// 17x17x17, 33x33x33, or 65x65x65.
///
/// # Structure
///
/// - `size^3` entries, each containing RGB output values
/// - Stored in Blue-major order (OCIO compatible): B varies fastest, then G, then R
/// - Index formula: `index = B + dim*G + dim²*R`
/// - Trilinear or tetrahedral interpolation for lookup
///
/// # Example
///
/// ```rust
/// use vfx_lut::Lut3D;
///
/// // Create identity LUT
/// let lut = Lut3D::identity(33);
///
/// // Apply to RGB
/// let output = lut.apply([0.5, 0.3, 0.2]);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Lut3D {
    /// LUT data in Blue-major order (OCIO compatible).
    /// Index formula: `B + dim*G + dim²*R` - Blue varies fastest.
    pub data: Vec<[f32; 3]>,
    /// Cube size (typically 17, 33, or 65)
    pub size: usize,
    /// Input domain minimum (per channel)
    pub domain_min: [f32; 3],
    /// Input domain maximum (per channel)
    pub domain_max: [f32; 3],
    /// Interpolation method
    pub interpolation: Interpolation,
}

impl Lut3D {
    /// Creates an identity (pass-through) 3D LUT.
    ///
    /// # Arguments
    ///
    /// * `size` - Cube size (e.g., 33)
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_lut::Lut3D;
    ///
    /// let lut = Lut3D::identity(17);
    /// let result = lut.apply([0.5, 0.3, 0.8]);
    /// assert!((result[0] - 0.5).abs() < 0.1);
    /// ```
    pub fn identity(size: usize) -> Self {
        let total = size * size * size;
        let mut data = Vec::with_capacity(total);

        // OCIO order: R slowest (outer), B fastest (inner)
        for r in 0..size {
            for g in 0..size {
                for b in 0..size {
                    let rf = r as f32 / (size - 1) as f32;
                    let gf = g as f32 / (size - 1) as f32;
                    let bf = b as f32 / (size - 1) as f32;
                    data.push([rf, gf, bf]);
                }
            }
        }

        Self {
            data,
            size,
            domain_min: [0.0, 0.0, 0.0],
            domain_max: [1.0, 1.0, 1.0],
            interpolation: Interpolation::Linear,
        }
    }

    /// Creates a 3D LUT from raw data.
    ///
    /// Data must be in Blue-major order (OCIO compatible) with exactly `size^3` entries.
    ///
    /// # COLOR-14
    ///
    /// `size < 2` is rejected here, at the one constructor every format parser funnels
    /// through (`pandora.rs`, `nuke_vf.rs`, `hdl.rs`, `iridas_look.rs` call this directly with
    /// no size floor of their own — `cube.rs`/`spi.rs`/`threedl.rs` already validate `size >= 2`
    /// before calling, but that upper-layer check is duplicated per-format and easy to miss for
    /// a new one). Without this floor, `size == 0` panics on `self.size - 1` and `size == 1`
    /// panics on `self.size - 2` (trilinear/tetrahedral interpolation) the first time `apply()`
    /// is called on a hostile or truncated LUT file — this is the single point that closes the
    /// gap for every caller at once instead of patching each parser individually.
    pub fn from_data(data: Vec<[f32; 3]>, size: usize) -> LutResult<Self> {
        if size < 2 {
            return Err(LutError::InvalidSize(format!(
                "3D LUT size must be at least 2, got {size}"
            )));
        }
        let expected = size
            .checked_mul(size)
            .and_then(|sq| sq.checked_mul(size))
            .ok_or_else(|| LutError::InvalidSize(format!("3D LUT size {size} overflows")))?;
        if data.len() != expected {
            return Err(LutError::InvalidSize(format!(
                "expected {} entries for size {}, got {}",
                expected,
                size,
                data.len()
            )));
        }
        Ok(Self {
            data,
            size,
            domain_min: [0.0, 0.0, 0.0],
            domain_max: [1.0, 1.0, 1.0],
            interpolation: Interpolation::Linear,
        })
    }

    /// Sets the input domain.
    pub fn with_domain(mut self, min: [f32; 3], max: [f32; 3]) -> Self {
        self.domain_min = min;
        self.domain_max = max;
        self
    }

    /// Sets the interpolation method.
    pub fn with_interpolation(mut self, interp: Interpolation) -> Self {
        self.interpolation = interp;
        self
    }

    /// Returns the total number of entries in the LUT.
    #[inline]
    pub fn entry_count(&self) -> usize {
        self.size * self.size * self.size
    }

    /// Returns the index for a given (r, g, b) grid position.
    /// Uses OCIO Blue-major order: index = B + dim*G + dim²*R
    #[inline]
    fn index(&self, r: usize, g: usize, b: usize) -> usize {
        b + self.size * (g + self.size * r)
    }

    /// Gets the value at grid position (r, g, b).
    #[inline]
    fn get(&self, r: usize, g: usize, b: usize) -> [f32; 3] {
        let idx = self.index(r, g, b);
        self.data[idx]
    }

    /// Applies the LUT to an RGB value.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_lut::Lut3D;
    ///
    /// let lut = Lut3D::identity(33);
    /// let output = lut.apply([0.5, 0.3, 0.2]);
    /// ```
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        let (r, g, b) = self.normalize(rgb);
        let n = (self.size - 1) as f32;
        interpolate_lattice(
            self.size,
            [r * n, g * n, b * n],
            self.interpolation,
            |r, g, b| self.get(r, g, b),
        )
    }

    /// Tetrahedral lookup regardless of `self.interpolation` (the inverse's Newton solve needs a
    /// fixed, piecewise-linear forward model).
    fn apply_tetrahedral(&self, rgb: [f32; 3]) -> [f32; 3] {
        let (r, g, b) = self.normalize(rgb);
        let n = (self.size - 1) as f32;
        interpolate_lattice(
            self.size,
            [r * n, g * n, b * n],
            Interpolation::Tetrahedral,
            |r, g, b| self.get(r, g, b),
        )
    }

    /// Applies the LUT to a batch of RGB pixels.
    ///
    /// Uses the same interpolation method as [`apply`](Self::apply), processing each pixel
    /// sequentially. For parallel processing, use [`apply_batch_par`](Self::apply_batch_par).
    pub fn apply_batch(&self, pixels: &[[f32; 3]]) -> Vec<[f32; 3]> {
        pixels.iter().map(|&p| self.apply(p)).collect()
    }

    /// Applies the LUT to a batch of RGB pixels in parallel, in bands of 1024 pixels on
    /// vfx-core's default runner (rayon's pool with the `rayon` feature, else serial).
    ///
    /// For small batches (<1024 pixels), uses sequential [`apply_batch`](Self::apply_batch) to
    /// avoid thread overhead. Results equal [`apply_batch`](Self::apply_batch) bit for bit.
    pub fn apply_batch_par(&self, pixels: &[[f32; 3]]) -> Vec<[f32; 3]> {
        if pixels.len() < PAR_BAND {
            return self.apply_batch(pixels);
        }
        let mut out = pixels.to_vec();
        let ran = rows(&mut out, PAR_BAND, default_runner(), &|_, band| {
            band.iter_mut().for_each(|p| *p = self.apply(*p));
        });
        // The default runners never fail; were one to, `out` is partly transformed, so redo it.
        match ran {
            Ok(()) => out,
            Err(_) => self.apply_batch(pixels),
        }
    }

    /// Applies the LUT in-place to a flat RGB buffer (3 floats per pixel).
    ///
    /// Buffer length must be divisible by 3.
    pub fn apply_inplace(&self, buf: &mut [f32]) {
        debug_assert!(buf.len().is_multiple_of(3));
        for pixel in buf.as_chunks_mut::<3>().0 {
            let rgb = [pixel[0], pixel[1], pixel[2]];
            let out = self.apply(rgb);
            pixel[0] = out[0];
            pixel[1] = out[1];
            pixel[2] = out[2];
        }
    }

    /// Applies the LUT in-place in parallel, in bands of 1024 pixels on vfx-core's
    /// default runner (rayon's pool with the `rayon` feature, else serial). Equals
    /// [`apply_inplace`](Self::apply_inplace) bit for bit.
    pub fn apply_inplace_par(&self, buf: &mut [f32]) {
        debug_assert!(buf.len().is_multiple_of(3));
        // Bands are whole pixels, so every band applies `apply_inplace` to its own pixels.
        let ran = rows(buf, 3 * PAR_BAND, default_runner(), &|_, band| {
            self.apply_inplace(band);
        });
        // The default runners never fail; were one to, some bands are already transformed and
        // re-applying would transform them twice, so the failure is reported in debug builds.
        debug_assert!(ran.is_ok(), "the default runner failed: {ran:?}");
    }

    /// Normalizes input RGB to [0, 1] based on domain.
    fn normalize(&self, rgb: [f32; 3]) -> (f32, f32, f32) {
        let r = (rgb[0] - self.domain_min[0]) / (self.domain_max[0] - self.domain_min[0]);
        let g = (rgb[1] - self.domain_min[1]) / (self.domain_max[1] - self.domain_min[1]);
        let b = (rgb[2] - self.domain_min[2]) / (self.domain_max[2] - self.domain_min[2]);
        (r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0))
    }

    /// Inverts the 3D LUT using Newton-Raphson iteration.
    ///
    /// Creates a new LUT that approximates the inverse transform.
    /// Works best for monotonic (bijective) LUTs.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_lut::Lut3D;
    ///
    /// let lut = Lut3D::identity(17);
    /// let inv = lut.invert().unwrap();
    /// let rgb = [0.5, 0.3, 0.2];
    /// let fwd = lut.apply(rgb);
    /// let back = inv.apply(fwd);
    /// // back ≈ rgb
    /// ```
    pub fn invert(&self) -> LutResult<Self> {
        let size = self.size;
        let mut inverted = Vec::with_capacity(size * size * size);

        let max_iters = 30;
        let tolerance = 1e-6f32;
        let damping = 0.8f32;

        for bi in 0..size {
            for gi in 0..size {
                for ri in 0..size {
                    // Target: the output we want to achieve
                    let target = [
                        self.domain_min[0]
                            + (ri as f32 / (size - 1) as f32)
                                * (self.domain_max[0] - self.domain_min[0]),
                        self.domain_min[1]
                            + (gi as f32 / (size - 1) as f32)
                                * (self.domain_max[1] - self.domain_min[1]),
                        self.domain_min[2]
                            + (bi as f32 / (size - 1) as f32)
                                * (self.domain_max[2] - self.domain_min[2]),
                    ];

                    // Initial guess: normalized grid position
                    let mut guess = [
                        ri as f32 / (size - 1) as f32,
                        gi as f32 / (size - 1) as f32,
                        bi as f32 / (size - 1) as f32,
                    ];

                    // Newton-Raphson iteration
                    for _ in 0..max_iters {
                        let eval = self.apply_tetrahedral([
                            self.domain_min[0]
                                + guess[0] * (self.domain_max[0] - self.domain_min[0]),
                            self.domain_min[1]
                                + guess[1] * (self.domain_max[1] - self.domain_min[1]),
                            self.domain_min[2]
                                + guess[2] * (self.domain_max[2] - self.domain_min[2]),
                        ]);

                        let err = [
                            eval[0] - target[0],
                            eval[1] - target[1],
                            eval[2] - target[2],
                        ];

                        let err_mag = (err[0] * err[0] + err[1] * err[1] + err[2] * err[2]).sqrt();
                        if err_mag < tolerance {
                            break;
                        }

                        // Compute Jacobian numerically
                        let delta = 1e-4f32;
                        let mut jacobian = [[0.0f32; 3]; 3];

                        for j in 0..3 {
                            let mut g_plus = guess;
                            g_plus[j] = (g_plus[j] + delta).min(1.0);
                            let eval_plus = self.apply_tetrahedral([
                                self.domain_min[0]
                                    + g_plus[0] * (self.domain_max[0] - self.domain_min[0]),
                                self.domain_min[1]
                                    + g_plus[1] * (self.domain_max[1] - self.domain_min[1]),
                                self.domain_min[2]
                                    + g_plus[2] * (self.domain_max[2] - self.domain_min[2]),
                            ]);
                            for i in 0..3 {
                                jacobian[i][j] = (eval_plus[i] - eval[i]) / delta;
                            }
                        }

                        // Solve 3x3 system using Cramer's rule
                        let dx = solve_3x3(&jacobian, &[-err[0], -err[1], -err[2]]);

                        guess[0] = (guess[0] + damping * dx[0]).clamp(0.0, 1.0);
                        guess[1] = (guess[1] + damping * dx[1]).clamp(0.0, 1.0);
                        guess[2] = (guess[2] + damping * dx[2]).clamp(0.0, 1.0);
                    }

                    inverted.push([
                        self.domain_min[0] + guess[0] * (self.domain_max[0] - self.domain_min[0]),
                        self.domain_min[1] + guess[1] * (self.domain_max[1] - self.domain_min[1]),
                        self.domain_min[2] + guess[2] * (self.domain_max[2] - self.domain_min[2]),
                    ]);
                }
            }
        }

        Ok(Self {
            data: inverted,
            size,
            domain_min: self.domain_min,
            domain_max: self.domain_max,
            interpolation: self.interpolation,
        })
    }
}

/// Solves 3x3 linear system Ax = b using Cramer's rule.
fn solve_3x3(a: &[[f32; 3]; 3], b: &[f32; 3]) -> [f32; 3] {
    let det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
        - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);

    if det.abs() < 1e-10 {
        return [0.0, 0.0, 0.0];
    }

    let det_x = b[0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
        - a[0][1] * (b[1] * a[2][2] - a[1][2] * b[2])
        + a[0][2] * (b[1] * a[2][1] - a[1][1] * b[2]);

    let det_y = a[0][0] * (b[1] * a[2][2] - a[1][2] * b[2])
        - b[0] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * b[2] - b[1] * a[2][0]);

    let det_z = a[0][0] * (a[1][1] * b[2] - b[1] * a[2][1])
        - a[0][1] * (a[1][0] * b[2] - b[1] * a[2][0])
        + b[0] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);

    [det_x / det, det_y / det, det_z / det]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `into_order` must be a true no-op (byte-identical, not a
    /// swap-and-swap-back) when the data is already in the requested order —
    /// asking for the order you're already in should never touch the bytes.
    #[test]
    fn ordered_lut3d_into_same_order_is_a_noop() {
        let size = 3;
        let asymmetric: Vec<[f32; 3]> = (0..size * size * size)
            .map(|i| [i as f32, (i * 2) as f32, (i * 3) as f32])
            .collect();

        let tagged = OrderedLut3dData::blue_fastest(asymmetric.clone(), size);
        let result = tagged.into_order(Lut3dOrder::BlueFastest);
        assert_eq!(result, asymmetric);
    }

    /// Asking for the OTHER order must match calling
    /// `swap_red_blue_fastest_order` directly — the tagged API is a thin
    /// wrapper, not a different algorithm.
    #[test]
    fn ordered_lut3d_into_other_order_matches_raw_swap() {
        let size = 3;
        let asymmetric: Vec<[f32; 3]> = (0..size * size * size)
            .map(|i| [i as f32, (i * 2) as f32, (i * 3) as f32])
            .collect();

        let via_wrapper = OrderedLut3dData::blue_fastest(asymmetric.clone(), size)
            .into_order(Lut3dOrder::RedFastest);
        let via_raw_fn = swap_red_blue_fastest_order(&asymmetric, size);
        assert_eq!(via_wrapper, via_raw_fn);
    }

    /// Round-tripping through both tags must recover the original data —
    /// the P1 hardening must not change the underlying (already-proven
    /// self-inverse) swap semantics.
    #[test]
    fn ordered_lut3d_round_trip_recovers_original() {
        let size = 4;
        let asymmetric: Vec<[f32; 3]> = (0..size * size * size)
            .map(|i| [i as f32, (i * 7) as f32 % 5.0, (i * 3) as f32 % 11.0])
            .collect();

        let red = OrderedLut3dData::blue_fastest(asymmetric.clone(), size)
            .into_order(Lut3dOrder::RedFastest);
        let back = OrderedLut3dData::red_fastest(red, size).into_order(Lut3dOrder::BlueFastest);
        assert_eq!(back, asymmetric);
    }

    /// COLOR-14 regression: `size < 2` must be rejected by the constructor, not panic later
    /// inside `apply()` via a `usize` underflow on `size - 1` / `size - 2`.
    #[test]
    fn test_from_data_rejects_degenerate_sizes() {
        assert!(Lut3D::from_data(vec![], 0).is_err());
        assert!(Lut3D::from_data(vec![[0.0, 0.0, 0.0]], 1).is_err());
        // size 2 is the smallest legal cube and must still succeed.
        let data = vec![[0.0, 0.0, 0.0]; 8];
        let lut = Lut3D::from_data(data, 2).unwrap();
        // Exercises both trilinear and tetrahedral without underflowing.
        let _ = lut.apply([0.5, 0.5, 0.5]);
    }

    #[test]
    fn test_identity() {
        let lut = Lut3D::identity(17);
        let result = lut.apply([0.5, 0.3, 0.8]);
        assert!((result[0] - 0.5).abs() < 0.1);
        assert!((result[1] - 0.3).abs() < 0.1);
        assert!((result[2] - 0.8).abs() < 0.1);
    }

    #[test]
    fn test_corners() {
        let lut = Lut3D::identity(33);

        // Black
        let black = lut.apply([0.0, 0.0, 0.0]);
        assert!((black[0]).abs() < 0.01);

        // White
        let white = lut.apply([1.0, 1.0, 1.0]);
        assert!((white[0] - 1.0).abs() < 0.01);

        // Red
        let red = lut.apply([1.0, 0.0, 0.0]);
        assert!((red[0] - 1.0).abs() < 0.01);
        assert!((red[1]).abs() < 0.01);
    }

    #[test]
    fn test_tetrahedral() {
        let lut = Lut3D::identity(33).with_interpolation(Interpolation::Tetrahedral);
        let result = lut.apply([0.5, 0.3, 0.8]);
        assert!((result[0] - 0.5).abs() < 0.1);
        assert!((result[1] - 0.3).abs() < 0.1);
        assert!((result[2] - 0.8).abs() < 0.1);
    }

    #[test]
    fn test_from_data() {
        let data: Vec<[f32; 3]> = (0..8).map(|_| [0.5, 0.5, 0.5]).collect();
        let lut = Lut3D::from_data(data, 2).unwrap();
        let result = lut.apply([0.5, 0.5, 0.5]);
        assert_eq!(result, [0.5, 0.5, 0.5]);
    }

    #[test]
    fn test_batch() {
        let lut = Lut3D::identity(33);
        let pixels = vec![
            [0.0, 0.0, 0.0],
            [0.5, 0.3, 0.8],
            [1.0, 1.0, 1.0],
            [0.2, 0.7, 0.4],
        ];
        let batch = lut.apply_batch(&pixels);
        for (i, &p) in pixels.iter().enumerate() {
            let single = lut.apply(p);
            assert!(
                (batch[i][0] - single[0]).abs() < 1e-6,
                "batch[{i}] R mismatch"
            );
            assert!(
                (batch[i][1] - single[1]).abs() < 1e-6,
                "batch[{i}] G mismatch"
            );
            assert!(
                (batch[i][2] - single[2]).abs() < 1e-6,
                "batch[{i}] B mismatch"
            );
        }
    }

    #[test]
    fn test_batch_par() {
        let lut = Lut3D::identity(33);
        // Generate enough pixels to trigger rayon path (>1024)
        let pixels: Vec<[f32; 3]> = (0..2000)
            .map(|i| {
                let t = i as f32 / 2000.0;
                [t, 1.0 - t, t * 0.5]
            })
            .collect();
        let seq = lut.apply_batch(&pixels);
        let par = lut.apply_batch_par(&pixels);
        assert_eq!(seq.len(), par.len());
        for (i, (s, p)) in seq.iter().zip(par.iter()).enumerate() {
            assert!((s[0] - p[0]).abs() < 1e-6, "par[{i}] R mismatch");
        }
    }

    #[test]
    fn test_inplace() {
        let lut = Lut3D::identity(33);
        let mut buf = vec![0.5f32, 0.3, 0.8, 0.2, 0.7, 0.4];
        let expected_0 = lut.apply([0.5, 0.3, 0.8]);
        let expected_1 = lut.apply([0.2, 0.7, 0.4]);
        lut.apply_inplace(&mut buf);
        assert!((buf[0] - expected_0[0]).abs() < 1e-6);
        assert!((buf[3] - expected_1[0]).abs() < 1e-6);
    }

    #[test]
    fn test_trilinear_vs_tetrahedral_midpoint() {
        // Both methods should agree on identity LUT midpoint
        let tri = Lut3D::identity(33);
        let tet = Lut3D::identity(33).with_interpolation(Interpolation::Tetrahedral);
        let input = [0.5, 0.5, 0.5];
        let rt = tri.apply(input);
        let re = tet.apply(input);
        for i in 0..3 {
            assert!((rt[i] - re[i]).abs() < 0.05, "tri/tet channel {i} differ");
        }
    }
}
