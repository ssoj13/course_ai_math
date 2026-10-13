//! Transform processor for applying color conversions.
//!
//! The processor compiles a chain of transforms into an optimized
//! pipeline for efficient pixel processing.
//!
//! # Example
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use vfx_ocio::{Config, Processor};
//!
//! let config = Config::from_file("config.ocio")?;
//! let processor = config.processor("ACEScg", "sRGB")?;
//!
//! // Process pixels
//! let mut pixels = [[0.18_f32, 0.18, 0.18]; 100];
//! processor.apply_rgb(&mut pixels);
//! # Ok(())
//! # }
//! ```

use half::f16;
use vfx_core::pixel::{REC709_LUMA_B, REC709_LUMA_G, REC709_LUMA_R};

// ============================================================================
// Bit Depth (OCIO parity)
// ============================================================================

/// Pixel bit depth for packed/planar CPU processing.
/// Used for stride-aware apply methods. Matches OCIO BitDepth enum.
/// OCIO ref: OpenColorTypes.h BitDepth enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum PackedBitDepth {
    /// Unknown bit depth.
    Unknown = 0,
    /// 8-bit unsigned integer [0, 255].
    Uint8 = 1,
    /// 10-bit unsigned integer [0, 1023].
    Uint10 = 2,
    /// 12-bit unsigned integer [0, 4095].
    Uint12 = 3,
    /// 16-bit unsigned integer [0, 65535].
    Uint16 = 4,
    /// 16-bit floating point (half).
    F16 = 5,
    /// 32-bit floating point.
    #[default]
    F32 = 6,
}

impl PackedBitDepth {
    /// Returns the size in bytes per channel.
    pub fn bytes_per_channel(self) -> usize {
        match self {
            Self::Unknown => 0,
            Self::Uint8 => 1,
            Self::Uint10 | Self::Uint12 | Self::Uint16 | Self::F16 => 2,
            Self::F32 => 4,
        }
    }

    /// Returns the normalization factor for integer types.
    pub fn max_value(self) -> f32 {
        match self {
            Self::Unknown => 1.0,
            Self::Uint8 => 255.0,
            Self::Uint10 => 1023.0,
            Self::Uint12 => 4095.0,
            Self::Uint16 => 65535.0,
            Self::F16 | Self::F32 => 1.0,
        }
    }
}

// ============================================================================
// Channel Ordering (OCIO parity)
// ============================================================================

/// Channel ordering for packed pixel formats.
/// Matches OCIO ChannelOrdering enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum ChannelOrdering {
    /// RGB (3 channels).
    Rgb = 0,
    /// RGBA (4 channels).
    #[default]
    Rgba = 1,
    /// BGR (3 channels, reversed).
    Bgr = 2,
    /// BGRA (4 channels, reversed RGB).
    Bgra = 3,
    /// ABGR (4 channels, alpha first, reversed RGB).
    Abgr = 4,
}

impl ChannelOrdering {
    /// Returns the number of channels.
    pub fn num_channels(self) -> usize {
        match self {
            Self::Rgb | Self::Bgr => 3,
            Self::Rgba | Self::Bgra | Self::Abgr => 4,
        }
    }

    /// Returns true if alpha channel is present.
    pub fn has_alpha(self) -> bool {
        matches!(self, Self::Rgba | Self::Bgra | Self::Abgr)
    }

    /// Returns the index of the R channel.
    pub fn r_index(self) -> usize {
        match self {
            Self::Rgb | Self::Rgba => 0,
            Self::Bgr | Self::Bgra => 2,
            Self::Abgr => 3,
        }
    }

    /// Returns the index of the G channel.
    pub fn g_index(self) -> usize {
        match self {
            Self::Rgb | Self::Rgba | Self::Bgr | Self::Bgra => 1,
            Self::Abgr => 2,
        }
    }

    /// Returns the index of the B channel.
    pub fn b_index(self) -> usize {
        match self {
            Self::Rgb | Self::Rgba => 2,
            Self::Bgr | Self::Bgra => 0,
            Self::Abgr => 1,
        }
    }

    /// Returns the index of the A channel, if present.
    pub fn a_index(self) -> Option<usize> {
        match self {
            Self::Rgb | Self::Bgr => None,
            Self::Rgba | Self::Bgra => Some(3),
            Self::Abgr => Some(0),
        }
    }
}

use crate::error::{OcioError, OcioResult};
use crate::grading_bspline::{GradingControlPoint, build_hue_uniforms, build_rgb_uniforms};
use crate::io_proxy::ConfigIOProxy;
use crate::transform::*;
use std::sync::Arc;

/// Read-only / introspection methods on `Processor` (STRUCT.2.next split).
mod accessors;
/// Per-pixel application (`apply_rgb` / `apply_packed` / `apply_one_*`)
/// methods on `Processor` (STRUCT.2.next split).
mod apply;
/// `Transform` AST → `ProcessorOp` compilation methods (STRUCT.2.next split).
mod compile;
/// Numerical helpers (matrix inversion, transfer curves, LUT inversion/indexing).
mod ff_color;
/// Exact per-pixel LUT inverses (OCIO `InvLut1DRenderer` / `InvLut3DRenderer`).
pub mod inv_lut;
mod math;
pub(crate) use math::half_domain_edges;
use math::{apply_transfer, invert_3x3};

/// Optimization level for processors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OptimizationLevel {
    /// No optimization.
    None,
    /// OCIO LOSSLESS: identities, inverse pairs, SIMPLIFY_OPS, matrix/Range/gamma composition.
    #[default]
    Lossless,
    /// OCIO GOOD: LOSSLESS plus 1D and 3D LUT composition.
    Good,
    /// OCIO VERY_GOOD (its default): LOSSLESS plus 1D LUT composition, without its
    /// `LUT_INV_FAST` (which swaps an inverse LUT for a baked approximation): inverses stay exact.
    Best,
    /// OCIO DRAFT: as `Good` here (the remaining DRAFT flags are not ported).
    Draft,
}

/// Bit-flag form of OCIO's optimization knobs.
///
/// Mirrors the C++ `enum OptimizationFlags` declared in
/// `OpenColorTypes.h:637-723`. Bit values are kept identical to the
/// C++ definition so any host that ANDs/ORs them together gets the
/// same combinations across language boundaries. Aggregate presets
/// (`LOSSLESS`, `GOOD`, `DEFAULT`, …) match C++.
///
/// `Processor::optimize` is driven by [`OptimizationLevel`], not by these flags; they are
/// accepted at the API boundary for hosts that pass OCIO flag values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OptimizationFlags(pub u32);

impl OptimizationFlags {
    /// No optimisation passes — keep the op chain as-built.
    pub const NONE: Self = Self(0x00000000);
    /// Drop identity matrix / range / LUT ops.
    pub const IDENTITY: Self = Self(0x00000001);
    /// Drop identity gamma ops.
    pub const IDENTITY_GAMMA: Self = Self(0x00000002);
    /// Collapse forward-then-inverse CDL pairs.
    pub const PAIR_IDENTITY_CDL: Self = Self(0x00000040);
    /// Collapse forward-then-inverse exposure-contrast pairs.
    pub const PAIR_IDENTITY_EXPOSURE_CONTRAST: Self = Self(0x00000080);
    /// Collapse forward-then-inverse fixed-function pairs.
    pub const PAIR_IDENTITY_FIXED_FUNCTION: Self = Self(0x00000100);
    /// Collapse forward-then-inverse gamma pairs.
    pub const PAIR_IDENTITY_GAMMA: Self = Self(0x00000200);
    /// Collapse forward-then-inverse Lut1D pairs.
    pub const PAIR_IDENTITY_LUT1D: Self = Self(0x00000400);
    /// Collapse forward-then-inverse Lut3D pairs.
    pub const PAIR_IDENTITY_LUT3D: Self = Self(0x00000800);
    /// Collapse forward-then-inverse log pairs.
    pub const PAIR_IDENTITY_LOG: Self = Self(0x00001000);
    /// Collapse forward-then-inverse grading pairs.
    pub const PAIR_IDENTITY_GRADING: Self = Self(0x00002000);
    /// Fold adjacent exponent ops.
    pub const COMP_EXPONENT: Self = Self(0x00040000);
    /// Fold adjacent gamma ops.
    pub const COMP_GAMMA: Self = Self(0x00080000);
    /// Fold adjacent matrix ops.
    pub const COMP_MATRIX: Self = Self(0x00100000);
    /// Fold adjacent Lut1D ops.
    pub const COMP_LUT1D: Self = Self(0x00200000);
    /// Fold adjacent Lut3D ops.
    pub const COMP_LUT3D: Self = Self(0x00400000);
    /// Fold adjacent range ops.
    pub const COMP_RANGE: Self = Self(0x00800000);
    /// Coalesce the separable prefix of a chain into a single op.
    pub const COMP_SEPARABLE_PREFIX: Self = Self(0x01000000);
    /// Use the fast inverse-LUT approximation.
    pub const LUT_INV_FAST: Self = Self(0x02000000);
    /// Use the fast log/exp/pow approximations (lossy).
    pub const FAST_LOG_EXP_POW: Self = Self(0x04000000);
    /// Collapse trivially-identical adjacent ops.
    pub const SIMPLIFY_OPS: Self = Self(0x08000000);
    /// Discard dynamic-property hooks (CPU-only optimisation).
    pub const NO_DYNAMIC_PROPERTIES: Self = Self(0x10000000);
    /// Enable every flag — most aggressive optimisation.
    pub const ALL: Self = Self(0xFFFFFFFF);

    /// `OPTIMIZATION_LOSSLESS` — identity pruning + identity-pair
    /// folding for every op type, no precision-affecting passes.
    pub const LOSSLESS: Self = Self(
        Self::IDENTITY.0
            | Self::IDENTITY_GAMMA.0
            | Self::PAIR_IDENTITY_CDL.0
            | Self::PAIR_IDENTITY_EXPOSURE_CONTRAST.0
            | Self::PAIR_IDENTITY_FIXED_FUNCTION.0
            | Self::PAIR_IDENTITY_GAMMA.0
            | Self::PAIR_IDENTITY_LUT1D.0
            | Self::PAIR_IDENTITY_LUT3D.0
            | Self::PAIR_IDENTITY_LOG.0
            | Self::PAIR_IDENTITY_GRADING.0,
    );

    /// `OPTIMIZATION_VERY_GOOD` — LOSSLESS plus matrix/range/LUT
    /// composition. Default for `getProcessor` in OCIO C++.
    pub const VERY_GOOD: Self = Self(
        Self::LOSSLESS.0
            | Self::COMP_EXPONENT.0
            | Self::COMP_GAMMA.0
            | Self::COMP_MATRIX.0
            | Self::COMP_LUT1D.0
            | Self::COMP_RANGE.0
            | Self::COMP_SEPARABLE_PREFIX.0
            | Self::LUT_INV_FAST.0
            | Self::SIMPLIFY_OPS.0,
    );

    /// `OPTIMIZATION_GOOD` — alias of `VERY_GOOD` in OCIO C++.
    pub const GOOD: Self = Self::VERY_GOOD;

    /// `OPTIMIZATION_DRAFT` — every fast-path flag, including the
    /// precision-trading `FAST_LOG_EXP_POW`.
    pub const DRAFT: Self = Self(Self::VERY_GOOD.0 | Self::COMP_LUT3D.0 | Self::FAST_LOG_EXP_POW.0);

    /// `OPTIMIZATION_DEFAULT` — `VERY_GOOD`, matching OCIO's
    /// default `getProcessor` setting.
    pub const DEFAULT: Self = Self::VERY_GOOD;

    /// Tests whether all bits in `other` are set.
    #[inline]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Union with another set of flags.
    #[inline]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Intersection with another set of flags.
    #[inline]
    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// Raw `u32` flag value.
    #[inline]
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// Construct from a raw `u32`.
    #[inline]
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }
}

impl Default for OptimizationFlags {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl OptimizationLevel {
    /// Maps the coarse level to the matching OCIO bit-flag preset.
    pub fn to_flags(self) -> OptimizationFlags {
        match self {
            OptimizationLevel::None => OptimizationFlags::NONE,
            OptimizationLevel::Lossless => OptimizationFlags::LOSSLESS,
            OptimizationLevel::Good => OptimizationFlags::GOOD,
            OptimizationLevel::Best => OptimizationFlags::ALL,
            OptimizationLevel::Draft => OptimizationFlags::DRAFT,
        }
    }
}

/// Reads `OCIO_OPTIMIZATION_FLAGS` env override. The env var is
/// parsed as either a decimal or `0x`-prefixed hex `u32`. Returns
/// `None` when unset or unparseable, matching OCIO C++ behaviour at
/// `OpenColorTypes.h:827-832`.
pub fn optimization_flags_env_override() -> Option<OptimizationFlags> {
    let v = std::env::var("OCIO_OPTIMIZATION_FLAGS").ok()?;
    let trimmed = v.trim();
    let parsed = if let Some(rest) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        u32::from_str_radix(rest, 16).ok()?
    } else {
        trimmed.parse::<u32>().ok()?
    };
    Some(OptimizationFlags::from_bits(parsed))
}

/// Compiled transform processor.
///
/// The processor holds an optimized transform chain ready for pixel application.
///
/// `Clone` copies the op list, so a host can keep one processor for the GPU and clone it to
/// move a dynamic property on a CPU twin (`Processor::set_dynamic`) without locks.
#[derive(Debug, Clone)]
pub struct Processor {
    /// Compiled operation list.
    ops: Vec<ProcessorOp>,
    /// Input bit depth hint.
    input_bit_depth: PackedBitDepth,
    /// Output bit depth hint.
    output_bit_depth: PackedBitDepth,
    /// Context for variable resolution.
    #[allow(dead_code)]
    context: Option<crate::Context>,
    /// Config IO proxy for FileTransform resolution.
    #[allow(dead_code)]
    io_proxy: Option<Arc<dyn ConfigIOProxy>>,
    /// External files this processor was built from, in compile order.
    ///
    /// Populated by the `FileTransform` arm of `compile_transform`, which is the only point where
    /// a file path is still known — by the time a LUT reaches [`ProcessorOp::Lut1d`]/`Lut3d` it is
    /// baked sample data with no provenance. The reference reaches the same result differently:
    /// it inserts a marker op (`CreateFileNoOp`, `FileTransform.cpp:945`) whose `dumpMetadata`
    /// reports the path (`NoOps.cpp:355`). Collecting at the same point avoids adding a no-op
    /// variant to `ProcessorOp` purely to carry a string.
    ///
    /// Stores the RESOLVED path, matching the reference — `FileTransform.cpp:945` passes the same
    /// `filepath` it handed to `GetCachedFileAndFormat`, i.e. after context/search-path
    /// resolution. When an IO proxy supplies the bytes, the temporary file written to satisfy the
    /// parser is deliberately NOT recorded; the caller asked about the logical file.
    files: Vec<String>,
}

// Note: vfx_core::BitDepth is for I/O, we use our own BitDepth for CPU processing

pub mod op;
pub use op::*;

/// Built-in transfer function styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferStyle {
    /// sRGB OETF.
    Srgb,
    /// Rec.709 OETF.
    Rec709,
    /// Rec.2020 OETF.
    Rec2020,
    /// Gamma 2.2.
    Gamma22,
    /// Gamma 2.4.
    Gamma24,
    /// Gamma 2.6 (DCI).
    Gamma26,
    /// Rec.1886 (gamma 2.4 for broadcast).
    Rec1886,
    /// Linear (passthrough).
    Linear,
    /// PQ (SMPTE ST 2084).
    Pq,
    /// HLG (ARIB STD-B67).
    Hlg,
    /// ACEScct.
    AcesCct,
    /// ACEScc.
    AcesCc,
    /// Log3G10.
    Log3G10,
    /// LogC3 (ARRI).
    LogC3,
    /// LogC4 (ARRI).
    LogC4,
    /// S-Log3 (Sony).
    SLog3,
    /// V-Log (Panasonic).
    VLog,
    /// Log-C (Blackmagic).
    BmdFilmGen5,
    /// Apple Log.
    AppleLog,
    /// Canon C-Log2.
    CanonCLog2,
    /// Canon C-Log3.
    CanonCLog3,
}

impl Processor {
    /// Creates a new empty processor.
    pub(crate) fn new() -> Self {
        Self {
            ops: Vec::new(),
            input_bit_depth: PackedBitDepth::Unknown,
            output_bit_depth: PackedBitDepth::Unknown,
            context: None,
            io_proxy: None,
            files: Vec::new(),
        }
    }

    /// Creates a processor from pre-compiled ops (for caching).
    pub fn from_ops(ops: Vec<ProcessorOp>) -> Self {
        Self {
            ops,
            input_bit_depth: PackedBitDepth::Unknown,
            output_bit_depth: PackedBitDepth::Unknown,
            context: None,
            io_proxy: None,
            files: Vec::new(),
        }
    }

    /// Sets the context for variable resolution.
    pub fn set_context(&mut self, context: crate::Context) {
        self.context = Some(context);
    }

    /// Sets the config IO proxy.
    pub fn set_config_io_proxy(&mut self, proxy: Option<Arc<dyn ConfigIOProxy>>) {
        self.io_proxy = proxy;
    }

    /// Returns the context (if set).
    pub fn context(&self) -> Option<&crate::Context> {
        self.context.as_ref()
    }

    /// Returns the config IO proxy (if set).
    pub fn config_io_proxy(&self) -> Option<Arc<dyn ConfigIOProxy>> {
        self.io_proxy.clone()
    }

    /// Returns the compiled operations (for caching).
    pub fn ops(&self) -> &[ProcessorOp] {
        &self.ops
    }

    /// Creates a processor from a transform.
    pub fn from_transform(
        transform: &Transform,
        direction: TransformDirection,
    ) -> OcioResult<Self> {
        Self::from_transform_with_opts(transform, direction, OptimizationLevel::default())
    }

    /// Creates a processor from a transform with optimization level.
    pub fn from_transform_with_opts(
        transform: &Transform,
        direction: TransformDirection,
        optimization: OptimizationLevel,
    ) -> OcioResult<Self> {
        Self::from_transform_with_context_and_proxy(transform, direction, optimization, None, None)
    }

    /// Creates a processor with context for variable resolution.
    ///
    /// Context variables are used to resolve `$VAR` references in FileTransform paths.
    pub fn from_transform_with_context(
        transform: &Transform,
        direction: TransformDirection,
        optimization: OptimizationLevel,
        context: &crate::Context,
    ) -> OcioResult<Self> {
        Self::from_transform_with_context_and_proxy(
            transform,
            direction,
            optimization,
            Some(context.clone()),
            None,
        )
    }

    /// Creates a processor with optional context and config IO proxy.
    pub fn from_transform_with_context_and_proxy(
        transform: &Transform,
        direction: TransformDirection,
        optimization: OptimizationLevel,
        context: Option<crate::Context>,
        io_proxy: Option<Arc<dyn ConfigIOProxy>>,
    ) -> OcioResult<Self> {
        let mut processor = Self::new();
        processor.context = context;
        processor.io_proxy = io_proxy;
        processor.compile_transform(transform, direction)?;
        processor.optimize(optimization);
        Ok(processor)
    }

    /// Combines two processors into one by concatenating their ops.
    ///
    /// This is used for cross-config workflows where a source processor
    /// converts to interchange space, and a destination processor converts
    /// from interchange to the final color space.
    pub fn combine(first: &Processor, second: &Processor) -> OcioResult<Self> {
        let mut combined = Self::new();
        combined.ops.extend(first.ops.iter().cloned());
        combined.ops.extend(second.ops.iter().cloned());
        // Carry file provenance across the join in the same order as the ops. Without this a
        // combined processor reports "no external files" even when both halves were built from
        // LUT files — the same gap `metadata()` itself used to have.
        combined.files.extend(first.files.iter().cloned());
        combined.files.extend(second.files.iter().cloned());
        combined.optimize(OptimizationLevel::default());
        Ok(combined)
    }

    /// Creates an inverse processor that reverses this processor's operations.
    ///
    /// Note: Not all operations are invertible. Returns error if any op
    /// cannot be inverted.
    pub fn inverse(&self) -> OcioResult<Self> {
        let mut inverted_ops = Vec::with_capacity(self.ops.len());

        // Reverse order and invert each op
        for op in self.ops.iter().rev() {
            inverted_ops.extend(op.inverse()?);
        }

        let mut result = Self::new();
        result.ops = inverted_ops;
        result.optimize(OptimizationLevel::default());
        Ok(result)
    }

    /// OCIO `OpRcPtrVec::optimize` (`OpOptimizers.cpp`). Every level but `None` runs OCIO's
    /// LOSSLESS flags; `Best` (OCIO VERY_GOOD) adds COMP_LUT1D, `Good` (OCIO GOOD) and `Draft`
    /// also COMP_LUT3D. OCIO's LUT_INV_FAST, FAST_LOG_EXP_POW and separable-prefix flags are not
    /// ported (the CPU inverts LUTs exactly and evaluates at full precision; an inverse LUT is
    /// not composed). Each pass, as OCIO's, in order:
    /// `RemoveNoOps` (ops whose [`ProcessorOp::is_identity`] holds), `ReplaceOps`
    /// (SIMPLIFY_OPS, [`ProcessorOp::simpler_replacement`]), `ReplaceIdentityOps`
    /// ([`ProcessorOp::identity_replacement`]), `RemoveInverseOps`
    /// ([`ProcessorOp::pair_identity_with`]) and `CombineOps` (at most one pair per pass,
    /// [`ProcessorOp::combine_with`]); passes repeat until the removals, replacements, pairs and
    /// combines all come up empty, 80 at most.
    pub fn optimize(&mut self, level: OptimizationLevel) {
        if level == OptimizationLevel::None {
            return;
        }
        for _ in 0..=80 {
            let before = self.ops.len();
            self.ops.retain(|op| !op.is_identity());
            let noops = before - self.ops.len();
            self.replace_ops();
            let identities = self.replace_identity_ops();
            let pairs = self.remove_inverse_pairs();
            let combines = self.combine_ops(level);
            if noops + identities + pairs + combines == 0 {
                break;
            }
        }
    }

    /// OCIO `ReplaceOps`: each op with a simpler replacement is swapped for it in place.
    fn replace_ops(&mut self) -> usize {
        let mut count = 0;
        let mut i = 0;
        while i < self.ops.len() {
            if let Some(ops) = self.ops[i].simpler_replacement() {
                let n = ops.len();
                self.ops.splice(i..i + 1, ops);
                i += n;
                count += 1;
            } else {
                i += 1;
            }
        }
        count
    }

    /// OCIO `ReplaceIdentityOps`: an identity that still clamps becomes that clamp.
    fn replace_identity_ops(&mut self) -> usize {
        let mut count = 0;
        for op in &mut self.ops {
            if let Some(r) = op.identity_replacement() {
                *op = r;
                count += 1;
            }
        }
        count
    }

    /// OCIO `RemoveInverseOps`: `..., A, B, B', A', ...` collapses from the inside out. An exact
    /// pair is removed and the pass steps back to reconsider `A, A'`; a pair whose round trip
    /// clamps is replaced by that Range and the pass moves on (OCIO steps back only when the
    /// replacement is a no-op, and a Range never is). Returns the number of pairs handled.
    fn remove_inverse_pairs(&mut self) -> usize {
        let mut count = 0;
        let mut i = 0;
        while i + 1 < self.ops.len() {
            match self.ops[i].pair_identity_with(&self.ops[i + 1]) {
                None => i += 1,
                Some(None) => {
                    self.ops.drain(i..i + 2);
                    i = i.saturating_sub(1);
                    count += 1;
                }
                Some(Some(op)) => {
                    self.ops.splice(i..i + 2, [op]);
                    i += 1;
                    count += 1;
                }
            }
        }
        count
    }

    /// OCIO `CombineOps`: the first adjacent pair that composes is replaced by its composition,
    /// then the pass ends (OCIO prefers another round of pair removal to more composition).
    fn combine_ops(&mut self, level: OptimizationLevel) -> usize {
        use OptimizationLevel as L;
        let lut1d = matches!(level, L::Best | L::Good | L::Draft);
        let lut3d = matches!(level, L::Good | L::Draft);
        for i in 0..self.ops.len().saturating_sub(1) {
            let (a, b) = (&self.ops[i], &self.ops[i + 1]);
            let combined = a
                .combine_with(b)
                .or_else(|| if lut1d { a.compose_lut1d_with(b) } else { None })
                .or_else(|| if lut3d { a.compose_lut3d_with(b) } else { None });
            if let Some(op) = combined {
                self.ops.splice(i..i + 2, [op]);
                return 1;
            }
        }
        0
    }
}

/// Processor metadata containing information about the processing pipeline.
/// OCIO ref: ProcessorMetadata
#[derive(Debug, Clone, Default)]
pub struct ProcessorMetadata {
    /// Number of external files used.
    pub num_files: usize,
    /// List of external file paths.
    pub files: Vec<String>,
}

/// Format metadata for transform serialization.
/// OCIO ref: FormatMetadata
#[derive(Debug, Clone, Default)]
pub struct FormatMetadata {
    /// Unique identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Key-value attributes.
    pub attributes: Vec<(String, String)>,
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_optimizer;
