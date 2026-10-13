//! Layout-tagged matrices: row-major vs column-major encoded in the *type*.
//!
//! # Why this exists
//!
//! Three independent transposition bugs shipped in this workspace because the storage
//! convention of a `[f32; 9]` / `[f32; 16]` lived only in a comment next to it:
//!
//! - `vfx-ocio` `builtin_transforms::bradford_adapt` fed the Bradford **rows** into
//!   `glam::DMat3::from_cols_array` (which wants **columns**), silently transposing the
//!   cone-response matrix. The neighbouring `cat02_adapt` did it correctly *and carried a
//!   comment warning about exactly this trap* — proving that comments are read by whoever
//!   writes the next function, and not by whoever copies it.
//! - `vfx-compute`'s wgpu backend uploads a row-major `[f32; 16]` straight into a
//!   `mat4x4<f32>` uniform, which WGSL reads column-major.
//! - The `vfx-ocio` GLSL and HLSL emitters disagree with the (correct) WGSL and Metal ones.
//!
//! Every one of these is invisible on the neutral axis — a transposed chromatic-adaptation
//! matrix still maps white to white exactly — so identity/grey tests pass and the defect
//! only shows on saturated colour. The convention therefore cannot live in prose; it has to
//! be something the compiler checks.
//!
//! # How to use it
//!
//! The layout is a type parameter, and the constructors are split by layout:
//! [`Mat3::from_rows`] exists only on `Mat3<RowMajor>`, [`Mat3::from_cols`] only on
//! `Mat3<ColMajor>`. Passing the wrong one to a function is a compile error, and the fix is
//! to spell the transposing conversion out loud — [`Mat3::to_col_major`] /
//! [`Mat3::to_row_major`].
//!
//! ```
//! use vfx_math::layout::{ColMajor, Mat3d, RowMajor};
//!
//! // Bradford cone-response matrix as published (rows), verbatim from the reference.
//! let bradford = Mat3d::<RowMajor>::from_rows([
//!     [0.8951, 0.2664, -0.1614],
//!     [-0.7502, 1.7135, 0.0367],
//!     [0.0389, -0.0685, 1.0296],
//! ]);
//!
//! // glam is column-major. The conversion is explicit and cannot be forgotten, because
//! // `to_glam` on a row-major matrix is the only route in and it transposes for you.
//! let g: glam::DMat3 = bradford.to_glam();
//! assert_eq!(g.col(0).to_array(), [0.8951, -0.7502, 0.0389]);
//! ```
//!
//! # Which layout does a target want?
//!
//! These types carry no knowledge of shading languages — the caller picks. The table below
//! is the reference the emitters and GPU backends should be migrated against:
//!
//! | Target                                            | Wants        |
//! |---------------------------------------------------|--------------|
//! | `glam` (`from_cols_array`, `to_cols_array`)        | [`ColMajor`] |
//! | WGSL / GLSL / MSL `matNxN` uniform upload          | [`ColMajor`] |
//! | GLSL `mat3(...)` / `mat4(...)` constructor args    | [`ColMajor`] |
//! | HLSL `float3x3(...)` / `float4x4(...)` ctor args   | [`RowMajor`] |
//! | OCIO reference sources, published vendor matrices  | [`RowMajor`] |
//! | [`crate::Mat3`], this crate's public matrix type   | [`RowMajor`] |
//!
//! A migrated call site should take a layout-tagged matrix in its *signature* — that is what
//! makes the wrong usage fail to compile. Taking a bare `[f32; 16]` and converting inside
//! restores the original hazard.
//!
//! # Used by
//!
//! `vfx-ocio` (builtin transform matrix builders, GPU emitters), `vfx-compute` (matrix op
//! upload across the wgpu/CUDA/CPU backends), `vfx-warp`.

use std::marker::PhantomData;

mod sealed {
    /// Prevents downstream crates from inventing a third "layout", which would defeat the
    /// point: the set of conventions is closed at two.
    pub trait Sealed {}
}

/// Storage convention of a flat matrix array. Implemented only by [`RowMajor`] and
/// [`ColMajor`]; the trait is sealed.
pub trait Layout: sealed::Sealed + Copy + 'static {
    /// Human-readable name, for diagnostics and emitted shader comments.
    const NAME: &'static str;
}

/// Row-major storage: element `(row, col)` of an `N x N` matrix lives at index `row * N + col`.
///
/// This is how matrices are written in papers, in the OCIO reference sources, and in vendor
/// specifications, so it is the layout literals should normally be *entered* in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RowMajor;

/// Column-major storage: element `(row, col)` lives at index `col * N + row`.
///
/// This is what `glam` and the GPU shading languages consume. Reaching it from [`RowMajor`]
/// always costs a transpose, and that transpose must be spelled out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ColMajor;

impl sealed::Sealed for RowMajor {}
impl sealed::Sealed for ColMajor {}

impl Layout for RowMajor {
    const NAME: &'static str = "row-major";
}

impl Layout for ColMajor {
    const NAME: &'static str = "column-major";
}

/// Generates one layout-tagged square-matrix type.
///
/// `$name` is the type, `$elem` the scalar, `$dim` the side length, `$len` = `$dim * $dim`
/// (const-generic arithmetic is not yet stable, so the length is passed explicitly), and
/// `$glam` the corresponding column-major `glam` type.
macro_rules! def_layout_mat {
    ($name:ident, $elem:ty, $dim:expr, $len:expr, $glam:ty, $doc:expr) => {
        #[doc = $doc]
        ///
        /// The element order of the backing array is defined by the layout parameter `L`;
        /// see the [module docs](self) for why that is a type and not a comment.
        #[repr(transparent)]
        pub struct $name<L: Layout> {
            elems: [$elem; $len],
            // `fn() -> L` rather than `L` so the auto traits and variance of the matrix do
            // not depend on the marker, and so the derives below need no bound on `L`.
            _layout: PhantomData<fn() -> L>,
        }

        // Hand-written rather than derived: `derive` would emit `L: Clone`/`L: PartialEq`
        // bounds that the marker types do not need to satisfy for the matrix to be usable.
        impl<L: Layout> Clone for $name<L> {
            fn clone(&self) -> Self {
                *self
            }
        }

        impl<L: Layout> Copy for $name<L> {}

        impl<L: Layout> PartialEq for $name<L> {
            fn eq(&self, other: &Self) -> bool {
                self.elems == other.elems
            }
        }

        impl<L: Layout> std::fmt::Debug for $name<L> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct(stringify!($name))
                    .field("layout", &L::NAME)
                    .field("elems", &self.elems)
                    .finish()
            }
        }

        impl<L: Layout> $name<L> {
            /// Side length of the matrix.
            pub const DIM: usize = $dim;

            /// Wrap a flat array **already known** to be in layout `L`.
            ///
            /// This is the one unchecked entry point — it asserts a fact about data that
            /// came from elsewhere (a file, a wire format, an existing `[f32; N]` API).
            /// When entering literals, prefer `from_rows` / `from_cols`, which make the
            /// claim visually verifiable.
            pub const fn from_array(elems: [$elem; $len]) -> Self {
                Self {
                    elems,
                    _layout: PhantomData,
                }
            }

            /// The backing array, in layout `L`.
            pub const fn to_array(self) -> [$elem; $len] {
                self.elems
            }

            /// Borrow the backing array, in layout `L` — for zero-copy GPU uploads.
            pub const fn as_array(&self) -> &[$elem; $len] {
                &self.elems
            }

            /// Reinterpret the storage under the opposite convention.
            ///
            /// This is *not* a conversion: the bytes are unchanged and the matrix it
            /// describes is the mathematical transpose. It exists for the rare site that
            /// deliberately wants the transpose (e.g. building an inverse rotation), and is
            /// named to make that intent unmistakable. To keep the same matrix while
            /// changing layout, use `to_col_major` / `to_row_major`, which move the elements.
            pub const fn reinterpret_transposed<M: Layout>(self) -> $name<M> {
                $name::from_array(self.elems)
            }
        }

        impl $name<RowMajor> {
            /// Build from rows, exactly as the matrix is printed in a paper or reference source.
            pub const fn from_rows(rows: [[$elem; $dim]; $dim]) -> Self {
                let mut elems = [0 as $elem; $len];
                let mut r = 0;
                while r < $dim {
                    let mut c = 0;
                    while c < $dim {
                        elems[r * $dim + c] = rows[r][c];
                        c += 1;
                    }
                    r += 1;
                }
                Self::from_array(elems)
            }

            /// The rows.
            pub const fn rows(self) -> [[$elem; $dim]; $dim] {
                let mut out = [[0 as $elem; $dim]; $dim];
                let mut r = 0;
                while r < $dim {
                    let mut c = 0;
                    while c < $dim {
                        out[r][c] = self.elems[r * $dim + c];
                        c += 1;
                    }
                    r += 1;
                }
                out
            }

            /// Transpose the storage into column-major, preserving the matrix.
            ///
            /// This is the explicit, named conversion that the three shipped transposition
            /// bugs were each missing.
            pub const fn to_col_major(self) -> $name<ColMajor> {
                let mut elems = [0 as $elem; $len];
                let mut r = 0;
                while r < $dim {
                    let mut c = 0;
                    while c < $dim {
                        elems[c * $dim + r] = self.elems[r * $dim + c];
                        c += 1;
                    }
                    r += 1;
                }
                $name::from_array(elems)
            }

            /// Convert to the `glam` type, transposing — `glam` stores columns.
            pub fn to_glam(self) -> $glam {
                <$glam>::from_cols_array(self.to_col_major().as_array())
            }

            /// Build from a `glam` value, transposing — `glam` stores columns.
            pub fn from_glam(m: $glam) -> Self {
                $name::<ColMajor>::from_array(m.to_cols_array()).to_row_major()
            }
        }

        impl $name<ColMajor> {
            /// Build from columns. Use this only when the source data really is column-wise;
            /// published matrices are almost always rows, so prefer
            #[doc = concat!("[`", stringify!($name), "::from_rows`] plus `to_col_major`.")]
            pub const fn from_cols(cols: [[$elem; $dim]; $dim]) -> Self {
                let mut elems = [0 as $elem; $len];
                let mut c = 0;
                while c < $dim {
                    let mut r = 0;
                    while r < $dim {
                        elems[c * $dim + r] = cols[c][r];
                        r += 1;
                    }
                    c += 1;
                }
                Self::from_array(elems)
            }

            /// The columns.
            pub const fn cols(self) -> [[$elem; $dim]; $dim] {
                let mut out = [[0 as $elem; $dim]; $dim];
                let mut c = 0;
                while c < $dim {
                    let mut r = 0;
                    while r < $dim {
                        out[c][r] = self.elems[c * $dim + r];
                        r += 1;
                    }
                    c += 1;
                }
                out
            }

            /// Transpose the storage into row-major, preserving the matrix.
            pub const fn to_row_major(self) -> $name<RowMajor> {
                let mut elems = [0 as $elem; $len];
                let mut c = 0;
                while c < $dim {
                    let mut r = 0;
                    while r < $dim {
                        elems[r * $dim + c] = self.elems[c * $dim + r];
                        r += 1;
                    }
                    c += 1;
                }
                $name::from_array(elems)
            }

            /// Convert to the `glam` type. No transpose: both are column-major.
            pub fn to_glam(self) -> $glam {
                <$glam>::from_cols_array(self.as_array())
            }

            /// Build from a `glam` value. No transpose: both are column-major.
            pub fn from_glam(m: $glam) -> Self {
                Self::from_array(m.to_cols_array())
            }
        }
    };
}

def_layout_mat!(
    Mat3,
    f32,
    3,
    9,
    glam::Mat3,
    "A layout-tagged 3x3 `f32` matrix — RGB/XYZ colour transforms."
);
def_layout_mat!(
    Mat3d,
    f64,
    3,
    9,
    glam::DMat3,
    "A layout-tagged 3x3 `f64` matrix — chromatic adaptation, where f32 rounding in the \
     cone-response inverse is visible."
);
def_layout_mat!(
    Mat4,
    f32,
    4,
    16,
    glam::Mat4,
    "A layout-tagged 4x4 `f32` matrix — the `[f32; 16]` colour matrix carried through \
     `vfx-compute` and the `vfx-ocio` builtin transform tables."
);

impl<L: Layout> Mat4<L> {
    /// The upper-left 3x3 block — the RGB part of a 4x4 colour matrix, dropping the
    /// offset row/column.
    ///
    /// Layout-preserving and layout-agnostic: in a 4x4 the upper-left block occupies index
    /// set `{0,1,2,4,5,6,8,9,10}` under *both* conventions (`row*4+col` and `col*4+row`
    /// coincide on it), and reading those in order yields the 3x3 in the same convention.
    pub const fn to_mat3(self) -> Mat3<L> {
        let e = self.elems;
        Mat3::from_array([e[0], e[1], e[2], e[4], e[5], e[6], e[8], e[9], e[10]])
    }
}

impl<L: Layout> Mat3<L> {
    /// Embed as the upper-left block of a 4x4, with an identity fourth row and column.
    ///
    /// The inverse of [`Mat4::to_mat3`]; layout-preserving for the same reason.
    pub const fn to_mat4(self) -> Mat4<L> {
        let e = self.elems;
        Mat4::from_array([
            e[0], e[1], e[2], 0.0, e[3], e[4], e[5], 0.0, e[6], e[7], e[8], 0.0, 0.0, 0.0, 0.0, 1.0,
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact defect from `builtin_transforms.rs:2158`: the published Bradford rows fed to
    /// `glam::DMat3::from_cols_array` yield the transpose. `to_glam` must produce the correct
    /// matrix, and must *not* equal the buggy construction.
    #[test]
    fn row_major_to_glam_matches_ocio_bradford() {
        const ROWS: [[f64; 3]; 3] = [
            [0.8951, 0.2664, -0.1614],
            [-0.7502, 1.7135, 0.0367],
            [0.0389, -0.0685, 1.0296],
        ];
        let flat_rows = [
            0.8951, 0.2664, -0.1614, -0.7502, 1.7135, 0.0367, 0.0389, -0.0685, 1.0296,
        ];

        let correct = Mat3d::<RowMajor>::from_rows(ROWS).to_glam();
        let buggy = glam::DMat3::from_cols_array(&flat_rows);

        assert_ne!(
            correct, buggy,
            "the bug must be reproducible for the test to mean anything"
        );
        assert_eq!(correct, buggy.transpose());
        assert_eq!(correct.col(0).to_array(), [0.8951, -0.7502, 0.0389]);
        assert_eq!(correct.row(0).to_array(), [0.8951, 0.2664, -0.1614]);
    }

    /// A transposed cone-response matrix still maps white to white exactly, which is why
    /// grey-based tests never caught this. Pin the property so nobody adds another one.
    #[test]
    fn neutral_axis_is_blind_to_transposition() {
        let rows = [
            [0.8951, 0.2664, -0.1614],
            [-0.7502, 1.7135, 0.0367],
            [0.0389, -0.0685, 1.0296],
        ];
        let m = Mat3d::<RowMajor>::from_rows(rows).to_glam();
        let mt = m.transpose();
        let white = glam::DVec3::new(0.95047, 1.0, 1.08883);

        // Adaptation is M^-1 * diag(dst/src) * M; on the source white both agree exactly.
        let adapt = |c: glam::DMat3| {
            let s = c * white;
            let d = c * white;
            c.inverse() * glam::DMat3::from_diagonal(d / s) * c
        };
        let a = adapt(m) * white;
        let b = adapt(mt) * white;
        assert!(
            (a - b).abs().max_element() < 1e-12,
            "neutral axis cannot distinguish them"
        );

        // A saturated colour can, and by a wide margin.
        let sat = glam::DVec3::new(0.4, 0.2, 0.05);
        assert!((m * sat - mt * sat).abs().max_element() > 1e-3);
    }

    #[test]
    fn roundtrip_layout_conversions_preserve_the_matrix() {
        let m = Mat3::<RowMajor>::from_rows([[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]]);
        assert_eq!(m.to_col_major().to_row_major(), m);
        assert_eq!(
            m.to_col_major().to_array(),
            [1.0, 4.0, 7.0, 2.0, 5.0, 8.0, 3.0, 6.0, 9.0]
        );
        assert_eq!(
            m.to_col_major().cols(),
            [[1.0, 4.0, 7.0], [2.0, 5.0, 8.0], [3.0, 6.0, 9.0]]
        );
        assert_eq!(Mat3::<RowMajor>::from_glam(m.to_glam()), m);
    }

    #[test]
    fn col_major_glam_roundtrip_does_not_transpose() {
        let cols = Mat3::<ColMajor>::from_cols([[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]]);
        assert_eq!(cols.to_glam().to_cols_array(), *cols.as_array());
        assert_eq!(Mat3::<ColMajor>::from_glam(cols.to_glam()), cols);
    }

    /// The 3x3 <-> 4x4 index set claim in the rustdoc, checked under both conventions.
    #[test]
    fn mat3_mat4_embedding_is_layout_preserving() {
        let r = Mat3::<RowMajor>::from_rows([[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]]);
        assert_eq!(r.to_mat4().to_mat3(), r);
        assert_eq!(r.to_mat4().rows()[0], [1.0, 2.0, 3.0, 0.0]);
        assert_eq!(r.to_mat4().rows()[3], [0.0, 0.0, 0.0, 1.0]);

        let c = r.to_col_major();
        assert_eq!(c.to_mat4().to_mat3(), c);
        // Embedding then transposing must equal transposing then embedding.
        assert_eq!(r.to_mat4().to_col_major(), c.to_mat4());
    }

    #[test]
    fn mat4_upload_array_is_column_major_for_gpu() {
        // What a wgpu `mat4x4<f32>` uniform must receive: the columns, contiguously.
        let m = Mat4::<RowMajor>::from_rows([
            [1.0, 2.0, 3.0, 4.0],
            [5.0, 6.0, 7.0, 8.0],
            [9.0, 10.0, 11.0, 12.0],
            [13.0, 14.0, 15.0, 16.0],
        ]);
        assert_eq!(
            m.to_col_major().to_array(),
            [
                1.0, 5.0, 9.0, 13.0, 2.0, 6.0, 10.0, 14.0, 3.0, 7.0, 11.0, 15.0, 4.0, 8.0, 12.0,
                16.0
            ]
        );
    }

    #[test]
    fn reinterpret_transposed_keeps_bytes() {
        let m = Mat3::<RowMajor>::from_rows([[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]]);
        let same_bytes: Mat3<ColMajor> = m.reinterpret_transposed();
        assert_eq!(same_bytes.to_array(), m.to_array());
        // Same bytes read the other way round == the mathematical transpose.
        assert_eq!(same_bytes.cols(), m.rows());
    }
}
