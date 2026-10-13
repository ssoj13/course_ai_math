//! Unified pixel and bit depth formats.
//!
//! This module provides the canonical definitions for pixel data formats
//! used across all vfx-rs crates, compatible with OpenImageIO's TypeDesc.
//!
//! # Types
//!
//! - [`BitDepth`] - Bit depth specification (includes packed formats like 10/12-bit)
//! - [`DataFormat`] - Runtime pixel data type (U8, U16, U32, F16, F32)
//! - [`TypeDesc`] - Full type descriptor with base type, aggregate, and semantics
//! - [`BaseType`] - Fundamental data type (matches OIIO BASETYPE)
//! - [`Aggregate`] - Aggregation of base types (SCALAR, VEC2, VEC3, etc.)
//! - [`VecSemantics`] - Semantic meaning of vector types
//!
//! # Usage
//!
//! ```rust
//! use vfx_core::format::{BitDepth, DataFormat, TypeDesc, BaseType, Aggregate};
//!
//! // DPX uses 10-bit packed
//! let dpx_depth = BitDepth::U10;
//! let storage = dpx_depth.storage_format(); // U16 (smallest that fits)
//!
//! // EXR uses half-float
//! let exr_format = DataFormat::F16;
//! let bit_depth = BitDepth::from(exr_format); // F16
//!
//! // Full type descriptor for a color value
//! let color_type = TypeDesc::new(BaseType::Float, Aggregate::Vec3);
//! assert_eq!(color_type.size(), 12); // 3 * 4 bytes
//! ```

/// Bit depth specification for image data.
///
/// Represents the precision of pixel values, including packed formats
/// commonly used in film/broadcast (10-bit, 12-bit DPX).
///
/// # Variants
///
/// Integer formats:
/// - `U8` - 8-bit unsigned [0, 255]
/// - `U10` - 10-bit unsigned [0, 1023] (DPX, broadcast)
/// - `U12` - 12-bit unsigned [0, 4095] (cinema cameras)
/// - `U16` - 16-bit unsigned [0, 65535]
/// - `U32` - 32-bit unsigned [0, 4294967295]
///
/// Floating-point formats:
/// - `F16` - 16-bit half-precision IEEE 754
/// - `F32` - 32-bit single-precision IEEE 754
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BitDepth {
    /// Unknown/auto-detect.
    Unknown,
    /// 8-bit unsigned integer.
    U8,
    /// 10-bit unsigned integer (DPX, broadcast).
    U10,
    /// 12-bit unsigned integer (cinema cameras).
    U12,
    /// 16-bit unsigned integer.
    U16,
    /// 32-bit unsigned integer.
    U32,
    /// 16-bit half-precision float.
    F16,
    /// 32-bit single-precision float (VFX standard).
    #[default]
    F32,
}

impl BitDepth {
    /// Number of bits per channel.
    /// Returns 0 for Unknown.
    #[inline]
    pub const fn bits(&self) -> u32 {
        match self {
            Self::Unknown => 0,
            Self::U8 => 8,
            Self::U10 => 10,
            Self::U12 => 12,
            Self::U16 => 16,
            Self::U32 => 32,
            Self::F16 => 16,
            Self::F32 => 32,
        }
    }

    /// Whether this is a floating-point format.
    /// Returns false for Unknown.
    #[inline]
    pub const fn is_float(&self) -> bool {
        matches!(self, Self::F16 | Self::F32)
    }

    /// Whether this is unknown/auto-detect.
    #[inline]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }

    /// Whether this is an integer format.
    /// Returns false for Unknown.
    #[inline]
    pub const fn is_integer(&self) -> bool {
        matches!(
            self,
            Self::U8 | Self::U10 | Self::U12 | Self::U16 | Self::U32
        )
    }

    /// Whether this is a packed format (not byte-aligned).
    #[inline]
    pub const fn is_packed(&self) -> bool {
        matches!(self, Self::U10 | Self::U12)
    }

    /// Maximum representable integer value.
    /// Returns 0 for Unknown.
    #[inline]
    pub const fn max_value(&self) -> u32 {
        match self {
            Self::Unknown => 0,
            Self::U8 => 255,
            Self::U10 => 1023,
            Self::U12 => 4095,
            Self::U16 => 65535,
            Self::U32 => u32::MAX,
            Self::F16 | Self::F32 => u32::MAX, // Not really applicable
        }
    }

    /// Returns the smallest [`DataFormat`] that can store this bit depth.
    ///
    /// Packed formats (10-bit, 12-bit) require U16 storage.
    /// Unknown defaults to F32.
    #[inline]
    pub const fn storage_format(&self) -> DataFormat {
        match self {
            Self::Unknown => DataFormat::F32,
            Self::U8 => DataFormat::U8,
            Self::U10 | Self::U12 | Self::U16 => DataFormat::U16,
            Self::U32 => DataFormat::U32,
            Self::F16 => DataFormat::F16,
            Self::F32 => DataFormat::F32,
        }
    }

    /// Bytes needed per channel in storage format.
    #[inline]
    pub const fn bytes_per_channel(&self) -> usize {
        self.storage_format().bytes_per_channel()
    }

    /// Normalization factor for converting to [0, 1] float range.
    /// Returns 1.0 for Unknown.
    #[inline]
    pub fn normalize_factor(&self) -> f32 {
        match self {
            Self::Unknown => 1.0,
            Self::U8 => 255.0,
            Self::U10 => 1023.0,
            Self::U12 => 4095.0,
            Self::U16 => 65535.0,
            Self::U32 => 4294967295.0,
            Self::F16 | Self::F32 => 1.0,
        }
    }

    /// Alias for `normalize_factor()` (CLF compatibility).
    #[inline]
    pub fn scale(&self) -> f32 {
        self.normalize_factor()
    }

    /// Parse from CLF bit depth string ("8i", "10i", "12i", "16i", "16f", "32f").
    ///
    /// # Example
    /// ```rust
    /// use vfx_core::BitDepth;
    /// assert_eq!(BitDepth::from_clf_str("10i"), Some(BitDepth::U10));
    /// assert_eq!(BitDepth::from_clf_str("32f"), Some(BitDepth::F32));
    /// ```
    pub fn from_clf_str(s: &str) -> Option<Self> {
        match s {
            "8i" => Some(Self::U8),
            "10i" => Some(Self::U10),
            "12i" => Some(Self::U12),
            "16i" => Some(Self::U16),
            "16f" => Some(Self::F16),
            "32f" => Some(Self::F32),
            _ => None,
        }
    }

    /// Returns CLF bit depth string.
    ///
    /// # Example
    /// ```rust
    /// use vfx_core::BitDepth;
    /// assert_eq!(BitDepth::U10.clf_str(), "10i");
    /// assert_eq!(BitDepth::F32.clf_str(), "32f");
    /// ```
    pub fn clf_str(&self) -> &'static str {
        match self {
            Self::Unknown => "32f", // Default to float
            Self::U8 => "8i",
            Self::U10 => "10i",
            Self::U12 => "12i",
            Self::U16 => "16i",
            Self::U32 => "32f", // CLF has no 32i; 32f is the closest representation
            Self::F16 => "16f",
            Self::F32 => "32f",
        }
    }
}

impl std::fmt::Display for BitDepth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown => write!(f, "unknown"),
            Self::U8 => write!(f, "8-bit"),
            Self::U10 => write!(f, "10-bit"),
            Self::U12 => write!(f, "12-bit"),
            Self::U16 => write!(f, "16-bit"),
            Self::U32 => write!(f, "32-bit"),
            Self::F16 => write!(f, "half"),
            Self::F32 => write!(f, "float"),
        }
    }
}

/// Runtime pixel data format.
///
/// Represents the actual storage type of pixel data in memory.
/// Unlike [`BitDepth`], this only includes byte-aligned types.
///
/// # Relationship with BitDepth
///
/// - `BitDepth::U10` and `BitDepth::U12` both use `DataFormat::U16` for storage
/// - Use [`BitDepth::storage_format()`] to get the appropriate storage format
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DataFormat {
    /// 8-bit unsigned integer.
    U8,
    /// 16-bit unsigned integer.
    #[default]
    U16,
    /// 32-bit unsigned integer.
    U32,
    /// 8-bit signed integer.
    I8,
    /// 16-bit signed integer.
    I16,
    /// 32-bit signed integer.
    I32,
    /// 16-bit half-precision float.
    F16,
    /// 32-bit single-precision float.
    F32,
    /// 64-bit double-precision float.
    F64,
}
impl DataFormat {
    /// Number of bytes per channel.
    #[inline]
    pub const fn bytes_per_channel(&self) -> usize {
        match self {
            Self::U8 => 1,
            Self::U16 => 2,
            Self::U32 => 4,
            Self::I8 => 1,
            Self::I16 => 2,
            Self::I32 => 4,
            Self::F16 => 2,
            Self::F32 => 4,
            Self::F64 => 8,
        }
    }

    /// Number of bits per channel.
    #[inline]
    pub const fn bits(&self) -> u32 {
        match self {
            Self::U8 => 8,
            Self::U16 => 16,
            Self::U32 => 32,
            Self::I8 => 8,
            Self::I16 => 16,
            Self::I32 => 32,
            Self::F16 => 16,
            Self::F32 => 32,
            Self::F64 => 64,
        }
    }

    /// Whether this is a floating-point format.
    #[inline]
    pub const fn is_float(&self) -> bool {
        matches!(self, Self::F16 | Self::F32 | Self::F64)
    }

    /// Whether this is an integer format.
    #[inline]
    pub const fn is_integer(&self) -> bool {
        !self.is_float()
    }

    /// Whether this is a signed type (signed integers or floats).
    #[inline]
    pub const fn is_signed(&self) -> bool {
        matches!(
            self,
            Self::I8 | Self::I16 | Self::I32 | Self::F16 | Self::F32 | Self::F64
        )
    }

    /// Normalization factor for converting to float range.
    ///
    /// For unsigned integers: maps [0, MAX] to [0.0, 1.0].
    /// For signed integers: maps [-MAX, MAX] to [-1.0, 1.0].
    /// For floats: returns 1.0 (values are already normalized).
    ///
    /// # Precision note
    /// Returns f32. For U32/I32, use [`normalize_factor_f64`] to avoid
    /// precision loss (u32::MAX = 4294967295 is not representable in f32).
    #[inline]
    pub fn normalize_factor(&self) -> f32 {
        self.normalize_factor_f64() as f32
    }

    /// Normalization factor as f64 for full precision.
    ///
    /// U32 max (4294967295) and I32 max (2147483647) cannot be represented
    /// exactly in f32, so this method provides the accurate f64 value.
    #[inline]
    pub fn normalize_factor_f64(&self) -> f64 {
        match self {
            Self::U8 => 255.0,
            Self::U16 => 65535.0,
            Self::U32 => u32::MAX as f64, // 4294967295.0 - exact in f64
            Self::I8 => 127.0,
            Self::I16 => 32767.0,
            Self::I32 => i32::MAX as f64, // 2147483647.0 - exact in f64
            Self::F16 | Self::F32 | Self::F64 => 1.0,
        }
    }

    /// Short name for display.
    pub const fn name(&self) -> &'static str {
        match self {
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::F16 => "f16",
            Self::F32 => "f32",
            Self::F64 => "f64",
        }
    }
}

impl std::fmt::Display for DataFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

// Conversions between BitDepth and DataFormat

impl From<DataFormat> for BitDepth {
    fn from(fmt: DataFormat) -> Self {
        match fmt {
            DataFormat::U8 => BitDepth::U8,
            DataFormat::U16 => BitDepth::U16,
            DataFormat::U32 => BitDepth::U32,
            DataFormat::F16 => BitDepth::F16,
            DataFormat::F32 => BitDepth::F32,
            // Signed integers map to nearest unsigned bit depth
            DataFormat::I8 => BitDepth::U8,
            DataFormat::I16 => BitDepth::U16,
            DataFormat::I32 => BitDepth::U32,
            DataFormat::F64 => BitDepth::F32, // No F64 in BitDepth; downgrade
        }
    }
}

impl From<BitDepth> for DataFormat {
    /// Converts to storage format. Packed formats become U16.
    fn from(depth: BitDepth) -> Self {
        depth.storage_format()
    }
}

// =============================================================================
// OIIO-Compatible TypeDesc System
// =============================================================================

/// Base data type for TypeDesc (matches OIIO BASETYPE).
///
/// Represents the fundamental storage type for each element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum BaseType {
    /// Unknown or unspecified type
    Unknown = 0,
    /// No type (void)
    None = 1,
    /// 8-bit unsigned integer
    UInt8 = 2,
    /// 8-bit signed integer
    Int8 = 3,
    /// 16-bit unsigned integer
    UInt16 = 4,
    /// 16-bit signed integer
    Int16 = 5,
    /// 32-bit unsigned integer
    UInt32 = 6,
    /// 32-bit signed integer
    Int32 = 7,
    /// 64-bit unsigned integer
    UInt64 = 8,
    /// 64-bit signed integer
    Int64 = 9,
    /// 16-bit IEEE floating point (half)
    Half = 10,
    /// 32-bit IEEE floating point
    #[default]
    Float = 11,
    /// 64-bit IEEE floating point
    Double = 12,
    /// String (pointer to char)
    String = 13,
    /// Pointer (void*)
    Ptr = 14,
    /// Unique string hash (OIIO-specific)
    UstringHash = 15,
}

impl BaseType {
    /// Size in bytes of one element of this base type.
    #[inline]
    pub const fn size(&self) -> usize {
        match self {
            Self::Unknown | Self::None => 0,
            Self::UInt8 | Self::Int8 => 1,
            Self::UInt16 | Self::Int16 | Self::Half => 2,
            Self::UInt32 | Self::Int32 | Self::Float => 4,
            Self::UInt64 | Self::Int64 | Self::Double => 8,
            Self::String | Self::Ptr => std::mem::size_of::<usize>(),
            Self::UstringHash => 8, // 64-bit hash
        }
    }

    /// Returns true if this is a floating-point type.
    #[inline]
    pub const fn is_floating_point(&self) -> bool {
        matches!(self, Self::Half | Self::Float | Self::Double)
    }

    /// Returns true if this is a signed type.
    #[inline]
    pub const fn is_signed(&self) -> bool {
        matches!(
            self,
            Self::Int8
                | Self::Int16
                | Self::Int32
                | Self::Int64
                | Self::Half
                | Self::Float
                | Self::Double
        )
    }

    /// Returns true if this is an integer type.
    #[inline]
    pub const fn is_integer(&self) -> bool {
        matches!(
            self,
            Self::UInt8
                | Self::Int8
                | Self::UInt16
                | Self::Int16
                | Self::UInt32
                | Self::Int32
                | Self::UInt64
                | Self::Int64
        )
    }

    /// Short name for the type.
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::None => "none",
            Self::UInt8 => "uint8",
            Self::Int8 => "int8",
            Self::UInt16 => "uint16",
            Self::Int16 => "int16",
            Self::UInt32 => "uint32",
            Self::Int32 => "int32",
            Self::UInt64 => "uint64",
            Self::Int64 => "int64",
            Self::Half => "half",
            Self::Float => "float",
            Self::Double => "double",
            Self::String => "string",
            Self::Ptr => "ptr",
            Self::UstringHash => "ustringhash",
        }
    }

    /// Finds a common compatible type that can represent both input types without data loss.
    ///
    /// This follows OIIO's type promotion rules:
    /// - Same type returns that type
    /// - Unknown defers to the other type
    /// - Larger size wins
    /// - Float/Double trump all integer types
    /// - Falls back to Float for edge cases
    ///
    /// # Example
    /// ```
    /// use vfx_core::format::BaseType;
    /// assert_eq!(BaseType::merge(BaseType::UInt8, BaseType::UInt16), BaseType::UInt16);
    /// assert_eq!(BaseType::merge(BaseType::Int32, BaseType::Float), BaseType::Float);
    /// assert_eq!(BaseType::merge(BaseType::Half, BaseType::UInt8), BaseType::Half);
    /// ```
    #[inline]
    pub const fn merge(a: Self, b: Self) -> Self {
        // Same type already? done.
        if a as u8 == b as u8 {
            return a;
        }
        // Unknown defers to the other
        if matches!(a, Self::Unknown) {
            return b;
        }
        if matches!(b, Self::Unknown) {
            return a;
        }

        // Get sizes - need to canonicalize so 'big' has larger or equal size
        let size_a = a.size();
        let size_b = b.size();
        let (big, small) = if size_a >= size_b { (a, b) } else { (b, a) };

        // Float/Double trump anything else
        if matches!(big, Self::Double | Self::Float) {
            return big;
        }

        // Integer promotion rules
        match (big, small) {
            // UInt32 can hold UInt16/UInt8
            (Self::UInt32, Self::UInt16 | Self::UInt8) => big,
            // Int32 can hold all smaller integers (signed and unsigned)
            (Self::Int32, Self::Int16 | Self::UInt16 | Self::Int8 | Self::UInt8) => big,
            // UInt16/Half can hold UInt8
            (Self::UInt16 | Self::Half, Self::UInt8) => big,
            // Int16/Half can hold Int8/UInt8
            (Self::Int16 | Self::Half, Self::Int8 | Self::UInt8) => big,
            // UInt64 can hold smaller unsigned
            (Self::UInt64, Self::UInt32 | Self::UInt16 | Self::UInt8) => big,
            // Int64 can hold all smaller integers
            (
                Self::Int64,
                Self::Int32 | Self::UInt32 | Self::Int16 | Self::UInt16 | Self::Int8 | Self::UInt8,
            ) => big,
            // Edge cases: fall back to Float
            _ => Self::Float,
        }
    }
}

impl std::fmt::Display for BaseType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Aggregate type for TypeDesc (matches OIIO AGGREGATE).
///
/// Specifies how many base type elements are combined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum Aggregate {
    /// Single scalar value
    #[default]
    Scalar = 1,
    /// 2-component vector
    Vec2 = 2,
    /// 3-component vector
    Vec3 = 3,
    /// 4-component vector
    Vec4 = 4,
    /// 3x3 matrix (9 elements)
    Matrix33 = 9,
    /// 4x4 matrix (16 elements)
    Matrix44 = 16,
}

impl Aggregate {
    /// Number of base type elements in this aggregate.
    #[inline]
    pub const fn count(&self) -> usize {
        match self {
            Self::Scalar => 1,
            Self::Vec2 => 2,
            Self::Vec3 => 3,
            Self::Vec4 => 4,
            Self::Matrix33 => 9,
            Self::Matrix44 => 16,
        }
    }

    /// Returns true if this is a scalar (not a vector or matrix).
    #[inline]
    pub const fn is_scalar(&self) -> bool {
        matches!(self, Self::Scalar)
    }

    /// Returns true if this is a vector type.
    #[inline]
    pub const fn is_vector(&self) -> bool {
        matches!(self, Self::Vec2 | Self::Vec3 | Self::Vec4)
    }

    /// Returns true if this is a matrix type.
    #[inline]
    pub const fn is_matrix(&self) -> bool {
        matches!(self, Self::Matrix33 | Self::Matrix44)
    }
}

impl std::fmt::Display for Aggregate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Scalar => write!(f, "scalar"),
            Self::Vec2 => write!(f, "vec2"),
            Self::Vec3 => write!(f, "vec3"),
            Self::Vec4 => write!(f, "vec4"),
            Self::Matrix33 => write!(f, "matrix33"),
            Self::Matrix44 => write!(f, "matrix44"),
        }
    }
}

/// Vector semantics for TypeDesc (matches OIIO VECSEMANTICS).
///
/// Specifies the semantic meaning of vector types for proper transformation handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum VecSemantics {
    /// No transformation needed / No specific semantics
    #[default]
    NoSemantics = 0,
    /// Color value (no transformation)
    Color = 1,
    /// 3D point (transforms with full matrix)
    Point = 2,
    /// 3D direction vector (transforms without translation)
    Vector = 3,
    /// Surface normal (transforms with inverse transpose)
    Normal = 4,
    /// SMPTE timecode
    Timecode = 5,
    /// Film keycode
    Keycode = 6,
    /// Rational number (numerator/denominator)
    Rational = 7,
    /// Bounding box (min/max pairs) - OIIO compatibility
    Box = 8,
}

impl VecSemantics {
    /// Alias for NoSemantics (OIIO compatibility).
    pub const NOXFORM: Self = Self::NoSemantics;
}

impl VecSemantics {
    /// Returns true if this requires special transformation handling.
    #[inline]
    pub const fn requires_transform(&self) -> bool {
        matches!(self, Self::Point | Self::Vector | Self::Normal)
    }
}

impl std::fmt::Display for VecSemantics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSemantics => write!(f, ""),
            Self::Color => write!(f, "color"),
            Self::Point => write!(f, "point"),
            Self::Vector => write!(f, "vector"),
            Self::Normal => write!(f, "normal"),
            Self::Timecode => write!(f, "timecode"),
            Self::Keycode => write!(f, "keycode"),
            Self::Rational => write!(f, "rational"),
            Self::Box => write!(f, "box"),
        }
    }
}

/// Complete type descriptor (matches OIIO TypeDesc).
///
/// Describes the complete type of a value including base type, aggregation,
/// array size, and vector semantics.
///
/// # Example
///
/// ```rust
/// use vfx_core::format::{TypeDesc, BaseType, Aggregate, VecSemantics};
///
/// // A single float
/// let scalar = TypeDesc::FLOAT;
/// assert_eq!(scalar.size(), 4);
///
/// // RGB color (3 floats)
/// let color = TypeDesc::color();
/// assert_eq!(color.size(), 12);
///
/// // Array of 10 integers
/// let arr = TypeDesc::int_array(10);
/// assert_eq!(arr.size(), 40);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeDesc {
    /// Base data type
    pub basetype: BaseType,
    /// Aggregation of base type
    pub aggregate: Aggregate,
    /// Vector semantics
    pub vecsemantics: VecSemantics,
    /// Array length (0 means not an array, -1 means unsized array)
    pub arraylen: i32,
}

impl Default for TypeDesc {
    fn default() -> Self {
        Self::UNKNOWN
    }
}

impl TypeDesc {
    // Common type constants
    /// Unknown type
    pub const UNKNOWN: Self = Self::scalar(BaseType::Unknown);
    /// No type (void)
    pub const NONE: Self = Self::scalar(BaseType::None);
    /// Single uint8
    pub const UINT8: Self = Self::scalar(BaseType::UInt8);
    /// Single int8
    pub const INT8: Self = Self::scalar(BaseType::Int8);
    /// Single uint16
    pub const UINT16: Self = Self::scalar(BaseType::UInt16);
    /// Single int16
    pub const INT16: Self = Self::scalar(BaseType::Int16);
    /// Single uint32
    pub const UINT32: Self = Self::scalar(BaseType::UInt32);
    /// Single int32
    pub const INT32: Self = Self::scalar(BaseType::Int32);
    /// Single uint64
    pub const UINT64: Self = Self::scalar(BaseType::UInt64);
    /// Single int64
    pub const INT64: Self = Self::scalar(BaseType::Int64);
    /// Single half (16-bit float)
    pub const HALF: Self = Self::scalar(BaseType::Half);
    /// Single float (32-bit)
    pub const FLOAT: Self = Self::scalar(BaseType::Float);
    /// Single double (64-bit float)
    pub const DOUBLE: Self = Self::scalar(BaseType::Double);
    /// String type
    pub const STRING: Self = Self::scalar(BaseType::String);
    /// Pointer type
    pub const PTR: Self = Self::scalar(BaseType::Ptr);

    /// Creates a new TypeDesc with the given base type and aggregate.
    #[inline]
    pub const fn new(basetype: BaseType, aggregate: Aggregate) -> Self {
        Self {
            basetype,
            aggregate,
            vecsemantics: VecSemantics::NoSemantics,
            arraylen: 0,
        }
    }

    /// Creates a scalar TypeDesc.
    #[inline]
    pub const fn scalar(basetype: BaseType) -> Self {
        Self::new(basetype, Aggregate::Scalar)
    }

    /// Creates a TypeDesc with vector semantics.
    #[inline]
    pub const fn with_semantics(mut self, semantics: VecSemantics) -> Self {
        self.vecsemantics = semantics;
        self
    }

    /// Creates an array TypeDesc.
    #[inline]
    pub const fn array(mut self, len: i32) -> Self {
        self.arraylen = len;
        self
    }

    /// Creates a color type (Vec3 float with color semantics).
    #[inline]
    pub const fn color() -> Self {
        Self {
            basetype: BaseType::Float,
            aggregate: Aggregate::Vec3,
            vecsemantics: VecSemantics::Color,
            arraylen: 0,
        }
    }

    /// Creates a point type (Vec3 float with point semantics).
    #[inline]
    pub const fn point() -> Self {
        Self {
            basetype: BaseType::Float,
            aggregate: Aggregate::Vec3,
            vecsemantics: VecSemantics::Point,
            arraylen: 0,
        }
    }

    /// Creates a vector type (Vec3 float with vector semantics).
    #[inline]
    pub const fn vector() -> Self {
        Self {
            basetype: BaseType::Float,
            aggregate: Aggregate::Vec3,
            vecsemantics: VecSemantics::Vector,
            arraylen: 0,
        }
    }

    /// Creates a normal type (Vec3 float with normal semantics).
    #[inline]
    pub const fn normal() -> Self {
        Self {
            basetype: BaseType::Float,
            aggregate: Aggregate::Vec3,
            vecsemantics: VecSemantics::Normal,
            arraylen: 0,
        }
    }

    /// Creates a 4x4 matrix type.
    #[inline]
    pub const fn matrix44() -> Self {
        Self::new(BaseType::Float, Aggregate::Matrix44)
    }

    /// Creates a 3x3 matrix type.
    #[inline]
    pub const fn matrix33() -> Self {
        Self::new(BaseType::Float, Aggregate::Matrix33)
    }

    /// Creates an int array type.
    #[inline]
    pub const fn int_array(len: i32) -> Self {
        Self::INT32.array(len)
    }

    /// Creates a float array type.
    #[inline]
    pub const fn float_array(len: i32) -> Self {
        Self::FLOAT.array(len)
    }

    /// Size of one base element in bytes.
    #[inline]
    pub const fn basesize(&self) -> usize {
        self.basetype.size()
    }

    /// Size of one aggregate element (base * aggregate count).
    #[inline]
    pub const fn elementsize(&self) -> usize {
        self.basetype.size() * self.aggregate.count()
    }

    /// Total size in bytes.
    #[inline]
    pub const fn size(&self) -> usize {
        let base = self.elementsize();
        if self.arraylen > 0 {
            base * self.arraylen as usize
        } else {
            base
        }
    }

    /// Number of base type elements in one aggregate.
    #[inline]
    pub const fn basevalues(&self) -> usize {
        self.aggregate.count()
    }

    /// Number of array elements (1 if not an array).
    #[inline]
    pub const fn numelements(&self) -> usize {
        if self.arraylen > 0 {
            self.arraylen as usize
        } else {
            1
        }
    }

    /// Returns true if this is an array type.
    #[inline]
    pub const fn is_array(&self) -> bool {
        self.arraylen != 0
    }

    /// Returns true if this is a sized array.
    #[inline]
    pub const fn is_sized_array(&self) -> bool {
        self.arraylen > 0
    }

    /// Returns true if this is an unsized array (arraylen == -1).
    #[inline]
    pub const fn is_unsized_array(&self) -> bool {
        self.arraylen < 0
    }

    /// Returns true if this is a floating-point type.
    #[inline]
    pub const fn is_floating_point(&self) -> bool {
        self.basetype.is_floating_point()
    }

    /// Returns true if this is a signed type.
    #[inline]
    pub const fn is_signed(&self) -> bool {
        self.basetype.is_signed()
    }

    /// Returns true if this type is unknown.
    #[inline]
    pub const fn is_unknown(&self) -> bool {
        matches!(self.basetype, BaseType::Unknown)
    }

    /// Finds a common compatible base type that can represent both input types.
    ///
    /// This is the OIIO-compatible static method that operates on TypeDesc inputs
    /// but only considers the basetype for merging.
    ///
    /// # Example
    /// ```
    /// use vfx_core::format::TypeDesc;
    /// let merged = TypeDesc::basetype_merge(TypeDesc::UINT8, TypeDesc::UINT16);
    /// assert_eq!(merged, vfx_core::format::BaseType::UInt16);
    /// ```
    #[inline]
    pub const fn basetype_merge(a: Self, b: Self) -> BaseType {
        BaseType::merge(a.basetype, b.basetype)
    }

    /// Returns the type of a single element (strips array).
    #[inline]
    pub const fn elementtype(&self) -> Self {
        Self {
            basetype: self.basetype,
            aggregate: self.aggregate,
            vecsemantics: self.vecsemantics,
            arraylen: 0,
        }
    }

    /// Returns the scalar base type (strips aggregate and array).
    #[inline]
    pub const fn scalartype(&self) -> Self {
        Self::scalar(self.basetype)
    }

    /// Returns this type without array specification.
    #[inline]
    pub const fn unarray(&self) -> Self {
        self.elementtype()
    }

    /// Equivalent types (same base and aggregate, ignoring semantics and array).
    #[inline]
    pub const fn equivalent(&self, other: &Self) -> bool {
        self.basetype as u8 == other.basetype as u8 && self.aggregate as u8 == other.aggregate as u8
    }

    /// Creates a TypeDesc from a BaseType.
    #[inline]
    pub const fn from_basetype(basetype: BaseType) -> Self {
        Self::scalar(basetype)
    }

    /// Creates a TypeDesc from a DataFormat.
    #[inline]
    pub const fn from_format(format: DataFormat) -> Self {
        match format {
            DataFormat::U8 => Self::UINT8,
            DataFormat::U16 => Self::UINT16,
            DataFormat::U32 => Self::UINT32,
            DataFormat::F16 => Self::HALF,
            DataFormat::F32 => Self::FLOAT,
            DataFormat::I8 => Self::INT8,
            DataFormat::I16 => Self::INT16,
            DataFormat::I32 => Self::INT32,
            DataFormat::F64 => Self::DOUBLE,
        }
    }

    // =========================================================================
    // OIIO Parity Methods
    // =========================================================================

    /// Returns true if this is a Vec2 type (optionally of specific basetype).
    /// If basetype is None, checks any Vec2. If Some, checks specific base.
    #[inline]
    pub fn is_vec2(&self, basetype: Option<BaseType>) -> bool {
        self.aggregate == Aggregate::Vec2 && basetype.is_none_or(|bt| self.basetype == bt)
    }

    /// Returns true if this is a Vec3 type (optionally of specific basetype).
    #[inline]
    pub fn is_vec3(&self, basetype: Option<BaseType>) -> bool {
        self.aggregate == Aggregate::Vec3 && basetype.is_none_or(|bt| self.basetype == bt)
    }

    /// Returns true if this is a Vec4 type (optionally of specific basetype).
    #[inline]
    pub fn is_vec4(&self, basetype: Option<BaseType>) -> bool {
        self.aggregate == Aggregate::Vec4 && basetype.is_none_or(|bt| self.basetype == bt)
    }

    /// Returns true if this is a 2D bounding box type (Vec2 with Box semantics, min+max = 4 values).
    /// Optionally checks for specific basetype.
    #[inline]
    pub fn is_box2(&self, basetype: Option<BaseType>) -> bool {
        self.aggregate == Aggregate::Vec4  // 4 values: minx, miny, maxx, maxy
            && self.vecsemantics == VecSemantics::Box
            && basetype.is_none_or(|bt| self.basetype == bt)
    }

    /// Returns true if this is a 3D bounding box type (6 values: min xyz, max xyz).
    /// Represented as array of 2 Vec3 with Box semantics.
    #[inline]
    pub fn is_box3(&self, basetype: Option<BaseType>) -> bool {
        // Box3 is 6 values, represented as Vec3[2] with Box semantics
        self.aggregate == Aggregate::Vec3
            && self.arraylen == 2
            && self.vecsemantics == VecSemantics::Box
            && basetype.is_none_or(|bt| self.basetype == bt)
    }

    /// Returns true if this TypeDesc represents a valid (non-unknown) type.
    /// Equivalent to OIIO's operator bool().
    #[inline]
    pub const fn is_valid(&self) -> bool {
        !matches!(self.basetype, BaseType::Unknown)
    }

    /// Creates a 2D bounding box type (4 floats with Box semantics).
    #[inline]
    pub const fn box2() -> Self {
        Self {
            basetype: BaseType::Float,
            aggregate: Aggregate::Vec4,
            vecsemantics: VecSemantics::Box,
            arraylen: 0,
        }
    }

    /// Creates a 2D integer bounding box type.
    #[inline]
    pub const fn box2i() -> Self {
        Self {
            basetype: BaseType::Int32,
            aggregate: Aggregate::Vec4,
            vecsemantics: VecSemantics::Box,
            arraylen: 0,
        }
    }

    /// Creates a 3D bounding box type (6 floats: min xyz, max xyz).
    #[inline]
    pub const fn box3() -> Self {
        Self {
            basetype: BaseType::Float,
            aggregate: Aggregate::Vec3,
            vecsemantics: VecSemantics::Box,
            arraylen: 2, // 2 Vec3s = 6 values
        }
    }

    /// Creates a 3D integer bounding box type.
    #[inline]
    pub const fn box3i() -> Self {
        Self {
            basetype: BaseType::Int32,
            aggregate: Aggregate::Vec3,
            vecsemantics: VecSemantics::Box,
            arraylen: 2,
        }
    }

    /// Creates a Vec2 float type.
    #[inline]
    pub const fn float2() -> Self {
        Self::new(BaseType::Float, Aggregate::Vec2)
    }

    /// Creates a Vec4 float type.
    #[inline]
    pub const fn float4() -> Self {
        Self::new(BaseType::Float, Aggregate::Vec4)
    }

    /// Creates a Vec2 int type.
    #[inline]
    pub const fn int2() -> Self {
        Self::new(BaseType::Int32, Aggregate::Vec2)
    }

    /// Creates a Vec3 int type.
    #[inline]
    pub const fn int3() -> Self {
        Self::new(BaseType::Int32, Aggregate::Vec3)
    }

    /// Creates a UstringHash type.
    pub const USTRINGHASH: Self = Self::scalar(BaseType::UstringHash);

    // Predefined box types (OIIO compatibility)
    /// 2D float bounding box
    pub const TYPE_BOX2: Self = Self::box2();
    /// 2D integer bounding box
    pub const TYPE_BOX2I: Self = Self::box2i();
    /// 3D float bounding box
    pub const TYPE_BOX3: Self = Self::box3();
    /// 3D integer bounding box
    pub const TYPE_BOX3I: Self = Self::box3i();
}

/// Lexicographic ordering for TypeDesc (OIIO parity).
impl Ord for TypeDesc {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Compare basetype, then aggregate, then vecsemantics, then arraylen
        (self.basetype as u8)
            .cmp(&(other.basetype as u8))
            .then((self.aggregate as u8).cmp(&(other.aggregate as u8)))
            .then((self.vecsemantics as u8).cmp(&(other.vecsemantics as u8)))
            .then(self.arraylen.cmp(&other.arraylen))
    }
}

impl PartialOrd for TypeDesc {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Parse error for TypeDesc string parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDescParseError {
    /// The input string that failed to parse.
    pub input: String,
    /// The reason for the parse failure.
    pub reason: &'static str,
}

impl std::fmt::Display for TypeDescParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid TypeDesc '{}': {}", self.input, self.reason)
    }
}

impl std::error::Error for TypeDescParseError {}

impl std::str::FromStr for TypeDesc {
    type Err = TypeDescParseError;

    /// Parse TypeDesc from string (OIIO-compatible format).
    ///
    /// Supported formats:
    /// - Base types: "float", "int", "uint8", "half", "double", etc.
    /// - Semantic types: "color", "point", "vector", "normal"
    /// - Matrix types: "matrix33", "matrix44", "matrix"
    /// - Vector types: "float[2]", "float[3]", "float[4]"
    /// - Arrays: "float[10]", "int[]" (unsized)
    ///
    /// Examples:
    /// ```rust
    /// use vfx_core::format::TypeDesc;
    /// let td: TypeDesc = "color".parse().unwrap();
    /// let td: TypeDesc = "float[3]".parse().unwrap();
    /// let td: TypeDesc = "matrix44".parse().unwrap();
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.is_empty() {
            return Err(TypeDescParseError {
                input: s.to_string(),
                reason: "empty string",
            });
        }

        // Check for semantic types first
        match s {
            "color" => return Ok(Self::color()),
            "point" => return Ok(Self::point()),
            "vector" => return Ok(Self::vector()),
            "normal" => return Ok(Self::normal()),
            "matrix" | "matrix44" => return Ok(Self::matrix44()),
            "matrix33" => return Ok(Self::matrix33()),
            "timecode" => {
                return Ok(Self {
                    basetype: BaseType::UInt32,
                    aggregate: Aggregate::Vec2,
                    vecsemantics: VecSemantics::Timecode,
                    arraylen: 0,
                });
            }
            "keycode" => {
                return Ok(Self {
                    basetype: BaseType::Int32,
                    aggregate: Aggregate::Vec4,
                    vecsemantics: VecSemantics::Keycode,
                    arraylen: 0,
                });
            }
            "rational" => {
                return Ok(Self {
                    basetype: BaseType::Int32,
                    aggregate: Aggregate::Vec2,
                    vecsemantics: VecSemantics::Rational,
                    arraylen: 0,
                });
            }
            "box2" | "box2f" => return Ok(Self::box2()),
            "box2i" => return Ok(Self::box2i()),
            "box3" | "box3f" => return Ok(Self::box3()),
            "box3i" => return Ok(Self::box3i()),
            _ => {}
        }

        // Check for array suffix: basetype[N] or basetype[]
        let (base_str, array_len): (&str, i32) = if let Some(bracket_pos) = s.find('[') {
            let base = &s[..bracket_pos];
            let suffix = &s[bracket_pos..];
            if suffix == "[]" {
                (base, -1i32) // Unsized array
            } else if suffix.starts_with('[') && suffix.ends_with(']') {
                let num_str = &suffix[1..suffix.len() - 1];
                // Check if it's aggregate notation like float[3] for Vec3
                let n = num_str.parse::<i32>().map_err(|_| TypeDescParseError {
                    input: s.to_string(),
                    reason: "invalid array length number",
                })?;

                if n <= 0 {
                    return Err(TypeDescParseError {
                        input: s.to_string(),
                        reason: "invalid array length",
                    });
                }

                // For n=2,3,4 - check if it's a vector type notation
                if (2..=4).contains(&n)
                    && let Ok(bt) = parse_basetype(base)
                {
                    let agg = match n {
                        2 => Aggregate::Vec2,
                        3 => Aggregate::Vec3,
                        4 => Aggregate::Vec4,
                        _ => unreachable!(),
                    };
                    return Ok(Self::new(bt, agg));
                }

                // Otherwise it's an array
                (base, n)
            } else {
                return Err(TypeDescParseError {
                    input: s.to_string(),
                    reason: "malformed array syntax",
                });
            }
        } else {
            (s, 0)
        };

        // Parse base type
        let basetype = parse_basetype(base_str).map_err(|_| TypeDescParseError {
            input: s.to_string(),
            reason: "unknown base type",
        })?;

        Ok(Self {
            basetype,
            aggregate: Aggregate::Scalar,
            vecsemantics: VecSemantics::NoSemantics,
            arraylen: array_len,
        })
    }
}

/// Helper to parse base type from string.
fn parse_basetype(s: &str) -> Result<BaseType, ()> {
    match s.to_lowercase().as_str() {
        "unknown" => Ok(BaseType::Unknown),
        "none" | "void" => Ok(BaseType::None),
        "uint8" | "uchar" | "unsigned char" => Ok(BaseType::UInt8),
        "int8" | "char" | "signed char" => Ok(BaseType::Int8),
        "uint16" | "ushort" | "unsigned short" => Ok(BaseType::UInt16),
        "int16" | "short" => Ok(BaseType::Int16),
        "uint32" | "uint" | "unsigned int" => Ok(BaseType::UInt32),
        "int32" | "int" => Ok(BaseType::Int32),
        "uint64" | "ulong" | "unsigned long" => Ok(BaseType::UInt64),
        "int64" | "long" => Ok(BaseType::Int64),
        "half" | "float16" => Ok(BaseType::Half),
        "float" | "float32" => Ok(BaseType::Float),
        "double" | "float64" => Ok(BaseType::Double),
        "string" => Ok(BaseType::String),
        "ptr" | "pointer" => Ok(BaseType::Ptr),
        "ustringhash" => Ok(BaseType::UstringHash),
        _ => Err(()),
    }
}

impl std::fmt::Display for TypeDesc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Format semantics if present
        let sem = match self.vecsemantics {
            VecSemantics::Color => "color",
            VecSemantics::Point => "point",
            VecSemantics::Vector => "vector",
            VecSemantics::Normal => "normal",
            _ => "",
        };

        if !sem.is_empty() {
            write!(f, "{}", sem)?;
        } else {
            // Base type and aggregate
            match self.aggregate {
                Aggregate::Scalar => write!(f, "{}", self.basetype)?,
                Aggregate::Vec2 => write!(f, "{}[2]", self.basetype)?,
                Aggregate::Vec3 => write!(f, "{}[3]", self.basetype)?,
                Aggregate::Vec4 => write!(f, "{}[4]", self.basetype)?,
                Aggregate::Matrix33 => write!(f, "matrix33")?,
                Aggregate::Matrix44 => write!(f, "matrix44")?,
            }
        }

        // Array suffix
        if self.arraylen > 0 {
            write!(f, "[{}]", self.arraylen)?;
        } else if self.arraylen < 0 {
            write!(f, "[]")?;
        }

        Ok(())
    }
}

// Conversions from DataFormat to TypeDesc
impl From<DataFormat> for TypeDesc {
    fn from(fmt: DataFormat) -> Self {
        match fmt {
            DataFormat::U8 => TypeDesc::UINT8,
            DataFormat::U16 => TypeDesc::UINT16,
            DataFormat::U32 => TypeDesc::UINT32,
            DataFormat::F16 => TypeDesc::HALF,
            DataFormat::F32 => TypeDesc::FLOAT,
            DataFormat::I8 => TypeDesc::INT8,
            DataFormat::I16 => TypeDesc::INT16,
            DataFormat::I32 => TypeDesc::INT32,
            DataFormat::F64 => TypeDesc::DOUBLE,
        }
    }
}

impl From<TypeDesc> for DataFormat {
    fn from(td: TypeDesc) -> Self {
        match td.basetype {
            BaseType::UInt8 => DataFormat::U8,
            BaseType::Int8 => DataFormat::I8,
            BaseType::UInt16 => DataFormat::U16,
            BaseType::Int16 => DataFormat::I16,
            BaseType::Half => DataFormat::F16,
            BaseType::UInt32 => DataFormat::U32,
            BaseType::Int32 => DataFormat::I32,
            BaseType::Float => DataFormat::F32,
            BaseType::Double => DataFormat::F64,
            _ => DataFormat::F32, // Default fallback
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bit_depth_bits() {
        assert_eq!(BitDepth::U8.bits(), 8);
        assert_eq!(BitDepth::U10.bits(), 10);
        assert_eq!(BitDepth::U12.bits(), 12);
        assert_eq!(BitDepth::U16.bits(), 16);
        assert_eq!(BitDepth::F16.bits(), 16);
        assert_eq!(BitDepth::F32.bits(), 32);
    }

    #[test]
    fn test_storage_format() {
        assert_eq!(BitDepth::U8.storage_format(), DataFormat::U8);
        assert_eq!(BitDepth::U10.storage_format(), DataFormat::U16);
        assert_eq!(BitDepth::U12.storage_format(), DataFormat::U16);
        assert_eq!(BitDepth::U16.storage_format(), DataFormat::U16);
        assert_eq!(BitDepth::F16.storage_format(), DataFormat::F16);
        assert_eq!(BitDepth::F32.storage_format(), DataFormat::F32);
    }

    #[test]
    fn test_is_float() {
        assert!(!BitDepth::U8.is_float());
        assert!(!BitDepth::U10.is_float());
        assert!(BitDepth::F16.is_float());
        assert!(BitDepth::F32.is_float());
    }

    #[test]
    fn test_is_integer() {
        assert!(BitDepth::U8.is_integer());
        assert!(BitDepth::U10.is_integer());
        assert!(BitDepth::U12.is_integer());
        assert!(BitDepth::U16.is_integer());
        assert!(BitDepth::U32.is_integer());
        assert!(!BitDepth::F16.is_integer());
        assert!(!BitDepth::F32.is_integer());
        // Unknown should NOT be considered integer
        assert!(!BitDepth::Unknown.is_integer());
    }

    #[test]
    fn test_normalize_factor() {
        assert_eq!(BitDepth::U8.normalize_factor(), 255.0);
        assert_eq!(BitDepth::U10.normalize_factor(), 1023.0);
        assert_eq!(BitDepth::U12.normalize_factor(), 4095.0);
        assert_eq!(BitDepth::F32.normalize_factor(), 1.0);
    }

    #[test]
    fn test_conversions() {
        // DataFormat -> BitDepth
        assert_eq!(BitDepth::from(DataFormat::U8), BitDepth::U8);
        assert_eq!(BitDepth::from(DataFormat::F16), BitDepth::F16);

        // BitDepth -> DataFormat (storage)
        assert_eq!(DataFormat::from(BitDepth::U10), DataFormat::U16);
        assert_eq!(DataFormat::from(BitDepth::F32), DataFormat::F32);
    }

    #[test]
    fn test_signed_integer_formats() {
        // Bytes per channel
        assert_eq!(DataFormat::I8.bytes_per_channel(), 1);
        assert_eq!(DataFormat::I16.bytes_per_channel(), 2);
        assert_eq!(DataFormat::I32.bytes_per_channel(), 4);
        assert_eq!(DataFormat::F64.bytes_per_channel(), 8);

        // Float / integer classification
        assert!(!DataFormat::I8.is_float());
        assert!(!DataFormat::I16.is_float());
        assert!(!DataFormat::I32.is_float());
        assert!(DataFormat::F64.is_float());

        assert!(DataFormat::I8.is_integer());
        assert!(DataFormat::I16.is_integer());
        assert!(DataFormat::I32.is_integer());
        assert!(!DataFormat::F64.is_integer());

        // Signed flag
        assert!(DataFormat::I8.is_signed());
        assert!(DataFormat::I16.is_signed());
        assert!(DataFormat::I32.is_signed());
        assert!(DataFormat::F64.is_signed());
        assert!(!DataFormat::U8.is_signed());
        assert!(!DataFormat::U32.is_signed());
    }

    #[test]
    fn test_dataformat_normalize_factor() {
        // Unsigned
        assert_eq!(DataFormat::U8.normalize_factor(), 255.0f32);
        assert_eq!(DataFormat::U16.normalize_factor(), 65535.0f32);
        // Signed
        assert_eq!(DataFormat::I8.normalize_factor(), 127.0f32);
        assert_eq!(DataFormat::I16.normalize_factor(), 32767.0f32);
        assert_eq!(DataFormat::I32.normalize_factor(), i32::MAX as f32);
        // Floats
        assert_eq!(DataFormat::F16.normalize_factor(), 1.0f32);
        assert_eq!(DataFormat::F32.normalize_factor(), 1.0f32);
        assert_eq!(DataFormat::F64.normalize_factor(), 1.0f32);

        // f64 precision: u32::MAX is not exact as f32 (rounds up to 4294967296.0)
        // but exact as f64
        assert_eq!(DataFormat::U32.normalize_factor_f64(), u32::MAX as f64);
        assert_eq!(DataFormat::I32.normalize_factor_f64(), i32::MAX as f64);
        // Verify f32 rounding: these two lose precision
        assert_ne!(DataFormat::U32.normalize_factor() as f64, u32::MAX as f64);
    }

    #[test]
    fn test_dataformat_typedesc_roundtrip() {
        // New types should roundtrip through TypeDesc correctly
        assert_eq!(TypeDesc::from(DataFormat::I8), TypeDesc::INT8);
        assert_eq!(TypeDesc::from(DataFormat::I16), TypeDesc::INT16);
        assert_eq!(TypeDesc::from(DataFormat::I32), TypeDesc::INT32);
        assert_eq!(TypeDesc::from(DataFormat::F64), TypeDesc::DOUBLE);

        // And back
        assert_eq!(DataFormat::from(TypeDesc::INT8), DataFormat::I8);
        assert_eq!(DataFormat::from(TypeDesc::INT16), DataFormat::I16);
        assert_eq!(DataFormat::from(TypeDesc::INT32), DataFormat::I32);
        assert_eq!(DataFormat::from(TypeDesc::DOUBLE), DataFormat::F64);
    }

    #[test]
    fn test_dataformat_name() {
        assert_eq!(DataFormat::I8.name(), "i8");
        assert_eq!(DataFormat::I16.name(), "i16");
        assert_eq!(DataFormat::I32.name(), "i32");
        assert_eq!(DataFormat::F64.name(), "f64");
    }

    // TypeDesc tests
    #[test]
    fn test_basetype_size() {
        assert_eq!(BaseType::UInt8.size(), 1);
        assert_eq!(BaseType::UInt16.size(), 2);
        assert_eq!(BaseType::Half.size(), 2);
        assert_eq!(BaseType::Float.size(), 4);
        assert_eq!(BaseType::Double.size(), 8);
    }

    #[test]
    fn test_basetype_properties() {
        assert!(BaseType::Float.is_floating_point());
        assert!(BaseType::Half.is_floating_point());
        assert!(!BaseType::UInt32.is_floating_point());

        assert!(BaseType::Int32.is_signed());
        assert!(BaseType::Float.is_signed());
        assert!(!BaseType::UInt32.is_signed());

        assert!(BaseType::UInt32.is_integer());
        assert!(!BaseType::Float.is_integer());
    }

    #[test]
    fn test_aggregate_count() {
        assert_eq!(Aggregate::Scalar.count(), 1);
        assert_eq!(Aggregate::Vec2.count(), 2);
        assert_eq!(Aggregate::Vec3.count(), 3);
        assert_eq!(Aggregate::Vec4.count(), 4);
        assert_eq!(Aggregate::Matrix33.count(), 9);
        assert_eq!(Aggregate::Matrix44.count(), 16);
    }

    #[test]
    fn test_typedesc_size() {
        assert_eq!(TypeDesc::FLOAT.size(), 4);
        assert_eq!(TypeDesc::UINT8.size(), 1);
        assert_eq!(TypeDesc::color().size(), 12); // 3 * 4
        assert_eq!(TypeDesc::matrix44().size(), 64); // 16 * 4
        assert_eq!(TypeDesc::int_array(10).size(), 40); // 10 * 4
    }

    #[test]
    fn test_typedesc_properties() {
        let color = TypeDesc::color();
        assert!(color.is_floating_point());
        assert!(!color.is_array());
        assert_eq!(color.basevalues(), 3);

        let arr = TypeDesc::float_array(5);
        assert!(arr.is_array());
        assert!(arr.is_sized_array());
        assert_eq!(arr.numelements(), 5);
    }

    #[test]
    fn test_typedesc_elementtype() {
        let arr = TypeDesc::float_array(10);
        let elem = arr.elementtype();
        assert!(!elem.is_array());
        assert_eq!(elem.size(), 4);
    }

    #[test]
    fn test_typedesc_equivalence() {
        let color1 = TypeDesc::color();
        let color2 = TypeDesc::point(); // Same base/aggregate, different semantics
        assert!(color1.equivalent(&color2));

        let scalar = TypeDesc::FLOAT;
        assert!(!color1.equivalent(&scalar));
    }

    #[test]
    fn test_typedesc_from_dataformat() {
        assert_eq!(TypeDesc::from(DataFormat::F32), TypeDesc::FLOAT);
        assert_eq!(TypeDesc::from(DataFormat::U8), TypeDesc::UINT8);
        assert_eq!(TypeDesc::from(DataFormat::F16), TypeDesc::HALF);
    }

    // =========================================================================
    // OIIO Parity Tests
    // =========================================================================

    #[test]
    fn test_typedesc_is_vec() {
        let v2 = TypeDesc::float2();
        let v3 = TypeDesc::color();
        let v4 = TypeDesc::float4();
        let scalar = TypeDesc::FLOAT;

        assert!(v2.is_vec2(None));
        assert!(v2.is_vec2(Some(BaseType::Float)));
        assert!(!v2.is_vec2(Some(BaseType::Int32)));

        assert!(v3.is_vec3(None));
        assert!(v3.is_vec3(Some(BaseType::Float)));

        assert!(v4.is_vec4(None));
        assert!(!scalar.is_vec2(None));
        assert!(!scalar.is_vec3(None));
        assert!(!scalar.is_vec4(None));
    }

    #[test]
    fn test_typedesc_box_types() {
        let b2 = TypeDesc::box2();
        let b2i = TypeDesc::box2i();
        let b3 = TypeDesc::box3();
        let b3i = TypeDesc::box3i();

        assert!(b2.is_box2(None));
        assert!(b2.is_box2(Some(BaseType::Float)));
        assert!(!b2.is_box2(Some(BaseType::Int32)));

        assert!(b2i.is_box2(None));
        assert!(b2i.is_box2(Some(BaseType::Int32)));

        assert!(b3.is_box3(None));
        assert!(b3.is_box3(Some(BaseType::Float)));

        assert!(b3i.is_box3(None));
        assert!(b3i.is_box3(Some(BaseType::Int32)));

        // Ensure not misidentified
        assert!(!b2.is_box3(None));
        assert!(!b3.is_box2(None));
    }

    #[test]
    fn test_typedesc_is_valid() {
        assert!(TypeDesc::FLOAT.is_valid());
        assert!(TypeDesc::color().is_valid());
        assert!(TypeDesc::matrix44().is_valid());
        assert!(!TypeDesc::UNKNOWN.is_valid());
    }

    #[test]
    fn test_typedesc_ordering() {
        // Ord trait for use in BTreeMap/BTreeSet
        let t1 = TypeDesc::FLOAT;
        let t2 = TypeDesc::DOUBLE;
        let t3 = TypeDesc::INT32;

        assert!(t1 < t2); // Float (11) < Double (12)
        assert!(t3 < t1); // Int32 (7) < Float (11)

        // Test in sorted collection
        let mut types = [t2, t1, t3];
        types.sort();
        assert_eq!(types[0], t3);
        assert_eq!(types[1], t1);
        assert_eq!(types[2], t2);
    }

    #[test]
    fn test_typedesc_from_str() {
        // Basic types
        assert_eq!("float".parse::<TypeDesc>().unwrap(), TypeDesc::FLOAT);
        assert_eq!("int".parse::<TypeDesc>().unwrap(), TypeDesc::INT32);
        assert_eq!("double".parse::<TypeDesc>().unwrap(), TypeDesc::DOUBLE);
        assert_eq!("half".parse::<TypeDesc>().unwrap(), TypeDesc::HALF);

        // Semantic types
        assert_eq!("color".parse::<TypeDesc>().unwrap(), TypeDesc::color());
        assert_eq!("point".parse::<TypeDesc>().unwrap(), TypeDesc::point());
        assert_eq!("vector".parse::<TypeDesc>().unwrap(), TypeDesc::vector());
        assert_eq!("normal".parse::<TypeDesc>().unwrap(), TypeDesc::normal());

        // Matrix types
        assert_eq!(
            "matrix44".parse::<TypeDesc>().unwrap(),
            TypeDesc::matrix44()
        );
        assert_eq!(
            "matrix33".parse::<TypeDesc>().unwrap(),
            TypeDesc::matrix33()
        );
        assert_eq!("matrix".parse::<TypeDesc>().unwrap(), TypeDesc::matrix44());

        // Vector notation
        assert_eq!(
            "float[3]".parse::<TypeDesc>().unwrap(),
            TypeDesc::new(BaseType::Float, Aggregate::Vec3)
        );
        assert_eq!(
            "int[2]".parse::<TypeDesc>().unwrap(),
            TypeDesc::new(BaseType::Int32, Aggregate::Vec2)
        );

        // Box types
        assert_eq!("box2".parse::<TypeDesc>().unwrap(), TypeDesc::box2());
        assert_eq!("box3".parse::<TypeDesc>().unwrap(), TypeDesc::box3());
        assert_eq!("box2i".parse::<TypeDesc>().unwrap(), TypeDesc::box2i());

        // Invalid
        assert!("invalid_type".parse::<TypeDesc>().is_err());
        assert!("".parse::<TypeDesc>().is_err());
    }

    #[test]
    fn test_typedesc_vec_semantics_box() {
        assert_eq!(VecSemantics::Box as u8, 8);
    }

    #[test]
    fn test_basetype_ustringhash() {
        assert_eq!(BaseType::UstringHash as u8, 15);
        assert_eq!(BaseType::UstringHash.size(), 8);
        assert_eq!(BaseType::UstringHash.name(), "ustringhash");
    }

    #[test]
    fn test_basetype_merge() {
        // Same type returns that type
        assert_eq!(
            BaseType::merge(BaseType::Float, BaseType::Float),
            BaseType::Float
        );
        assert_eq!(
            BaseType::merge(BaseType::Int32, BaseType::Int32),
            BaseType::Int32
        );

        // Unknown defers to the other
        assert_eq!(
            BaseType::merge(BaseType::Unknown, BaseType::Float),
            BaseType::Float
        );
        assert_eq!(
            BaseType::merge(BaseType::Int32, BaseType::Unknown),
            BaseType::Int32
        );
        assert_eq!(
            BaseType::merge(BaseType::Unknown, BaseType::Unknown),
            BaseType::Unknown
        );

        // Float/Double trump everything
        assert_eq!(
            BaseType::merge(BaseType::Float, BaseType::Int32),
            BaseType::Float
        );
        assert_eq!(
            BaseType::merge(BaseType::Double, BaseType::Float),
            BaseType::Double
        );
        assert_eq!(
            BaseType::merge(BaseType::Double, BaseType::UInt64),
            BaseType::Double
        );
        assert_eq!(
            BaseType::merge(BaseType::Float, BaseType::UInt8),
            BaseType::Float
        );

        // Integer promotion: larger wins for compatible types
        assert_eq!(
            BaseType::merge(BaseType::UInt8, BaseType::UInt16),
            BaseType::UInt16
        );
        assert_eq!(
            BaseType::merge(BaseType::UInt16, BaseType::UInt32),
            BaseType::UInt32
        );
        assert_eq!(
            BaseType::merge(BaseType::Int8, BaseType::Int32),
            BaseType::Int32
        );
        assert_eq!(
            BaseType::merge(BaseType::UInt8, BaseType::Int32),
            BaseType::Int32
        );

        // Half can hold UInt8
        assert_eq!(
            BaseType::merge(BaseType::Half, BaseType::UInt8),
            BaseType::Half
        );
        assert_eq!(
            BaseType::merge(BaseType::Half, BaseType::Int8),
            BaseType::Half
        );

        // 64-bit integer promotion
        assert_eq!(
            BaseType::merge(BaseType::UInt32, BaseType::UInt64),
            BaseType::UInt64
        );
        assert_eq!(
            BaseType::merge(BaseType::Int32, BaseType::Int64),
            BaseType::Int64
        );

        // Edge cases fallback to Float
        assert_eq!(
            BaseType::merge(BaseType::String, BaseType::Ptr),
            BaseType::Float
        );
        assert_eq!(
            BaseType::merge(BaseType::None, BaseType::Int32),
            BaseType::Float
        );
    }

    #[test]
    fn test_typedesc_basetype_merge() {
        // Test the TypeDesc static method wrapper
        let merged = TypeDesc::basetype_merge(TypeDesc::UINT8, TypeDesc::UINT16);
        assert_eq!(merged, BaseType::UInt16);

        let merged = TypeDesc::basetype_merge(TypeDesc::FLOAT, TypeDesc::INT32);
        assert_eq!(merged, BaseType::Float);

        // Works with complex types (only basetype considered)
        let merged = TypeDesc::basetype_merge(TypeDesc::color(), TypeDesc::DOUBLE);
        assert_eq!(merged, BaseType::Double);
    }
}
