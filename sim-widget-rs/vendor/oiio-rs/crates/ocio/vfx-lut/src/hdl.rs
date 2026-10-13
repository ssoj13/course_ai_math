//! Houdini .lut (HDL) format support.
//!
//! The HDL format is used by SideFX Houdini and related tools.
//! It supports 1D LUTs, 3D LUTs, and 3D LUTs with 1D preluts.
//!
//! # Format
//!
//! ```text
//! Version     3
//! Format      any
//! Type        3D+1D
//! From        0.0 1.0
//! To          0.0 1.0
//! Black       0.0
//! White       1.0
//! Length      32 1024
//! LUT:
//! Pre {
//!     0.0
//!     0.001
//!     ...
//! }
//! 3D {
//!     0.0 0.0 0.0
//!     1.0 0.0 0.0
//!     ...
//! }
//! ```
//!
//! # LUT Types
//!
//! - `C` or `RGB`: 1D LUT (version 1)
//! - `3D`: 3D LUT only (version 2)
//! - `3D+1D`: 3D LUT with 1D prelut (version 3)

use crate::{Lut1D, Lut3D, LutError, LutResult};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::Path;

/// HDL LUT type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HdlType {
    /// 1D LUT (mono channel "C" or RGB channels)
    Lut1D,
    /// 3D LUT only
    Lut3D,
    /// 3D LUT with 1D prelut
    Lut3D1D,
}

impl HdlType {
    fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "c" | "rgb" | "r" => Some(HdlType::Lut1D),
            "3d" => Some(HdlType::Lut3D),
            "3d+1d" => Some(HdlType::Lut3D1D),
            _ => None,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            HdlType::Lut1D => "RGB",
            HdlType::Lut3D => "3D",
            HdlType::Lut3D1D => "3D+1D",
        }
    }

    fn version(&self) -> u32 {
        match self {
            HdlType::Lut1D => 1,
            HdlType::Lut3D => 2,
            HdlType::Lut3D1D => 3,
        }
    }
}

/// Parsed HDL file data.
#[derive(Debug, Clone)]
pub struct HdlFile {
    /// LUT type
    pub lut_type: HdlType,
    /// Input range (from_min, from_max)
    pub from_range: (f32, f32),
    /// Output range (to_min, to_max)
    pub to_range: (f32, f32),
    /// Black level
    pub black: f32,
    /// White level
    pub white: f32,
    /// 1D LUT (if present)
    pub lut1d: Option<Lut1D>,
    /// 3D LUT (if present)
    pub lut3d: Option<Lut3D>,
}

impl HdlFile {
    /// Create a new HDL file with 1D LUT.
    pub fn new_1d(lut: Lut1D) -> Self {
        Self {
            lut_type: HdlType::Lut1D,
            // HDL uses scalar range; use R channel (assume uniform)
            from_range: (lut.domain_min[0], lut.domain_max[0]),
            to_range: (0.0, 1.0),
            black: 0.0,
            white: 1.0,
            lut1d: Some(lut),
            lut3d: None,
        }
    }

    /// Create a new HDL file with 3D LUT.
    pub fn new_3d(lut: Lut3D) -> Self {
        Self {
            lut_type: HdlType::Lut3D,
            from_range: (0.0, 1.0),
            to_range: (0.0, 1.0),
            black: 0.0,
            white: 1.0,
            lut1d: None,
            lut3d: Some(lut),
        }
    }

    /// Create a new HDL file with 3D LUT and 1D prelut.
    pub fn new_3d1d(prelut: Lut1D, lut3d: Lut3D) -> Self {
        Self {
            lut_type: HdlType::Lut3D1D,
            from_range: (prelut.domain_min[0], prelut.domain_max[0]),
            to_range: (0.0, 1.0),
            black: 0.0,
            white: 1.0,
            lut1d: Some(prelut),
            lut3d: Some(lut3d),
        }
    }
}

/// Read an HDL file.
pub fn read_hdl<P: AsRef<Path>>(path: P) -> LutResult<HdlFile> {
    let file = File::open(path.as_ref())?;
    let reader = BufReader::new(file);
    parse_hdl(reader)
}

/// Parse an HDL file from reader.
pub fn parse_hdl<R: Read>(reader: R) -> LutResult<HdlFile> {
    let buf_reader = BufReader::new(reader);

    // Parse headers
    let mut headers: HashMap<String, Vec<String>> = HashMap::new();
    let mut lines_iter = buf_reader.lines();
    let mut remaining_content = String::new();

    // Read headers until "LUT:" line
    for line_result in &mut lines_iter {
        let line = line_result?;
        let trimmed = line.trim().to_lowercase();

        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with("lut:") {
            break;
        }

        let parts: Vec<String> = line.split_whitespace().map(|s| s.to_string()).collect();

        if !parts.is_empty() {
            let key = parts[0].to_lowercase();
            let values = parts[1..].to_vec();
            headers.insert(key, values);
        }
    }

    // Collect remaining content for LUT parsing
    for line_result in lines_iter {
        remaining_content.push_str(&line_result?);
        remaining_content.push(' ');
    }

    // Parse header values
    let lut_type = headers
        .get("type")
        .and_then(|v| v.first())
        .and_then(|s| HdlType::from_str(s))
        .ok_or_else(|| LutError::ParseError("missing or invalid Type".into()))?;

    let from_range = parse_range(&headers, "from")?;
    let to_range = parse_range(&headers, "to")?;

    let black = headers
        .get("black")
        .and_then(|v| v.first())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);

    let white = headers
        .get("white")
        .and_then(|v| v.first())
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0);

    // Parse lengths
    // FileFormatHDL.cpp: a token that is not an integer is an error, not skipped.
    let lengths: Vec<usize> = headers
        .get("length")
        .map(|v| {
            v.iter()
                .map(|s| {
                    s.parse().map_err(|_| {
                        LutError::ParseError(format!("Invalid integer on 'Length' line: '{s}'"))
                    })
                })
                .collect::<LutResult<Vec<usize>>>()
        })
        .transpose()?
        .unwrap_or_default();

    if lengths.is_empty() {
        return Err(LutError::ParseError("missing Length".into()));
    }

    // Parse LUT data blocks
    let lut_blocks = parse_lut_blocks(&remaining_content)?;

    // Build LUTs based on type
    let (lut1d, lut3d) = match lut_type {
        HdlType::Lut1D => {
            let size = crate::limits::check_1d_length("HDL", lengths[0])?;
            let lut = build_1d_lut(&lut_blocks, size, from_range)?;
            (Some(lut), None)
        }
        HdlType::Lut3D => {
            let cube_size = crate::limits::check_3d_edge("HDL", lengths[0])?;
            let lut = build_3d_lut(&lut_blocks, cube_size, to_range)?;
            (None, Some(lut))
        }
        HdlType::Lut3D1D => {
            let cube_size = crate::limits::check_3d_edge("HDL", lengths[0])?;
            // A 3D+1D file names both sizes (FileFormatHDL.cpp reads `lut_sizes[1]`).
            let prelut_size = lengths.get(1).copied().ok_or_else(|| {
                LutError::ParseError(
                    "3D+1D LUT needs a second 'Length' value (the prelut size)".into(),
                )
            })?;
            let prelut_size = crate::limits::check_1d_length("HDL", prelut_size)?;

            let prelut = build_prelut(&lut_blocks, prelut_size, from_range)?;
            let lut3d = build_3d_lut(&lut_blocks, cube_size, to_range)?;

            (Some(prelut), Some(lut3d))
        }
    };

    Ok(HdlFile {
        lut_type,
        from_range,
        to_range,
        black,
        white,
        lut1d,
        lut3d,
    })
}

fn parse_range(headers: &HashMap<String, Vec<String>>, key: &str) -> LutResult<(f32, f32)> {
    let values = headers
        .get(key)
        .ok_or_else(|| LutError::ParseError(format!("missing {key}")))?;

    if values.len() < 2 {
        return Err(LutError::ParseError(format!("invalid {key} range")));
    }

    let min: f32 = values[0]
        .parse()
        .map_err(|_| LutError::ParseError(format!("invalid {key} min")))?;
    let max: f32 = values[1]
        .parse()
        .map_err(|_| LutError::ParseError(format!("invalid {key} max")))?;

    Ok((min, max))
}

/// Parse LUT blocks like "Pre { ... }", "3D { ... }", "RGB { ... }"
fn parse_lut_blocks(content: &str) -> LutResult<HashMap<String, Vec<f32>>> {
    let mut blocks: HashMap<String, Vec<f32>> = HashMap::new();
    let mut current_name = String::new();
    let mut in_block = false;
    let mut current_values: Vec<f32> = Vec::new();

    let words: Vec<&str> = content.split_whitespace().collect();
    let mut i = 0;

    while i < words.len() {
        let word = words[i];

        if !in_block {
            if word == "{" {
                // Anonymous block (3D LUT)
                in_block = true;
                current_name = "3d".to_string();
                current_values.clear();
            } else if i + 1 < words.len() && words[i + 1] == "{" {
                // Named block
                in_block = true;
                current_name = word.to_lowercase();
                current_values.clear();
                i += 1; // Skip the "{"
            }
        } else if word == "}" {
            // End of block
            blocks.insert(current_name.clone(), current_values.clone());
            in_block = false;
            current_name.clear();
        } else {
            // Try to parse as float
            if let Ok(v) = word.parse::<f32>() {
                current_values.push(v);
            }
        }

        i += 1;
    }

    Ok(blocks)
}

fn build_1d_lut(
    blocks: &HashMap<String, Vec<f32>>,
    size: usize,
    domain: (f32, f32),
) -> LutResult<Lut1D> {
    // Try RGB block first (mono LUT applied to all channels)
    if let Some(values) = blocks.get("rgb") {
        if values.len() != size {
            return Err(LutError::ParseError(format!(
                "RGB{{}} LUT was {} values long, expected {size} values",
                values.len()
            )));
        }
        return Lut1D::from_data(values.clone(), domain.0, domain.1);
    }

    // Try separate R, G, B blocks
    let r = blocks.get("r");
    let g = blocks.get("g");
    let b = blocks.get("b");

    if let (Some(r_vals), Some(g_vals), Some(b_vals)) = (r, g, b) {
        if [r_vals, g_vals, b_vals].iter().any(|v| v.len() != size) {
            return Err(LutError::ParseError(format!(
                "R/G/B LUT blocks are {}/{}/{} values long, expected {size} each",
                r_vals.len(),
                g_vals.len(),
                b_vals.len()
            )));
        }
        return Lut1D::from_rgb(
            r_vals.clone(),
            g_vals.clone(),
            b_vals.clone(),
            domain.0,
            domain.1,
        );
    }

    Err(LutError::ParseError("no valid 1D LUT data found".into()))
}

fn build_prelut(
    blocks: &HashMap<String, Vec<f32>>,
    size: usize,
    domain: (f32, f32),
) -> LutResult<Lut1D> {
    let values = blocks
        .get("pre")
        .ok_or_else(|| LutError::ParseError("missing Pre block".into()))?;

    if values.len() != size {
        return Err(LutError::ParseError(format!(
            "Pre{{}} LUT was {} values long, expected {size} values",
            values.len()
        )));
    }

    Lut1D::from_data(values.clone(), domain.0, domain.1)
}

fn build_3d_lut(
    blocks: &HashMap<String, Vec<f32>>,
    cube_size: usize,
    _range: (f32, f32),
) -> LutResult<Lut3D> {
    let values = blocks
        .get("3d")
        .ok_or_else(|| LutError::ParseError("missing 3D block".into()))?;

    let expected = crate::limits::check_3d_size("HDL", cube_size)? * 3;
    if values.len() != expected {
        return Err(LutError::ParseError(format!(
            "3D LUT has {} values, expected {}",
            values.len(),
            expected
        )));
    }

    // Convert flat RGB array to [[f32; 3]]
    let data: Vec<[f32; 3]> = values[..expected]
        .chunks(3)
        .map(|chunk| [chunk[0], chunk[1], chunk[2]])
        .collect();

    // HDL uses R-fastest order, but Lut3D expects B-fastest order
    // Reorder from file order (R-fastest) to memory order (B-fastest)
    // Reference: OCIO FileFormatHDL.cpp:533 calls setArrayFromRedFastestOrder()
    let mut reordered = vec![[0.0f32; 3]; data.len()];
    for b in 0..cube_size {
        for g in 0..cube_size {
            for r in 0..cube_size {
                let file_idx = r + g * cube_size + b * cube_size * cube_size; // R-fastest
                let mem_idx = b + g * cube_size + r * cube_size * cube_size; // B-fastest
                reordered[mem_idx] = data[file_idx];
            }
        }
    }

    Lut3D::from_data(reordered, cube_size)
}

/// Write an HDL file.
pub fn write_hdl<P: AsRef<Path>>(path: P, hdl: &HdlFile) -> LutResult<()> {
    let file = File::create(path.as_ref())?;
    let writer = BufWriter::new(file);
    write_hdl_to(writer, hdl)
}

/// Write an HDL file to any writer.
pub fn write_hdl_to<W: Write>(mut writer: W, hdl: &HdlFile) -> LutResult<()> {
    // Header
    writeln!(writer, "Version\t\t{}", hdl.lut_type.version())?;
    writeln!(writer, "Format\t\tany")?;
    writeln!(writer, "Type\t\t{}", hdl.lut_type.as_str())?;
    writeln!(writer, "From\t\t{} {}", hdl.from_range.0, hdl.from_range.1)?;
    writeln!(writer, "To\t\t{} {}", hdl.to_range.0, hdl.to_range.1)?;
    writeln!(writer, "Black\t\t{}", hdl.black)?;
    writeln!(writer, "White\t\t{}", hdl.white)?;

    // Length
    match hdl.lut_type {
        HdlType::Lut1D => {
            if let Some(lut) = &hdl.lut1d {
                writeln!(writer, "Length\t\t{}", lut.size())?;
            }
        }
        HdlType::Lut3D => {
            if let Some(lut) = &hdl.lut3d {
                writeln!(writer, "Length\t\t{}", lut.size)?;
            }
        }
        HdlType::Lut3D1D => {
            if let (Some(prelut), Some(lut3d)) = (&hdl.lut1d, &hdl.lut3d) {
                writeln!(writer, "Length\t\t{} {}", lut3d.size, prelut.size())?;
            }
        }
    }

    writeln!(writer, "LUT:")?;

    // Write LUT data
    match hdl.lut_type {
        HdlType::Lut1D => {
            if let Some(lut) = &hdl.lut1d {
                write_1d_lut(&mut writer, lut)?;
            }
        }
        HdlType::Lut3D => {
            if let Some(lut) = &hdl.lut3d {
                write_3d_lut(&mut writer, lut, false)?;
            }
        }
        HdlType::Lut3D1D => {
            if let Some(prelut) = &hdl.lut1d {
                write_prelut(&mut writer, prelut)?;
            }
            if let Some(lut3d) = &hdl.lut3d {
                write_3d_lut(&mut writer, lut3d, true)?;
            }
        }
    }

    Ok(())
}

fn write_1d_lut<W: Write>(writer: &mut W, lut: &Lut1D) -> LutResult<()> {
    let r = &lut.r;
    let g = lut.g.as_ref().unwrap_or(&lut.r);
    let b = lut.b.as_ref().unwrap_or(&lut.r);

    // Write separate channels
    writeln!(writer, "R {{")?;
    for &v in r.iter() {
        writeln!(writer, "\t{:.6}", v)?;
    }
    writeln!(writer, "}}")?;

    writeln!(writer, "G {{")?;
    for &v in g.iter() {
        writeln!(writer, "\t{:.6}", v)?;
    }
    writeln!(writer, "}}")?;

    writeln!(writer, "B {{")?;
    for &v in b.iter() {
        writeln!(writer, "\t{:.6}", v)?;
    }
    writeln!(writer, "}}")?;

    Ok(())
}

fn write_prelut<W: Write>(writer: &mut W, lut: &Lut1D) -> LutResult<()> {
    // Prelut is mono - use green channel (or red if mono)
    let data = lut.g.as_ref().unwrap_or(&lut.r);

    writeln!(writer, "Pre {{")?;
    for &v in data.iter() {
        writeln!(writer, "\t{:.6}", v)?;
    }
    writeln!(writer, "}}")?;

    Ok(())
}

fn write_3d_lut<W: Write>(writer: &mut W, lut: &Lut3D, named: bool) -> LutResult<()> {
    if named {
        writeln!(writer, "3D {{")?;
    } else {
        writeln!(writer, " {{")?;
    }

    // Write in R-fastest order (file format) from B-fastest memory order
    // HDL uses R-fastest like CUBE/ITX formats
    let size = lut.size;
    for b in 0..size {
        for g in 0..size {
            for r in 0..size {
                let mem_idx = b + g * size + r * size * size; // B-fastest memory
                let rgb = lut.data[mem_idx];
                writeln!(writer, "\t{:.6} {:.6} {:.6}", rgb[0], rgb[1], rgb[2])?;
            }
        }
    }

    writeln!(writer, " }}")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parse_3d_hdl() {
        let content = r#"
Version     2
Format      any
Type        3D
From        0.0 1.0
To          0.0 1.0
Black       0.0
White       1.0
Length      2
LUT:
 {
    0.0 0.0 0.0
    1.0 0.0 0.0
    0.0 1.0 0.0
    1.0 1.0 0.0
    0.0 0.0 1.0
    1.0 0.0 1.0
    0.0 1.0 1.0
    1.0 1.0 1.0
 }
"#;
        let hdl = parse_hdl(Cursor::new(content)).expect("parse failed");

        assert_eq!(hdl.lut_type, HdlType::Lut3D);
        assert!(hdl.lut3d.is_some());
        assert!(hdl.lut1d.is_none());

        let lut = hdl.lut3d.unwrap();
        assert_eq!(lut.size, 2);
        // Fixture is the corner cube: black -> [0,0,0], white -> [1,1,1].
        let black = lut.apply([0.0, 0.0, 0.0]);
        assert!(black.iter().all(|&c| c.abs() < 0.01), "black={black:?}");
        let white = lut.apply([1.0, 1.0, 1.0]);
        assert!(
            white.iter().all(|&c| (c - 1.0).abs() < 0.01),
            "white={white:?}"
        );

        // Interior (non-corner-of-the-diagonal) entry: file row 1, "1.0 0.0 0.0", is Houdini's
        // Red-fastest (r=1,g=0,b=0). Reference: `FileFormatHDL_tests.cpp:414-422` ("Houdini
        // order, red changes fastest"). Memory (Blue-fastest): idx = b+size*(g+size*r) = 4.
        assert_eq!(lut.data[4], [1.0, 0.0, 0.0]);
    }

    #[test]
    fn parse_1d_hdl() {
        let content = r#"
Version     1
Format      any
Type        RGB
From        0.0 1.0
To          0.0 1.0
Black       0.0
White       1.0
Length      4
LUT:
R {
    0.0
    0.333
    0.666
    1.0
}
G {
    0.0
    0.333
    0.666
    1.0
}
B {
    0.0
    0.333
    0.666
    1.0
}
"#;
        let hdl = parse_hdl(Cursor::new(content)).expect("parse failed");

        assert_eq!(hdl.lut_type, HdlType::Lut1D);
        assert!(hdl.lut1d.is_some());
        assert!(hdl.lut3d.is_none());

        let lut = hdl.lut1d.unwrap();
        assert_eq!(lut.size(), 4);
        // Authored ramp [0.0, 0.333, 0.666, 1.0] per channel: apply(0.5) -> idx 1.5 ->
        // lerp(0.333, 0.666, 0.5) = 0.4995.
        assert!((lut.apply(0.5) - 0.4995).abs() < 0.01);
        assert!((lut.apply(1.0) - 1.0).abs() < 0.01);
    }

    #[test]
    fn parse_3d1d_hdl() {
        let content = r#"
Version     3
Format      any
Type        3D+1D
From        0.0 1.0
To          0.0 1.0
Black       0.0
White       1.0
Length      2 4
LUT:
Pre {
    0.0
    0.333
    0.666
    1.0
}
3D {
    0.0 0.0 0.0
    1.0 0.0 0.0
    0.0 1.0 0.0
    1.0 1.0 0.0
    0.0 0.0 1.0
    1.0 0.0 1.0
    0.0 1.0 1.0
    1.0 1.0 1.0
}
"#;
        let hdl = parse_hdl(Cursor::new(content)).expect("parse failed");

        assert_eq!(hdl.lut_type, HdlType::Lut3D1D);
        assert!(hdl.lut1d.is_some());
        assert!(hdl.lut3d.is_some());

        let prelut = hdl.lut1d.unwrap();
        assert_eq!(prelut.size(), 4);
        // Authored prelut ramp [0.0, 0.333, 0.666, 1.0]: apply(0.5) -> 0.4995.
        assert!((prelut.apply(0.5) - 0.4995).abs() < 0.01);

        let lut3d = hdl.lut3d.unwrap();
        assert_eq!(lut3d.size, 2);
        // Authored corner cube: black -> [0,0,0], white -> [1,1,1].
        assert!(lut3d.apply([0.0, 0.0, 0.0]).iter().all(|&c| c.abs() < 0.01));
        assert!(
            lut3d
                .apply([1.0, 1.0, 1.0])
                .iter()
                .all(|&c| (c - 1.0).abs() < 0.01)
        );
    }

    #[test]
    fn roundtrip_3d() {
        let lut3d = Lut3D::identity(4);
        let hdl = HdlFile::new_3d(lut3d);

        let mut buf = Vec::new();
        write_hdl_to(&mut buf, &hdl).expect("write failed");

        let parsed = parse_hdl(Cursor::new(buf)).expect("parse failed");

        assert_eq!(parsed.lut_type, HdlType::Lut3D);
        assert!(parsed.lut3d.is_some());
        let parsed3d = parsed.lut3d.unwrap();
        assert_eq!(parsed3d.size, 4);
        // identity(4) corners survive write->read.
        assert_eq!(parsed3d.data[0], [0.0, 0.0, 0.0]);
        assert_eq!(parsed3d.data[parsed3d.data.len() - 1], [1.0, 1.0, 1.0]);
    }

    #[test]
    fn roundtrip_1d() {
        let lut1d = Lut1D::gamma(64, 2.2);
        let hdl = HdlFile::new_1d(lut1d);

        let mut buf = Vec::new();
        write_hdl_to(&mut buf, &hdl).expect("write failed");

        let parsed = parse_hdl(Cursor::new(buf)).expect("parse failed");

        assert_eq!(parsed.lut_type, HdlType::Lut1D);
        assert!(parsed.lut1d.is_some());
        let parsed1d = parsed.lut1d.unwrap();
        assert_eq!(parsed1d.size(), 64);
        // gamma(64, 2.2) survives write->read: apply(0.5) == 0.5^2.2 = 0.2176.
        assert!((parsed1d.apply(0.5) - 0.5f32.powf(2.2)).abs() < 0.01);
    }

    #[test]
    fn roundtrip_3d1d() {
        let prelut = Lut1D::gamma(32, 2.2);
        let lut3d = Lut3D::identity(4);
        let hdl = HdlFile::new_3d1d(prelut, lut3d);

        let mut buf = Vec::new();
        write_hdl_to(&mut buf, &hdl).expect("write failed");

        let parsed = parse_hdl(Cursor::new(buf)).expect("parse failed");

        assert_eq!(parsed.lut_type, HdlType::Lut3D1D);
        assert!(parsed.lut1d.is_some());
        assert!(parsed.lut3d.is_some());
        // gamma(32, 2.2) prelut survives: apply(0.5) == 0.5^2.2 = 0.2176.
        let parsed1d = parsed.lut1d.unwrap();
        assert!((parsed1d.apply(0.5) - 0.5f32.powf(2.2)).abs() < 0.01);
        // identity(4) cube corners survive.
        let parsed3d = parsed.lut3d.unwrap();
        assert_eq!(parsed3d.data[0], [0.0, 0.0, 0.0]);
        assert_eq!(parsed3d.data[parsed3d.data.len() - 1], [1.0, 1.0, 1.0]);
    }
}
