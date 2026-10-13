//! Pandora MGA/M3D 3D LUT format parser.
//!
//! Format used by Pandora color grading systems.
//!
//! # Format
//!
//! ```text
//! channel 3d
//! in 4096
//! out 4096
//! format lut
//! values red green blue
//! 0 0 0 0
//! 1 0 0 4095
//! ...
//! ```
//!
//! No colon after the keyword (`FileFormatPandora.cpp::read()` matches `parts[0]` against the
//! bare keyword; the real fixtures, e.g. `pandora_3d.m3d`, never carry one).
//!
//! - First column is index (ignored)
//! - Blue coordinate changes fastest (blue-fastest order)
//! - Values are scaled by `1 / (out - 1)`, not by `out` itself
//!   (`FileFormatPandora.cpp`: `scale = 1.0f / ((float)outputBitDepthMaxValue - 1.0f)`)

use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use crate::error::{LutError, LutResult};
use crate::lut3d::Lut3D;

/// Parses a Pandora MGA/M3D file from a reader.
#[allow(unused_assignments)]
pub fn parse_mga<R: Read>(reader: R) -> LutResult<Lut3D> {
    let reader = BufReader::new(reader);

    let mut in_count = 0usize;
    let mut out_max = 0i32;
    let mut in_lut = false;
    let mut raw_data: Vec<[i32; 3]> = Vec::new();

    for (line_num, line) in reader.lines().enumerate() {
        let line = line?;
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let lower = trimmed.to_lowercase();
        let parts: Vec<&str> = trimmed.split_whitespace().collect();

        // Parse headers. Reference: `FileFormatPandora.cpp::read()` matches `parts[0]` against
        // the bare keyword ("channel", "in", "out", "format", "values") with NO trailing colon
        // -- the real format (and the `pandora_3d.m3d`/`pandora_mga` fixtures) never uses one
        // (COLOR bug: this parser used to require `"channel:"` etc., which is not what OCIO reads
        // or what any real Pandora file contains; every data/header line was silently skipped
        // because none of the `starts_with("*:")` checks ever matched, `in_lut` stayed false, and
        // the file failed later with a misleading "missing 'out'" error instead of parsing).
        if parts.is_empty() {
            continue;
        }
        let keyword = parts[0].to_lowercase();

        if keyword == "channel" {
            if !lower.contains("3d") {
                return Err(LutError::ParseError("only 3D LUTs supported".into()));
            }
            continue;
        }

        if keyword == "in" {
            if parts.len() >= 2 {
                in_count = parts[1].parse().map_err(|_| {
                    LutError::ParseError(format!("invalid 'in' value at line {}", line_num + 1))
                })?;
                // FileFormatPandora.cpp: 'in' must be a cube of an edge in [2, Max3DLUTLength]
                // (8 ..= 129^3), checked before it sizes an allocation.
                crate::limits::cube_edge_from_count("Pandora", in_count)?;
                raw_data.reserve(in_count);
            }
            continue;
        }

        if keyword == "out" {
            if parts.len() >= 2 {
                out_max = parts[1].parse().map_err(|_| {
                    LutError::ParseError(format!("invalid 'out' value at line {}", line_num + 1))
                })?;
            }
            continue;
        }

        if keyword == "format" {
            if !lower.contains("lut") {
                return Err(LutError::ParseError("only LUT format supported".into()));
            }
            continue;
        }

        if keyword == "values" {
            // Validate that order is "red green blue" (OCIO requirement)
            if parts.len() != 4
                || !parts[1].eq_ignore_ascii_case("red")
                || !parts[2].eq_ignore_ascii_case("green")
                || !parts[3].eq_ignore_ascii_case("blue")
            {
                return Err(LutError::ParseError(
                    "only RGB LUTs supported (values: red green blue)".into(),
                ));
            }
            in_lut = true;
            continue;
        }

        // Parse LUT data
        if in_lut {
            if parts.len() != 4 {
                return Err(LutError::ParseError(format!(
                    "expected 4 values at line {}, got {}",
                    line_num + 1,
                    parts.len()
                )));
            }

            // First value is index (ignored), then R, G, B
            let r: i32 = parts[1].parse().map_err(|_| {
                LutError::ParseError(format!("invalid R value at line {}", line_num + 1))
            })?;
            let g: i32 = parts[2].parse().map_err(|_| {
                LutError::ParseError(format!("invalid G value at line {}", line_num + 1))
            })?;
            let b: i32 = parts[3].parse().map_err(|_| {
                LutError::ParseError(format!("invalid B value at line {}", line_num + 1))
            })?;

            // FileFormatPandora.cpp: "Too many 3D LUT entries."
            let max = crate::limits::MAX_3D_LUT_LENGTH;
            if raw_data.len() >= max * max * max {
                return Err(LutError::ParseError(format!(
                    "too many 3D LUT entries at line {}",
                    line_num + 1
                )));
            }
            raw_data.push([r, g, b]);
        }
    }

    // The values are scaled by 1 / (out - 1): `out` 1 would divide by zero (OCIO does not check
    // it and produces inf).
    if out_max < 2 {
        return Err(LutError::ParseError(format!(
            "missing or invalid 'out' value {out_max} (must be at least 2)"
        )));
    }

    if raw_data.is_empty() {
        return Err(LutError::ParseError("no LUT data found".into()));
    }

    // Calculate LUT size from `in:` value if specified, otherwise from data count
    let count = raw_data.len();
    let size = if in_count > 0 {
        // Use declared 'in:' value to compute edge length (OCIO behavior)
        let edge = crate::limits::cube_edge_from_count("Pandora", in_count)?;
        // Validate data matches declared size
        if count != in_count {
            return Err(LutError::ParseError(format!(
                "data count {} does not match declared 'in: {}'",
                count, in_count
            )));
        }
        edge
    } else {
        // Fallback: infer from data count (OCIO requires 'in:'; this reader does not).
        crate::limits::cube_edge_from_count("Pandora", count)?
    };

    // Scale and convert to float
    // Pandora uses blue-fastest, same as our internal Blue-major format
    let scale = 1.0 / (out_max - 1) as f32;
    let mut data = vec![[0.0f32; 3]; count];

    // Both file and memory use Blue-major: idx = B + G*size + R*size²
    // No conversion needed, just scale
    for (i, rgb) in raw_data.iter().enumerate() {
        data[i] = [
            rgb[0] as f32 * scale,
            rgb[1] as f32 * scale,
            rgb[2] as f32 * scale,
        ];
    }

    Lut3D::from_data(data, size)
}

/// Reads a Pandora MGA/M3D file from disk.
pub fn read_mga<P: AsRef<Path>>(path: P) -> LutResult<Lut3D> {
    let file = std::fs::File::open(path.as_ref())?;
    parse_mga(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real Pandora syntax has no colon after the keyword (`FileFormatPandora.cpp::read()`
    /// matches `parts[0] == "channel"` etc., and `pandora_3d.m3d` / the reference's own
    /// `SAMPLE_NO_ERROR` inline fixture (`FileFormatPandora_tests.cpp:46-59`) never carry one).
    /// Before the fix, every header/data line here was silently skipped by the old
    /// `starts_with("channel:")`-style checks, `in_lut` never became `true`, and the file failed
    /// with a misleading "missing 'out'" error instead of parsing.
    #[test]
    fn parse_simple_mga() {
        let data = r#"channel 3d
in 8
out 256
format lut
values red green blue
0 0 0 0
1 0 0 255
2 0 255 0
3 0 255 255
4 255 0 0
5 255 0 255
6 255 255 0
7 255 255 255
"#;
        let lut = parse_mga(data.as_bytes()).unwrap();
        assert_eq!(lut.size, 2);
        assert_eq!(lut.data.len(), 8);

        // scale = 1 / (out - 1) = 1 / 255 (FileFormatPandora.cpp): corners are exact.
        assert!((lut.data[0][0] - 0.0).abs() < 1e-6);
        assert!((lut.data[7][0] - 1.0).abs() < 1e-6);
        // Interior entry 1 (index 0,0,255 -> blue channel only): matches file row 1 exactly.
        assert!((lut.data[1][0] - 0.0).abs() < 1e-6);
        assert!((lut.data[1][1] - 0.0).abs() < 1e-6);
        assert!((lut.data[1][2] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn error_missing_out() {
        let data = r#"channel 3d
in 8
format lut
values red green blue
0 0 0 0
"#;
        assert!(parse_mga(data.as_bytes()).is_err());
    }

    #[test]
    fn error_non_cube() {
        let data = r#"channel 3d
in 7
out 256
format lut
values red green blue
0 0 0 0
1 0 0 255
2 0 255 0
3 0 255 255
4 255 0 0
5 255 0 255
6 255 255 0
"#;
        assert!(parse_mga(data.as_bytes()).is_err());
    }

    /// `channel:`/`in:`/... WITH a colon is not real Pandora syntax (OCIO's `parts[0] == "in"`
    /// never matches `"in:"`), but must still parse without silently dropping data: unrecognized
    /// keyword lines fall through to the `in_lut` data branch, same as OCIO's own unmatched
    /// `else if` chain (`FileFormatPandora.cpp::read()` doesn't special-case a colon suffix
    /// either -- it just never matches, and OCIO would misparse this the same way). Documented
    /// here so a future "helpful" colon-stripping fix doesn't silently reintroduce COLOR-08.
    #[test]
    fn colon_suffixed_headers_are_not_recognized() {
        let data = "channel: 3d\nin: 8\nout: 256\nformat: lut\nvalues: red green blue\n0 0 0 0\n";
        let err = parse_mga(data.as_bytes()).unwrap_err();
        assert!(matches!(err, LutError::ParseError(_)));
    }
}
