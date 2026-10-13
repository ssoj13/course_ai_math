//! Iridas ITX 3D LUT format parser.
//!
//! Simple 3D LUT format used by Iridas/Adobe SpeedGrade.
//!
//! # Format
//!
//! ```text
//! LUT_3D_SIZE 17
//! # optional comment
//! 0.0 0.0 0.0
//! 1.0 0.0 0.0
//! ...
//! ```
//!
//! - Red coordinate changes fastest, then green, then blue
//! - Values are floating point RGB triplets
//! - Lines starting with `#` are comments

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;

use crate::error::{LutError, LutResult};
use crate::lut3d::Lut3D;

/// Parses an Iridas ITX file from a reader.
pub fn parse_itx<R: Read>(reader: R) -> LutResult<Lut3D> {
    let reader = BufReader::new(reader);
    let mut size = 0usize;
    let mut data: Vec<[f32; 3]> = Vec::new();
    let mut in_data = false;

    for (line_num, line) in reader.lines().enumerate() {
        let line = line?;
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let lower = trimmed.to_lowercase();

        // Parse LUT_3D_SIZE header
        if lower.starts_with("lut_3d_size") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() != 2 {
                return Err(LutError::ParseError(format!(
                    "malformed LUT_3D_SIZE at line {}",
                    line_num + 1
                )));
            }
            size = parts[1].parse().map_err(|_| {
                LutError::ParseError(format!("invalid size at line {}", line_num + 1))
            })?;
            // FileFormatIridasItx.cpp: the size must be in [2, Max3DLUTLength].
            data.reserve(crate::limits::check_3d_size("ITX", size)?);
            in_data = true;
            continue;
        }

        // Parse data triplets
        if in_data {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() != 3 {
                return Err(LutError::ParseError(format!(
                    "expected 3 values at line {}, got {}",
                    line_num + 1,
                    parts.len()
                )));
            }

            let r: f32 = parts[0].parse().map_err(|_| {
                LutError::ParseError(format!("invalid float at line {}", line_num + 1))
            })?;
            let g: f32 = parts[1].parse().map_err(|_| {
                LutError::ParseError(format!("invalid float at line {}", line_num + 1))
            })?;
            let b: f32 = parts[2].parse().map_err(|_| {
                LutError::ParseError(format!("invalid float at line {}", line_num + 1))
            })?;

            data.push([r, g, b]);
        }
    }

    if size == 0 {
        return Err(LutError::ParseError("no LUT_3D_SIZE found".into()));
    }

    let expected = size * size * size;
    if data.len() != expected {
        return Err(LutError::ParseError(format!(
            "expected {} entries, got {}",
            expected,
            data.len()
        )));
    }

    // ITX uses R-fastest order, but Lut3D expects B-fastest order
    // Reorder from file order (R-fastest) to memory order (B-fastest)
    // Reference: OCIO FileFormatIridasItx.cpp:221 calls setArrayFromRedFastestOrder()
    let mut reordered = vec![[0.0f32; 3]; data.len()];
    for b in 0..size {
        for g in 0..size {
            for r in 0..size {
                let file_idx = r + g * size + b * size * size; // R-fastest
                let mem_idx = b + g * size + r * size * size; // B-fastest
                reordered[mem_idx] = data[file_idx];
            }
        }
    }

    Lut3D::from_data(reordered, size)
}

/// Reads an Iridas ITX file from disk.
pub fn read_itx<P: AsRef<Path>>(path: P) -> LutResult<Lut3D> {
    let file = std::fs::File::open(path.as_ref())?;
    parse_itx(file)
}

/// Writes a 3D LUT to Iridas ITX format.
pub fn write_itx<W: Write>(writer: &mut W, lut: &Lut3D) -> LutResult<()> {
    writeln!(writer, "LUT_3D_SIZE {}", lut.size)?;

    // Write data in R-fastest order (file format) from B-fastest memory order
    // Inverse of the read reordering
    let size = lut.size;
    for b in 0..size {
        for g in 0..size {
            for r in 0..size {
                let mem_idx = b + g * size + r * size * size; // B-fastest (memory)
                let entry = lut.data[mem_idx];
                writeln!(writer, "{:.6} {:.6} {:.6}", entry[0], entry[1], entry[2])?;
            }
        }
    }

    Ok(())
}

/// Writes a 3D LUT to an Iridas ITX file.
pub fn write_itx_file<P: AsRef<Path>>(path: P, lut: &Lut3D) -> LutResult<()> {
    let mut file = std::fs::File::create(path.as_ref())?;
    write_itx(&mut file, lut)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_itx() {
        let data = r#"LUT_3D_SIZE 2
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
"#;
        let lut = parse_itx(data.as_bytes()).unwrap();
        assert_eq!(lut.size, 2);
        assert_eq!(lut.data.len(), 8);

        // Check corners
        assert_eq!(lut.data[0], [0.0, 0.0, 0.0]); // black
        assert_eq!(lut.data[7], [1.0, 1.0, 1.0]); // white
    }

    #[test]
    fn parse_with_comments() {
        let data = r#"# This is a comment
LUT_3D_SIZE 2
# Another comment
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
"#;
        let lut = parse_itx(data.as_bytes()).unwrap();
        assert_eq!(lut.size, 2);
    }

    #[test]
    fn roundtrip() {
        let original = Lut3D::identity(4);
        let mut buf = Vec::new();
        write_itx(&mut buf, &original).unwrap();

        let parsed = parse_itx(&buf[..]).unwrap();
        assert_eq!(parsed.size, 4);
        assert_eq!(parsed.data.len(), 64);

        // Check identity
        for (i, entry) in parsed.data.iter().enumerate() {
            let orig = original.data[i];
            assert!((entry[0] - orig[0]).abs() < 1e-5);
            assert!((entry[1] - orig[1]).abs() < 1e-5);
            assert!((entry[2] - orig[2]).abs() < 1e-5);
        }
    }

    #[test]
    fn error_missing_size() {
        let data = "0.0 0.0 0.0\n1.0 1.0 1.0\n";
        let result = parse_itx(data.as_bytes());
        assert!(result.is_err());
    }

    #[test]
    fn error_wrong_count() {
        let data = r#"LUT_3D_SIZE 2
0.0 0.0 0.0
1.0 0.0 0.0
"#;
        let result = parse_itx(data.as_bytes());
        assert!(result.is_err());
    }
    /// Test that demonstrates the ITX data reordering bug.
    /// ITX files use R-fastest order, but Lut3D expects B-fastest order.
    /// The parser currently does NOT reorder the data, causing incorrect results.
    #[test]
    fn test_itx_data_order_bug() {
        // ITX format: R-fastest order (R varies fastest, then G, then B)
        // For size=2, the file order should be:
        // Index 0: R=0,G=0,B=0 (black)   -> [0.0, 0.0, 0.0]
        // Index 1: R=1,G=0,B=0 (red)     -> [1.0, 0.0, 0.0]
        // Index 2: R=0,G=1,B=0 (green)   -> [0.0, 1.0, 0.0]
        // Index 3: R=1,G=1,B=0 (yellow)  -> [1.0, 1.0, 0.0]
        // Index 4: R=0,G=0,B=1 (blue)    -> [0.0, 0.0, 1.0]
        // Index 5: R=1,G=0,B=1 (magenta) -> [1.0, 0.0, 1.0]
        // Index 6: R=0,G=1,B=1 (cyan)    -> [0.0, 1.0, 1.0]
        // Index 7: R=1,G=1,B=1 (white)   -> [1.0, 1.0, 1.0]

        let data = r#"LUT_3D_SIZE 2
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
"#;

        let lut = parse_itx(data.as_bytes()).unwrap();
        assert_eq!(lut.size, 2);
        assert_eq!(lut.data.len(), 8);

        // Lut3D uses B-fastest order: index = B + dim*G + dim²*R
        // After CORRECT reordering, the memory layout should be:
        // lut.data[0]: B=0,G=0,R=0 -> [0.0, 0.0, 0.0] (black)
        // lut.data[1]: B=1,G=0,R=0 -> [0.0, 0.0, 1.0] (blue)   <-- KEY TEST!
        // lut.data[2]: B=0,G=1,R=0 -> [0.0, 1.0, 0.0] (green)
        // lut.data[3]: B=1,G=1,R=0 -> [0.0, 1.0, 1.0] (cyan)
        // lut.data[4]: B=0,G=0,R=1 -> [1.0, 0.0, 0.0] (red)
        // lut.data[5]: B=1,G=0,R=1 -> [1.0, 0.0, 1.0] (magenta)
        // lut.data[6]: B=0,G=1,R=1 -> [1.0, 1.0, 0.0] (yellow)
        // lut.data[7]: B=1,G=1,R=1 -> [1.0, 1.0, 1.0] (white)

        // Verify corners
        assert_eq!(
            lut.data[0],
            [0.0, 0.0, 0.0],
            "Index 0: black should be [0,0,0]"
        );
        assert_eq!(
            lut.data[7],
            [1.0, 1.0, 1.0],
            "Index 7: white should be [1,1,1]"
        );

        // THIS IS THE KEY TEST: lut.data[1] should be BLUE [0,0,1], not RED [1,0,0]
        // Currently fails because parser doesn't reorder the data
        assert_eq!(
            lut.data[1],
            [0.0, 0.0, 1.0],
            "Index 1: should be blue [0,0,1] in B-fastest order, but got {:?}",
            lut.data[1]
        );

        // Additional checks to verify the complete reordering
        assert_eq!(lut.data[2], [0.0, 1.0, 0.0], "Index 2: should be green");
        assert_eq!(lut.data[3], [0.0, 1.0, 1.0], "Index 3: should be cyan");
        assert_eq!(lut.data[4], [1.0, 0.0, 0.0], "Index 4: should be red");
        assert_eq!(lut.data[5], [1.0, 0.0, 1.0], "Index 5: should be magenta");
        assert_eq!(lut.data[6], [1.0, 1.0, 0.0], "Index 6: should be yellow");
    }
}
