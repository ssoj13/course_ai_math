//! Nuke VF 3D LUT format parser.
//!
//! Format used by Foundry Nuke.
//!
//! # Format
//!
//! ```text
//! #Inventor V2.1 ascii
//! grid_size 17 17 17
//! global_transform 1 0 0 0  0 1 0 0  0 0 1 0  0 0 0 1
//! data
//! 0.0 0.0 0.0
//! ...
//! ```
//!
//! - Starts with `#Inventor V2.1 ascii` header
//! - `grid_size` must be uniform (X = Y = Z)
//! - Optional `global_transform` 4x4 matrix (pre-scaled by LUT size)
//! - Blue coordinate changes fastest

use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use crate::error::{LutError, LutResult};
use crate::lut3d::Lut3D;

/// Nuke VF file with optional pre-matrix.
#[derive(Debug, Clone)]
pub struct VfFile {
    /// Pre-transform matrix (4x4, row-major).
    pub matrix: Option<[f64; 16]>,
    /// 3D LUT data.
    pub lut: Lut3D,
}

/// Parses a Nuke VF file from a reader.
pub fn parse_vf<R: Read>(reader: R) -> LutResult<VfFile> {
    let reader = BufReader::new(reader);
    let mut lines = reader.lines();

    // Check header
    let first_line = lines
        .next()
        .ok_or_else(|| LutError::ParseError("empty file".into()))??;

    if !first_line.to_lowercase().starts_with("#inventor") {
        return Err(LutError::ParseError(
            "expected '#Inventor V2.1 ascii' header".into(),
        ));
    }

    let mut size = [0usize; 3];
    let mut matrix: Option<[f64; 16]> = None;
    let mut raw_data: Vec<[f32; 3]> = Vec::new();
    let mut in_data = false;

    for (line_num, line) in lines.enumerate() {
        let line = line?;
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let lower = trimmed.to_lowercase();
        let parts: Vec<&str> = trimmed.split_whitespace().collect();

        if !in_data {
            if lower.starts_with("grid_size") {
                if parts.len() != 4 {
                    return Err(LutError::ParseError(format!(
                        "grid_size requires 3 values at line {}",
                        line_num + 2
                    )));
                }
                size[0] = parts[1].parse().map_err(|_| {
                    LutError::ParseError(format!("invalid grid_size at line {}", line_num + 2))
                })?;
                size[1] = parts[2].parse().map_err(|_| {
                    LutError::ParseError(format!("invalid grid_size at line {}", line_num + 2))
                })?;
                size[2] = parts[3].parse().map_err(|_| {
                    LutError::ParseError(format!("invalid grid_size at line {}", line_num + 2))
                })?;

                if size[0] != size[1] || size[0] != size[2] {
                    return Err(LutError::ParseError(
                        "only uniform grid sizes supported".into(),
                    ));
                }

                // FileFormatVF.cpp: the grid size must be in [2, Max3DLUTLength].
                raw_data.reserve(crate::limits::check_3d_size("VF", size[0])?);
                continue;
            }

            if lower.starts_with("global_transform") {
                if parts.len() != 17 {
                    return Err(LutError::ParseError(format!(
                        "global_transform requires 16 values at line {}",
                        line_num + 2
                    )));
                }
                let mut m = [0.0f64; 16];
                for (i, p) in parts[1..17].iter().enumerate() {
                    m[i] = p.parse().map_err(|_| {
                        LutError::ParseError(format!(
                            "invalid global_transform at line {}",
                            line_num + 2
                        ))
                    })?;
                }

                // Store raw matrix - will unscale after parsing when size is known
                matrix = Some(m);
                continue;
            }

            if lower == "data" {
                in_data = true;
                continue;
            }
        } else {
            // Parse data triplets
            if parts.len() == 3 {
                let r: f32 = parts[0].parse().map_err(|_| {
                    LutError::ParseError(format!("invalid R value at line {}", line_num + 2))
                })?;
                let g: f32 = parts[1].parse().map_err(|_| {
                    LutError::ParseError(format!("invalid G value at line {}", line_num + 2))
                })?;
                let b: f32 = parts[2].parse().map_err(|_| {
                    LutError::ParseError(format!("invalid B value at line {}", line_num + 2))
                })?;

                raw_data.push([r, g, b]);
            }
        }
    }

    if size[0] == 0 {
        return Err(LutError::ParseError("missing grid_size".into()));
    }

    let expected = size[0] * size[1] * size[2];
    if raw_data.len() != expected {
        return Err(LutError::ParseError(format!(
            "expected {} entries, got {}",
            expected,
            raw_data.len()
        )));
    }

    // VF data is read sequentially and stored in Blue-major format
    // (same as our internal format: B + G*size + R*size²)
    let lut_size = size[0];

    // Unscale matrix by LUT size now that we know the final size (OCIO behavior)
    // This ensures parsing is order-independent (global_transform can appear before grid_size)
    let matrix = matrix.map(|mut m| {
        let s0 = size[0] as f64;
        let s1 = size[1] as f64;
        let s2 = size[2] as f64;
        for i in 0..4 {
            m[4 * i] *= s0;
            m[4 * i + 1] *= s1;
            m[4 * i + 2] *= s2;
        }
        m
    });

    Ok(VfFile {
        matrix,
        lut: Lut3D::from_data(raw_data, lut_size)?,
    })
}

/// Reads a Nuke VF file from disk.
pub fn read_vf<P: AsRef<Path>>(path: P) -> LutResult<VfFile> {
    let file = std::fs::File::open(path.as_ref())?;
    parse_vf(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_vf() {
        let data = r#"#Inventor V2.1 ascii
grid_size 2 2 2
data
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
"#;
        let vf = parse_vf(data.as_bytes()).unwrap();
        assert_eq!(vf.lut.size, 2);
        assert!(vf.matrix.is_none());
        // No reordering for VF (already blue-fastest in the file, per FileFormatVF.cpp:261 "LUT
        // in file is blue fastest."): interior entry 1 must be the file's 2nd data row exactly.
        assert_eq!(vf.lut.data[1], [1.0, 0.0, 0.0]);
    }

    #[test]
    fn parse_with_matrix() {
        let data = r#"#Inventor V2.1 ascii
grid_size 2 2 2
global_transform 0.5 0 0 0  0 0.5 0 0  0 0 0.5 0  0 0 0 1
data
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
"#;
        let vf = parse_vf(data.as_bytes()).unwrap();
        assert!(vf.matrix.is_some());
        let m = vf.matrix.unwrap();
        // 0.5 * 2 = 1.0 (unscaled)
        assert!((m[0] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn error_missing_header() {
        let data = "grid_size 2 2 2\ndata\n0 0 0\n";
        assert!(parse_vf(data.as_bytes()).is_err());
    }

    #[test]
    fn error_non_uniform() {
        let data = "#Inventor V2.1 ascii\ngrid_size 2 3 4\n";
        assert!(parse_vf(data.as_bytes()).is_err());
    }
}
