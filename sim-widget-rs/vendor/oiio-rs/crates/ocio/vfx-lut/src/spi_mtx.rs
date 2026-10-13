//! Sony Pictures Imageworks Matrix format (.spimtx).
//!
//! A simple text format storing a 3x3 color matrix with RGB offsets.
//! Used by OCIO and various SPI color pipelines.
//!
//! # Format
//!
//! 12 whitespace-separated float values:
//! ```text
//! m00 m01 m02 offset_r
//! m10 m11 m12 offset_g
//! m20 m21 m22 offset_b
//! ```
//!
//! The offset values are stored as 16-bit integers (0-65535 range)
//! and are normalized to `[0, 1]` on load.
//!
//! # Example
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use vfx_lut::spi_mtx::{read_spimtx, SpiMatrix};
//!
//! let mtx = read_spimtx("colorspace.spimtx")?;
//! let rgb_out = mtx.apply([0.5, 0.3, 0.2]);
//! # Ok(())
//! # }
//! ```

use crate::{LutError, LutResult};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

/// Offset normalization factor (16-bit integer range).
const OFFSET_SCALE: f64 = 65535.0;

/// A 3x3 color matrix with RGB offset.
///
/// Applies transform: `out = matrix * in + offset`
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpiMatrix {
    /// 3x3 matrix in row-major order `[row][col]`
    pub matrix: [[f64; 3]; 3],
    /// RGB offset (normalized to `[0, 1]`)
    pub offset: [f64; 3],
}

impl SpiMatrix {
    /// Create an identity matrix (no transformation).
    pub fn identity() -> Self {
        Self {
            matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            offset: [0.0, 0.0, 0.0],
        }
    }

    /// Create from matrix and offset.
    pub fn new(matrix: [[f64; 3]; 3], offset: [f64; 3]) -> Self {
        Self { matrix, offset }
    }

    /// Create from 12-element array (OCIO layout).
    ///
    /// Layout: [m00, m01, m02, off_r, m10, m11, m12, off_g, m20, m21, m22, off_b]
    pub fn from_array(arr: [f64; 12]) -> Self {
        Self {
            matrix: [
                [arr[0], arr[1], arr[2]],
                [arr[4], arr[5], arr[6]],
                [arr[8], arr[9], arr[10]],
            ],
            offset: [
                arr[3] / OFFSET_SCALE,
                arr[7] / OFFSET_SCALE,
                arr[11] / OFFSET_SCALE,
            ],
        }
    }

    /// Convert to 12-element array (OCIO layout).
    pub fn to_array(&self) -> [f64; 12] {
        [
            self.matrix[0][0],
            self.matrix[0][1],
            self.matrix[0][2],
            self.offset[0] * OFFSET_SCALE,
            self.matrix[1][0],
            self.matrix[1][1],
            self.matrix[1][2],
            self.offset[1] * OFFSET_SCALE,
            self.matrix[2][0],
            self.matrix[2][1],
            self.matrix[2][2],
            self.offset[2] * OFFSET_SCALE,
        ]
    }

    /// Apply matrix transform to RGB.
    ///
    /// `out = matrix * in + offset`
    #[inline]
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        let m = &self.matrix;
        let r = rgb[0] as f64;
        let g = rgb[1] as f64;
        let b = rgb[2] as f64;

        [
            (m[0][0] * r + m[0][1] * g + m[0][2] * b + self.offset[0]) as f32,
            (m[1][0] * r + m[1][1] * g + m[1][2] * b + self.offset[1]) as f32,
            (m[2][0] * r + m[2][1] * g + m[2][2] * b + self.offset[2]) as f32,
        ]
    }

    /// Apply matrix transform to RGB (f64 precision).
    #[inline]
    pub fn apply_f64(&self, rgb: [f64; 3]) -> [f64; 3] {
        let m = &self.matrix;
        [
            m[0][0] * rgb[0] + m[0][1] * rgb[1] + m[0][2] * rgb[2] + self.offset[0],
            m[1][0] * rgb[0] + m[1][1] * rgb[1] + m[1][2] * rgb[2] + self.offset[1],
            m[2][0] * rgb[0] + m[2][1] * rgb[1] + m[2][2] * rgb[2] + self.offset[2],
        ]
    }

    /// Compute inverse matrix (if invertible).
    ///
    /// Returns None if matrix is singular.
    pub fn inverse(&self) -> Option<Self> {
        let m = &self.matrix;

        // Compute determinant
        let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);

        if det.abs() < 1e-12 {
            return None;
        }

        let inv_det = 1.0 / det;

        // Adjugate matrix * 1/det
        let inv_m = [
            [
                (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv_det,
                (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv_det,
                (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv_det,
            ],
            [
                (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv_det,
                (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv_det,
                (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv_det,
            ],
            [
                (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv_det,
                (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv_det,
                (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv_det,
            ],
        ];

        // Inverse offset: -inv_m * offset
        let inv_off = [
            -(inv_m[0][0] * self.offset[0]
                + inv_m[0][1] * self.offset[1]
                + inv_m[0][2] * self.offset[2]),
            -(inv_m[1][0] * self.offset[0]
                + inv_m[1][1] * self.offset[1]
                + inv_m[1][2] * self.offset[2]),
            -(inv_m[2][0] * self.offset[0]
                + inv_m[2][1] * self.offset[1]
                + inv_m[2][2] * self.offset[2]),
        ];

        Some(Self {
            matrix: inv_m,
            offset: inv_off,
        })
    }

    /// Compose two matrices: self * other
    pub fn compose(&self, other: &SpiMatrix) -> Self {
        let a = &self.matrix;
        let b = &other.matrix;

        // Matrix multiplication
        let matrix = [
            [
                a[0][0] * b[0][0] + a[0][1] * b[1][0] + a[0][2] * b[2][0],
                a[0][0] * b[0][1] + a[0][1] * b[1][1] + a[0][2] * b[2][1],
                a[0][0] * b[0][2] + a[0][1] * b[1][2] + a[0][2] * b[2][2],
            ],
            [
                a[1][0] * b[0][0] + a[1][1] * b[1][0] + a[1][2] * b[2][0],
                a[1][0] * b[0][1] + a[1][1] * b[1][1] + a[1][2] * b[2][1],
                a[1][0] * b[0][2] + a[1][1] * b[1][2] + a[1][2] * b[2][2],
            ],
            [
                a[2][0] * b[0][0] + a[2][1] * b[1][0] + a[2][2] * b[2][0],
                a[2][0] * b[0][1] + a[2][1] * b[1][1] + a[2][2] * b[2][1],
                a[2][0] * b[0][2] + a[2][1] * b[1][2] + a[2][2] * b[2][2],
            ],
        ];

        // Combined offset: self.offset + self.matrix * other.offset
        let offset = [
            self.offset[0]
                + a[0][0] * other.offset[0]
                + a[0][1] * other.offset[1]
                + a[0][2] * other.offset[2],
            self.offset[1]
                + a[1][0] * other.offset[0]
                + a[1][1] * other.offset[1]
                + a[1][2] * other.offset[2],
            self.offset[2]
                + a[2][0] * other.offset[0]
                + a[2][1] * other.offset[1]
                + a[2][2] * other.offset[2],
        ];

        Self { matrix, offset }
    }
}

impl Default for SpiMatrix {
    fn default() -> Self {
        Self::identity()
    }
}

/// Read a .spimtx file.
pub fn read_spimtx<P: AsRef<Path>>(path: P) -> LutResult<SpiMatrix> {
    let file = File::open(path.as_ref())?;
    let reader = BufReader::new(file);
    parse_spimtx(reader)
}

/// Parse a .spimtx from reader.
pub fn parse_spimtx<R: Read>(mut reader: R) -> LutResult<SpiMatrix> {
    let mut content = String::new();
    reader.read_to_string(&mut content)?;

    // Split by whitespace, filter comments, strictly parse floats (OCIO behavior)
    let tokens: Vec<&str> = content
        .lines()
        .filter(|line| !line.trim().starts_with('#'))
        .flat_map(|line| line.split_whitespace())
        .collect();

    let mut values: Vec<f64> = Vec::with_capacity(12);
    for token in &tokens {
        match token.parse::<f64>() {
            Ok(v) => values.push(v),
            Err(_) => {
                return Err(LutError::ParseError(format!(
                    "invalid float value in spimtx: '{}'",
                    token
                )));
            }
        }
    }

    if values.len() != 12 {
        return Err(LutError::ParseError(format!(
            "spimtx requires 12 values, found {}",
            values.len()
        )));
    }

    let arr: [f64; 12] = values.try_into().unwrap();
    Ok(SpiMatrix::from_array(arr))
}

/// Write a .spimtx file.
pub fn write_spimtx<P: AsRef<Path>>(path: P, matrix: &SpiMatrix) -> LutResult<()> {
    let file = File::create(path.as_ref())?;
    let writer = BufWriter::new(file);
    write_spimtx_to(writer, matrix)
}

/// Write a .spimtx to any writer.
pub fn write_spimtx_to<W: Write>(mut writer: W, matrix: &SpiMatrix) -> LutResult<()> {
    let arr = matrix.to_array();

    // Write in 3 rows of 4 values
    writeln!(
        writer,
        "{:.10} {:.10} {:.10} {:.10}",
        arr[0], arr[1], arr[2], arr[3]
    )?;
    writeln!(
        writer,
        "{:.10} {:.10} {:.10} {:.10}",
        arr[4], arr[5], arr[6], arr[7]
    )?;
    writeln!(
        writer,
        "{:.10} {:.10} {:.10} {:.10}",
        arr[8], arr[9], arr[10], arr[11]
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_identity() {
        let mtx = SpiMatrix::identity();
        let rgb = mtx.apply([0.5, 0.3, 0.2]);

        assert!((rgb[0] - 0.5).abs() < 1e-6);
        assert!((rgb[1] - 0.3).abs() < 1e-6);
        assert!((rgb[2] - 0.2).abs() < 1e-6);
    }

    #[test]
    fn test_scale() {
        let mtx = SpiMatrix::new(
            [[2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 2.0]],
            [0.0, 0.0, 0.0],
        );
        let rgb = mtx.apply([0.25, 0.25, 0.25]);

        assert!((rgb[0] - 0.5).abs() < 1e-6);
        assert!((rgb[1] - 0.5).abs() < 1e-6);
        assert!((rgb[2] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_offset() {
        let mtx = SpiMatrix::new(
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            [0.1, 0.2, 0.3],
        );
        let rgb = mtx.apply([0.0, 0.0, 0.0]);

        assert!((rgb[0] - 0.1).abs() < 1e-6);
        assert!((rgb[1] - 0.2).abs() < 1e-6);
        assert!((rgb[2] - 0.3).abs() < 1e-6);
    }

    #[test]
    fn test_inverse() {
        let mtx = SpiMatrix::new(
            [[2.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 4.0]],
            [0.1, 0.2, 0.3],
        );

        let inv = mtx.inverse().expect("should be invertible");

        // Apply forward then inverse
        let rgb = [0.5, 0.3, 0.2];
        let fwd = mtx.apply_f64([rgb[0], rgb[1], rgb[2]]);
        let back = inv.apply_f64(fwd);

        assert!((back[0] - rgb[0]).abs() < 1e-10);
        assert!((back[1] - rgb[1]).abs() < 1e-10);
        assert!((back[2] - rgb[2]).abs() < 1e-10);
    }

    #[test]
    fn test_compose() {
        let a = SpiMatrix::new(
            [[2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 2.0]],
            [0.0, 0.0, 0.0],
        );
        let b = SpiMatrix::new(
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            [0.1, 0.1, 0.1],
        );

        // a.compose(b) = apply b first, then a
        // b: add 0.1, a: scale by 2
        // So: (0.25 + 0.1) * 2 = 0.7
        let c = a.compose(&b);
        let rgb = c.apply([0.25, 0.25, 0.25]);

        assert!((rgb[0] - 0.7).abs() < 1e-6);
    }

    #[test]
    fn test_parse_spimtx() {
        // Every one of the 9 matrix slots gets a distinct value, so a transposed row/column read
        // (or a row/offset-column off-by-one) would land the wrong number in the wrong slot --
        // an identity matrix's diagonal alone cannot catch that.
        let content = "1.0 2.0 3.0 0.0\n4.0 5.0 6.0 0.0\n7.0 8.0 9.0 0.0";
        let mtx = parse_spimtx(Cursor::new(content)).expect("parse failed");

        let expected = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]];
        for (i, (row, want_row)) in mtx.matrix.iter().zip(&expected).enumerate() {
            for (j, (&got, &want)) in row.iter().zip(want_row).enumerate() {
                assert!(
                    (got - want).abs() < 1e-10,
                    "matrix[{i}][{j}]: got {got}, expected {want}"
                );
            }
        }
        assert_eq!(mtx.offset, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn test_parse_spimtx_with_offset() {
        // Matrix with offset (stored as 16-bit int)
        // offset[0] = 6553.5 / 65535 = 0.1
        let content = "1.0 0.0 0.0 6553.5\n0.0 1.0 0.0 13107.0\n0.0 0.0 1.0 19660.5";
        let mtx = parse_spimtx(Cursor::new(content)).expect("parse failed");

        assert!((mtx.offset[0] - 0.1).abs() < 1e-6);
        assert!((mtx.offset[1] - 0.2).abs() < 1e-6);
        assert!((mtx.offset[2] - 0.3).abs() < 1e-6);
    }

    #[test]
    fn test_roundtrip() {
        let mtx = SpiMatrix::new(
            [[1.1, 0.2, 0.0], [0.0, 0.9, 0.1], [0.05, 0.0, 0.95]],
            [0.01, 0.02, 0.03],
        );

        let mut buf = Vec::new();
        write_spimtx_to(&mut buf, &mtx).expect("write failed");

        let parsed = parse_spimtx(Cursor::new(buf)).expect("parse failed");

        // Check matrix
        for i in 0..3 {
            for j in 0..3 {
                assert!(
                    (parsed.matrix[i][j] - mtx.matrix[i][j]).abs() < 1e-6,
                    "matrix[{}][{}] mismatch",
                    i,
                    j
                );
            }
        }

        // Check offset (lower precision due to 16-bit quantization)
        for i in 0..3 {
            assert!(
                (parsed.offset[i] - mtx.offset[i]).abs() < 1e-4,
                "offset[{}] mismatch",
                i
            );
        }
    }

    #[test]
    fn test_from_array() {
        // Distinct values in every slot (matrix AND offsets, at OCIO's 16-bit offset scale) so a
        // transposed row/column or an offset placed in the wrong output channel is caught -- an
        // identity round-trip cannot distinguish `arr[3]` (row 0's offset) from `arr[7]`/`arr[11]`.
        let arr = [
            1.0, 2.0, 3.0, 6553.5, // row 0, offset_r = 0.1 * 65535
            4.0, 5.0, 6.0, 13107.0, // row 1, offset_g = 0.2 * 65535
            7.0, 8.0, 9.0, 19660.5, // row 2, offset_b = 0.3 * 65535
        ];
        let mtx = SpiMatrix::from_array(arr);

        assert_eq!(
            mtx.matrix,
            [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]]
        );
        assert!((mtx.offset[0] - 0.1).abs() < 1e-6);
        assert!((mtx.offset[1] - 0.2).abs() < 1e-6);
        assert!((mtx.offset[2] - 0.3).abs() < 1e-6);
    }

    #[test]
    fn test_to_array() {
        let mtx = SpiMatrix::new(
            [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]],
            [0.1, 0.2, 0.3],
        );
        let arr = mtx.to_array();

        let expected = [
            1.0, 2.0, 3.0, 6553.5, 4.0, 5.0, 6.0, 13107.0, 7.0, 8.0, 9.0, 19660.5,
        ];
        for i in 0..12 {
            assert!(
                (arr[i] - expected[i]).abs() < 1e-4,
                "arr[{i}]: got {}, expected {}",
                arr[i],
                expected[i]
            );
        }
    }
}
