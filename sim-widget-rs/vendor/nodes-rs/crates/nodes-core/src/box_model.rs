//! Node parameter container — re-exported from the standalone `box-rs` crate.
//!
//! Each [`crate::node::NodeInstance`] owns a [`RustBox`] for keyed parameters (used with [`crate::address`]).

pub use box_rs::{
    ArrayDiffPolicy, ArrayMergePolicy, ArraySubtractPolicy, BoxConfig, BoxDuplicatesPolicy,
    BoxError, BoxList, BoxMap, BoxMut, BoxRef, BoxValue, Change, ChangeKind,
    DEFAULT_BOX_SAFE_PREFIX, DType, DiffError, DiffOptions, DiffReport, MergeError, MergeMode,
    MergeOptions, MissingKeyPolicy, NullMergePolicy, ObjectDiffPolicy, ObjectMergePolicy,
    PathFormat, PathKey, RecastRule, RustBox, ScalarComparePolicy, ScalarConflictPolicy, Segment,
    SubtractError, SubtractMode, SubtractOptions, TypeConflictPolicy, ValueKind, ZipPadPolicy,
    ZipTailPolicy,
};
