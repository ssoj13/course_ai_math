//! FilmLight Truelight .cub format support.
//!
//! The Truelight .cub format is used by FilmLight Truelight and other tools.
//! It supports 3D LUTs with optional 1D shaper (InputLUT).
//!
//! # Format
//!
//! ```text
//! # Truelight Cube v2.0
//! # lutLength 1024
//! # iDims     3
//! # oDims     3
//! # width     32 32 32
//!
//! # InputLUT
//! 0.000000 0.000000 0.000000
//! 0.030303 0.030303 0.030303
//! ...
//!
//! # Cube
//! 0.0 0.0 0.0
//! 1.0 0.0 0.0
//! ...
//! # end
//! ```
//!
//! # Notes
//!
//! - InputLUT values are scaled to [0, cube_size-1] range
//! - On read, values are descaled back to [0, 1]
//! - 3D LUT must be a cube (equal dimensions)
//! - The cube is stored **red-fastest** (`FileFormatTruelight.cpp` reads it with
//!   `setArrayFromRedFastestOrder` and bakes it from a `LUT3DORDER_FAST_RED` identity); the reader
//!   and writer convert to and from [`Lut3D`]'s blue-fastest memory order.

use crate::limits::{MAX_1D_LUT_LENGTH, MAX_3D_LUT_LENGTH, check_1d_length, check_3d_edge};
use crate::{Lut1D, Lut3D, Lut3dOrder, LutError, LutResult, OrderedLut3dData};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::Path;

/// Parsed Truelight file.
#[derive(Debug, Clone)]
pub struct TruelightFile {
    /// Optional 1D shaper LUT (InputLUT)
    pub shaper: Option<Lut1D>,
    /// 3D cube LUT
    pub cube: Lut3D,
}

impl TruelightFile {
    /// Create from 3D LUT only.
    pub fn new(cube: Lut3D) -> Self {
        Self { shaper: None, cube }
    }

    /// Create with shaper and 3D LUT.
    pub fn with_shaper(shaper: Lut1D, cube: Lut3D) -> Self {
        Self {
            shaper: Some(shaper),
            cube,
        }
    }
}

/// Read a Truelight .cub file.
pub fn read_cub<P: AsRef<Path>>(path: P) -> LutResult<TruelightFile> {
    let file = File::open(path.as_ref())?;
    let reader = BufReader::new(file);
    parse_cub(reader)
}

/// Parse a Truelight .cub file from reader.
pub fn parse_cub<R: Read>(reader: R) -> LutResult<TruelightFile> {
    let buf_reader = BufReader::new(reader);
    let mut lines = buf_reader.lines();

    // Validate first line
    let first_line = lines
        .next()
        .ok_or_else(|| LutError::ParseError("empty file".into()))??;

    if !first_line.to_lowercase().contains("truelight cube") {
        return Err(LutError::ParseError("not a Truelight .cub file".into()));
    }

    // Port of `FileFormatTruelight.cpp` `LocalFileFormat::read`: sizes come from the `width` and
    // `lutLength` tags (a malformed tag is an error, not 0), the entry counts are capped while
    // streaming and must match the tags exactly.
    let mut size1d = 0usize;
    let mut size3d = 0usize;
    let mut in_1d = false;
    let mut in_3d = false;
    let mut raw_1d: Vec<f32> = Vec::new();
    let mut raw_3d: Vec<f32> = Vec::new();

    for line_result in lines {
        let line = line_result?;
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with('#') {
            let lower = trimmed.to_lowercase();
            let parts: Vec<&str> = lower.split_whitespace().collect();

            if parts.len() < 2 {
                continue;
            }

            match parts[1] {
                "width" => {
                    let dims: Option<Vec<usize>> = parts
                        .get(2..5)
                        .and_then(|d| d.iter().map(|v| v.parse().ok()).collect());
                    let dims = dims.ok_or_else(|| {
                        LutError::ParseError("Malformed width tag in Truelight .cub LUT.".into())
                    })?;
                    if dims[0] != dims[1] || dims[0] != dims[2] {
                        return Err(LutError::SizeOutOfRange {
                            format: "Truelight",
                            dims: dims.iter().map(|&d| d as u64).collect(),
                            reason: "only equal grid size LUTs are supported".into(),
                        });
                    }
                    size3d = check_3d_edge("Truelight", dims[0])?;
                }
                "lutlength" => {
                    let n: usize = parts.get(2).and_then(|v| v.parse().ok()).ok_or_else(|| {
                        LutError::ParseError(
                            "Malformed lutlength tag in Truelight .cub LUT.".into(),
                        )
                    })?;
                    size1d = check_1d_length("Truelight", n)?;
                }
                "inputlut" => {
                    in_1d = true;
                    in_3d = false;
                }
                "cube" => {
                    in_3d = true;
                    in_1d = false;
                }
                "end" => {
                    break;
                }
                _ => {}
            }
            continue;
        }

        // A data line counts only when it is exactly three numbers (OCIO `StringVecToFloatVec`
        // plus `size() == 3`); anything else is skipped, as in OCIO.
        let values: Option<Vec<f32>> = trimmed.split_whitespace().map(|v| v.parse().ok()).collect();
        let Some(values) = values.filter(|v| v.len() == 3) else {
            continue;
        };
        if in_1d {
            if raw_1d.len() > MAX_1D_LUT_LENGTH * 3 {
                return Err(LutError::ParseError(
                    "Too many 1D LUT entries in Truelight .cub LUT.".into(),
                ));
            }
            raw_1d.extend_from_slice(&values);
        } else if in_3d {
            if raw_3d.len() > MAX_3D_LUT_LENGTH * MAX_3D_LUT_LENGTH * MAX_3D_LUT_LENGTH * 3 {
                return Err(LutError::ParseError(
                    "Too many 3D LUT entries in Truelight .cub LUT.".into(),
                ));
            }
            raw_3d.extend_from_slice(&values);
        }
    }

    if raw_1d.len() / 3 != size1d {
        return Err(LutError::ParseError(format!(
            "Incorrect number of lut1d entries in Truelight .cub LUT. Found {}, expected {size1d}.",
            raw_1d.len() / 3
        )));
    }
    let count3d = size3d * size3d * size3d;
    if raw_3d.len() / 3 != count3d {
        return Err(LutError::ParseError(format!(
            "Incorrect number of 3D LUT entries in Truelight .cub LUT. Found {}, expected {count3d}.",
            raw_3d.len() / 3
        )));
    }
    // OCIO also reads a 1D-only .cub; `TruelightFile` always carries a cube.
    if size3d == 0 {
        return Err(LutError::ParseError(
            "Truelight .cub LUT has no 3D cube (no width tag); 1D-only files are not supported"
                .into(),
        ));
    }

    // The file is red-fastest; `Lut3D` is blue-fastest.
    let cube_data: Vec<[f32; 3]> = raw_3d.as_chunks::<3>().0.iter().map(|c| [c[0], c[1], c[2]]).collect();
    let cube_data =
        OrderedLut3dData::red_fastest(cube_data, size3d).into_order(Lut3dOrder::BlueFastest);
    let cube = Lut3D::from_data(cube_data, size3d)?;

    // The InputLUT feeding an N^3 cube is written in [0, N-1] (OCIO: "the inputlut feeding a
    // 6x6x6 3D LUT should be scaled from 0.0-5.0").
    let shaper = if size1d > 0 {
        let descale = 1.0 / (size3d - 1) as f32;
        let channel = |c: usize| -> Vec<f32> {
            raw_1d
                .iter()
                .skip(c)
                .step_by(3)
                .map(|&v| v * descale)
                .collect()
        };
        Some(Lut1D::from_rgb(
            channel(0),
            channel(1),
            channel(2),
            0.0,
            1.0,
        )?)
    } else {
        None
    };

    Ok(TruelightFile { shaper, cube })
}

/// Write a Truelight .cub file.
pub fn write_cub<P: AsRef<Path>>(path: P, tl: &TruelightFile) -> LutResult<()> {
    let file = File::create(path.as_ref())?;
    let writer = BufWriter::new(file);
    write_cub_to(writer, tl)
}

/// Write a Truelight .cub file to any writer.
pub fn write_cub_to<W: Write>(mut writer: W, tl: &TruelightFile) -> LutResult<()> {
    let cube_size = tl.cube.size;
    let shaper_size = tl.shaper.as_ref().map(|s| s.size()).unwrap_or(1024);

    // Header
    writeln!(writer, "# Truelight Cube v2.0")?;
    writeln!(writer, "# lutLength {}", shaper_size)?;
    writeln!(writer, "# iDims     3")?;
    writeln!(writer, "# oDims     3")?;
    writeln!(
        writer,
        "# width     {} {} {}",
        cube_size, cube_size, cube_size
    )?;
    writeln!(writer)?;

    // InputLUT (shaper)
    writeln!(writer, "# InputLUT")?;

    if let Some(shaper) = &tl.shaper {
        // Scale to [0, cube_size-1]
        let scale = (cube_size - 1) as f32;
        let r = &shaper.r;
        let g = shaper.g.as_ref().unwrap_or(&shaper.r);
        let b = shaper.b.as_ref().unwrap_or(&shaper.r);

        for i in 0..shaper.size() {
            writeln!(
                writer,
                "{:.6} {:.6} {:.6}",
                r[i] * scale,
                g[i] * scale,
                b[i] * scale
            )?;
        }
    } else {
        // Generate identity shaper
        let scale = (cube_size - 1) as f32;
        for i in 0..shaper_size {
            let v = (i as f32 / (shaper_size - 1) as f32) * scale;
            writeln!(writer, "{:.6} {:.6} {:.6}", v, v, v)?;
        }
    }

    writeln!(writer)?;

    // Cube
    writeln!(writer, "# Cube")?;
    // Red-fastest on disk, like OCIO's baker.
    let red_fastest = OrderedLut3dData::blue_fastest(tl.cube.data.clone(), cube_size)
        .into_order(Lut3dOrder::RedFastest);
    for rgb in &red_fastest {
        writeln!(writer, "{:.6} {:.6} {:.6}", rgb[0], rgb[1], rgb[2])?;
    }

    writeln!(writer, "# end")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parse_basic_cub() {
        let content = r#"# Truelight Cube v2.0
# lutLength 4
# iDims     3
# oDims     3
# width     2 2 2

# InputLUT
0.0 0.0 0.0
0.333 0.333 0.333
0.666 0.666 0.666
1.0 1.0 1.0

# Cube
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
# end
"#;
        let tl = parse_cub(Cursor::new(content)).expect("parse failed");

        assert_eq!(tl.cube.size, 2);
        assert!(tl.shaper.is_some());

        let shaper = tl.shaper.unwrap();
        assert_eq!(shaper.size(), 4);
    }

    /// The fixture's second row (`1 0 0`) is the red corner: the file is red-fastest. Reading it
    /// as blue-fastest (the old reader) swapped red and blue.
    #[test]
    fn cube_is_red_fastest_on_disk() {
        let content = "# Truelight Cube v2.0\n# width 2 2 2\n# Cube\n\
            0 0 0\n0.9 0.1 0.2\n0 1 0\n1 1 0\n0.3 0.2 0.8\n1 0 1\n0 1 1\n1 1 1\n# end\n";
        let tl = parse_cub(Cursor::new(content)).expect("parse failed");
        assert_eq!(tl.cube.apply([1.0, 0.0, 0.0]), [0.9, 0.1, 0.2]);
        assert_eq!(tl.cube.apply([0.0, 0.0, 1.0]), [0.3, 0.2, 0.8]);

        // And the writer puts it back in the same order.
        let mut buf = Vec::new();
        write_cub_to(&mut buf, &tl).expect("write failed");
        let text = String::from_utf8(buf).unwrap();
        let cube_rows: Vec<&str> = text
            .lines()
            .skip_while(|l| *l != "# Cube")
            .skip(1)
            .take(2)
            .collect();
        assert_eq!(cube_rows[1], "0.900000 0.100000 0.200000");
    }

    #[test]
    fn hostile_or_malformed_sizes_are_errors() {
        let cub = |header: &str| format!("# Truelight Cube v2.0\n{header}\n# Cube\n# end\n");
        for header in [
            "# width 1 1 1",
            "# width 0 0 0",
            "# width 130 130 130",
            "# width 2 2 3",
            "# width 2 two 2",
            "# width 2 2",
            "# width 2 2 2\n# lutLength 1",
            "# width 2 2 2\n# lutLength 300001",
            "# width 2 2 2\n# lutLength x",
        ] {
            assert!(
                parse_cub(Cursor::new(cub(header))).is_err(),
                "{header:?} must be rejected"
            );
        }
    }

    #[test]
    fn entry_counts_must_match_the_tags() {
        let short = "# Truelight Cube v2.0\n# width 2 2 2\n# Cube\n0 0 0\n# end\n";
        assert!(parse_cub(Cursor::new(short)).is_err());
        let extra_1d = "# Truelight Cube v2.0\n# lutLength 2\n# width 2 2 2\n# InputLUT\n\
            0 0 0\n1 1 1\n1 1 1\n# Cube\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n";
        assert!(parse_cub(Cursor::new(extra_1d)).is_err());
    }

    #[test]
    fn parse_no_shaper() {
        let content = r#"# Truelight Cube v2.0
# width     2 2 2

# Cube
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
# end
"#;
        let tl = parse_cub(Cursor::new(content)).expect("parse failed");

        assert_eq!(tl.cube.size, 2);
        assert!(tl.shaper.is_none());
    }

    #[test]
    fn roundtrip_3d() {
        let cube = Lut3D::identity(4);
        let tl = TruelightFile::new(cube);

        let mut buf = Vec::new();
        write_cub_to(&mut buf, &tl).expect("write failed");

        let parsed = parse_cub(Cursor::new(buf)).expect("parse failed");

        assert_eq!(parsed.cube.size, 4);
        // identity(4) corners survive scale-on-write / descale-on-read round-trip.
        let c = &parsed.cube;
        assert!(
            c.data[0].iter().all(|&v| v.abs() < 1e-4),
            "black={:?}",
            c.data[0]
        );
        let last = c.data.len() - 1;
        assert!(
            c.data[last].iter().all(|&v| (v - 1.0).abs() < 1e-4),
            "white={:?}",
            c.data[last]
        );
    }

    #[test]
    fn roundtrip_with_shaper() {
        let shaper = Lut1D::gamma(64, 2.2);
        let cube = Lut3D::identity(8);
        let tl = TruelightFile::with_shaper(shaper, cube);

        let mut buf = Vec::new();
        write_cub_to(&mut buf, &tl).expect("write failed");

        let parsed = parse_cub(Cursor::new(buf)).expect("parse failed");

        assert_eq!(parsed.cube.size, 8);
        assert!(parsed.shaper.is_some());
        assert_eq!(parsed.shaper.unwrap().size(), 64);
    }

    #[test]
    fn shaper_descaling() {
        // Test that shaper values are properly descaled
        let content = r#"# Truelight Cube v2.0
# lutLength 2
# width     4 4 4

# InputLUT
0.0 0.0 0.0
3.0 3.0 3.0

# Cube
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
# end
"#;
        let tl = parse_cub(Cursor::new(content)).expect("parse failed");

        let shaper = tl.shaper.expect("should have shaper");
        // cube_size = 4, so descale = 1/3
        // 0.0 -> 0.0, 3.0 -> 1.0
        assert!((shaper.r[0] - 0.0).abs() < 0.01);
        assert!((shaper.r[1] - 1.0).abs() < 0.01);
    }
}
