//! Autodesk/Discreet .1dl LUT format support.
//!
//! The .1dl format is a 1D LUT format used by Autodesk Flame, Smoke, and Lustre.
//! It stores per-channel transfer functions with integer or float values.
//!
//! # Format Variants
//!
//! ## Old Format (legacy)
//! Plain integers, one per line. Count determines bit depth:
//! - 256 entries = 8-bit
//! - 1024 entries = 10-bit
//! - 4096 entries = 12-bit
//! - 65536 entries = 16-bit
//!
//! ## New Format (with header)
//! ```text
//! LUT: <numtables> <length> [dstDepth]
//! <values...>
//! ```
//!
//! Where:
//! - numtables: 1 (mono), 3 (RGB), or 4 (RGBA)
//! - length: 256, 1024, 4096, or 65536
//! - dstDepth (optional): 8, 10, 12, 16, 16f, or 32f
//!
//! # Example
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use vfx_lut::discreet1dl;
//!
//! let lut = discreet1dl::read_1dl("curve.1dl")?;
//! let value = lut.apply(0.5);
//! # Ok(())
//! # }
//! ```

use crate::{Lut1D, LutError, LutResult};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

/// Bit depth for output values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitDepth {
    /// 8-bit integer (0-255)
    Int8,
    /// 10-bit integer (0-1023)
    Int10,
    /// 12-bit integer (0-4095)
    Int12,
    /// 16-bit integer (0-65535)
    Int16,
    /// 16-bit half float
    Float16,
    /// 32-bit float
    Float32,
}

impl BitDepth {
    /// Maximum integer value for this bit depth.
    pub fn max_int(&self) -> u32 {
        match self {
            BitDepth::Int8 => 255,
            BitDepth::Int10 => 1023,
            BitDepth::Int12 => 4095,
            BitDepth::Int16 => 65535,
            BitDepth::Float16 | BitDepth::Float32 => 1, // normalized float
        }
    }

    /// Returns true if this is a float format.
    pub fn is_float(&self) -> bool {
        matches!(self, BitDepth::Float16 | BitDepth::Float32)
    }

    /// Parse from string (e.g., "8", "10", "16f", "32f", "65536f").
    ///
    /// Supports Smoke's convention of using "65536f" for 16f output depth.
    pub fn parse(s: &str) -> Option<Self> {
        let lower = s.to_lowercase();
        match lower.as_str() {
            "8" | "256" => Some(BitDepth::Int8),
            "10" | "1024" => Some(BitDepth::Int10),
            "12" | "4096" => Some(BitDepth::Int12),
            "16" | "65536" => Some(BitDepth::Int16),
            "16f" | "65536f" => Some(BitDepth::Float16),
            "32f" => Some(BitDepth::Float32),
            _ => {
                // Try parsing as {number}f pattern
                if lower.ends_with('f') {
                    let num_str = &lower[..lower.len() - 1];
                    if let Ok(num) = num_str.parse::<u32>() {
                        return match num {
                            256 => Some(BitDepth::Int8),   // unlikely but consistent
                            1024 => Some(BitDepth::Int10), // unlikely but consistent
                            4096 => Some(BitDepth::Int12), // unlikely but consistent
                            65536 => Some(BitDepth::Float16),
                            _ => None,
                        };
                    }
                }
                None
            }
        }
    }

    /// To string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            BitDepth::Int8 => "8",
            BitDepth::Int10 => "10",
            BitDepth::Int12 => "12",
            BitDepth::Int16 => "16",
            BitDepth::Float16 => "16f",
            BitDepth::Float32 => "32f",
        }
    }

    /// Infer bit depth from LUT length.
    pub fn from_length(len: usize) -> Option<Self> {
        match len {
            256 => Some(BitDepth::Int8),
            1024 => Some(BitDepth::Int10),
            4096 => Some(BitDepth::Int12),
            65536 => Some(BitDepth::Int16),
            _ => None,
        }
    }
}

/// Metadata from 1DL file header.
#[derive(Debug, Clone)]
pub struct Discreet1DLInfo {
    /// Number of channels (1=mono, 3=RGB, 4=RGBA)
    pub num_tables: usize,
    /// Number of entries per channel
    pub length: usize,
    /// Output bit depth
    pub dst_depth: BitDepth,
}

/// Reads a 1D LUT from a .1dl file.
///
/// # Example
///
/// ```no_run
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// # use vfx_lut::discreet1dl;
/// let lut = discreet1dl::read_1dl("curve.1dl")?;
/// # Ok(())
/// # }
/// ```
pub fn read_1dl<P: AsRef<Path>>(path: P) -> LutResult<Lut1D> {
    let file = File::open(path.as_ref())?;
    let reader = BufReader::new(file);
    parse_1dl(reader)
}

/// Reads a 1D LUT with metadata from a .1dl file.
pub fn read_1dl_with_info<P: AsRef<Path>>(path: P) -> LutResult<(Lut1D, Discreet1DLInfo)> {
    let file = File::open(path.as_ref())?;
    let reader = BufReader::new(file);
    parse_1dl_with_info(reader)
}

/// Parses a 1D LUT from a .1dl reader.
pub fn parse_1dl<R: BufRead>(reader: R) -> LutResult<Lut1D> {
    let (lut, _info) = parse_1dl_with_info(reader)?;
    Ok(lut)
}

/// Parses a 1D LUT with metadata from a .1dl reader.
pub fn parse_1dl_with_info<R: BufRead>(reader: R) -> LutResult<(Lut1D, Discreet1DLInfo)> {
    let mut lines: Vec<String> = Vec::new();
    let mut header_info: Option<(usize, usize, BitDepth)> = None;

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();

        // Skip empty lines and comments (# at start)
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Check for header (case-insensitive)
        // `get`, not `[..4]`: byte 4 need not be a char boundary (non-ASCII data line).
        if trimmed
            .get(..4)
            .is_some_and(|p| p.eq_ignore_ascii_case("LUT:"))
        {
            header_info = Some(parse_header(trimmed)?);
            continue;
        }

        // Collect data lines
        lines.push(trimmed.to_string());
    }

    if lines.is_empty() {
        return Err(LutError::ParseError("empty 1DL file".into()));
    }

    // Determine format based on header or data
    let (num_tables, length, dst_depth) = if let Some(info) = header_info {
        info
    } else {
        // Old format: infer from line count
        let count = lines.len();
        let depth = BitDepth::from_length(count)
            .ok_or_else(|| LutError::ParseError(format!("invalid entry count {count}")))?;
        (1, count, depth)
    };

    // Parse values
    let values = parse_values(&lines, num_tables, length, dst_depth)?;

    // Build LUT
    let lut = match num_tables {
        1 => Lut1D::from_data(values[0].clone(), 0.0, 1.0)?,
        3 | 4 => {
            // For RGBA, we ignore alpha channel in LUT1D
            Lut1D::from_rgb(
                values[0].clone(),
                values[1].clone(),
                values[2].clone(),
                0.0,
                1.0,
            )?
        }
        _ => {
            return Err(LutError::ParseError(format!(
                "invalid numtables {num_tables}"
            )));
        }
    };

    let info = Discreet1DLInfo {
        num_tables,
        length,
        dst_depth,
    };

    Ok((lut, info))
}

/// Parse header line: "LUT: numtables length [dstDepth]"
fn parse_header(line: &str) -> LutResult<(usize, usize, BitDepth)> {
    let parts: Vec<&str> = line.split_whitespace().collect();

    if parts.len() < 3 {
        return Err(LutError::ParseError("invalid LUT header".into()));
    }

    // parts[0] = "LUT:"
    let num_tables: usize = parts[1]
        .parse()
        .map_err(|_| LutError::ParseError("invalid numtables".into()))?;

    let length: usize = parts[2]
        .parse()
        .map_err(|_| LutError::ParseError("invalid length".into()))?;

    // Validate numtables
    if !matches!(num_tables, 1 | 3 | 4) {
        return Err(LutError::ParseError(format!(
            "numtables must be 1, 3, or 4, got {num_tables}"
        )));
    }

    // Validate length (standard sizes, but allow others for flexibility)
    // OCIO only allows 256/1024/4096/65536, but we accept any power of 2 >= 4
    if length < 4 || !length.is_power_of_two() {
        return Err(LutError::ParseError(format!(
            "length must be a power of 2 >= 4, got {length}"
        )));
    }
    // Upper bound before `length` sizes the per-table allocations.
    let length = crate::limits::check_1d_length("Discreet 1DL", length)?;

    // Parse optional dstDepth
    let dst_depth = if parts.len() >= 4 {
        BitDepth::parse(parts[3])
            .ok_or_else(|| LutError::ParseError(format!("invalid dstDepth: {}", parts[3])))?
    } else {
        // Default: infer from length
        BitDepth::from_length(length).unwrap_or(BitDepth::Int10)
    };

    Ok((num_tables, length, dst_depth))
}

/// Parse value lines into channel vectors.
fn parse_values(
    lines: &[String],
    num_tables: usize,
    length: usize,
    depth: BitDepth,
) -> LutResult<Vec<Vec<f32>>> {
    let max_val = depth.max_int() as f32;
    let is_float = depth.is_float();

    let mut channels: Vec<Vec<f32>> = (0..num_tables)
        .map(|_| Vec::with_capacity(length))
        .collect();

    // Parse based on format
    if num_tables == 1 {
        // Mono: one value per line
        for (i, line) in lines.iter().enumerate() {
            if i >= length {
                break;
            }
            let val = parse_value(line, is_float, max_val)?;
            channels[0].push(val);
        }
    } else {
        // RGB/RGBA: can be interleaved (one RGB per line) or separate blocks
        let values_per_line = count_values(&lines[0]);

        if values_per_line >= num_tables {
            // Interleaved: "R G B [A]" per line
            for (i, line) in lines.iter().enumerate() {
                if i >= length {
                    break;
                }
                let parts: Vec<&str> = line.split_whitespace().collect();
                for (ch, part) in parts.iter().enumerate().take(num_tables) {
                    let val = parse_single_value(part, is_float, max_val)?;
                    channels[ch].push(val);
                }
            }
        } else {
            // Separate blocks: all R, then all G, then all B, [then all A]
            let mut idx = 0;
            for channel in &mut channels {
                for _ in 0..length {
                    if idx >= lines.len() {
                        return Err(LutError::ParseError("not enough data".into()));
                    }
                    let val = parse_value(&lines[idx], is_float, max_val)?;
                    channel.push(val);
                    idx += 1;
                }
            }
        }
    }

    // Validate channel sizes
    for (i, ch) in channels.iter().enumerate() {
        if ch.len() != length {
            return Err(LutError::ParseError(format!(
                "channel {i} has {} entries, expected {length}",
                ch.len()
            )));
        }
    }

    Ok(channels)
}

/// Count values on a line.
fn count_values(line: &str) -> usize {
    line.split_whitespace().count()
}

/// Parse a single value from a line.
fn parse_value(line: &str, is_float: bool, max_val: f32) -> LutResult<f32> {
    let trimmed = line.trim();
    parse_single_value(trimmed, is_float, max_val)
}

/// Parse a single numeric string.
fn parse_single_value(s: &str, is_float: bool, max_val: f32) -> LutResult<f32> {
    if is_float {
        s.parse::<f32>()
            .map_err(|_| LutError::ParseError(format!("invalid float: {s}")))
    } else {
        let int_val: u32 = s
            .parse()
            .map_err(|_| LutError::ParseError(format!("invalid integer: {s}")))?;
        Ok(int_val as f32 / max_val)
    }
}

/// Writes a 1D LUT to a .1dl file.
///
/// Uses RGB format with 10-bit output by default.
pub fn write_1dl<P: AsRef<Path>>(path: P, lut: &Lut1D) -> LutResult<()> {
    let depth = BitDepth::from_length(lut.size()).unwrap_or(BitDepth::Int10);
    write_1dl_with_options(path, lut, depth, false)
}

/// Writes a 1D LUT to a .1dl file with options.
///
/// # Arguments
///
/// * `path` - Output file path
/// * `lut` - The 1D LUT to write
/// * `depth` - Output bit depth
/// * `interleaved` - If true, write RGB values per line; if false, write separate blocks
pub fn write_1dl_with_options<P: AsRef<Path>>(
    path: P,
    lut: &Lut1D,
    depth: BitDepth,
    interleaved: bool,
) -> LutResult<()> {
    let file = File::create(path.as_ref())?;
    let mut writer = BufWriter::new(file);

    let size = lut.size();
    let num_tables = if lut.is_mono() { 1 } else { 3 };
    let max_val = depth.max_int() as f32;
    let is_float = depth.is_float();

    // Write header
    writeln!(writer, "LUT: {} {} {}", num_tables, size, depth.as_str())?;

    // Get channel data
    let r = &lut.r;
    let g = lut.g.as_ref().unwrap_or(&lut.r);
    let b = lut.b.as_ref().unwrap_or(&lut.r);

    if num_tables == 1 {
        // Mono: one value per line
        for &val in r.iter() {
            write_value(&mut writer, val, is_float, max_val)?;
            writeln!(writer)?;
        }
    } else if interleaved {
        // Interleaved RGB
        for i in 0..size {
            write_value(&mut writer, r[i], is_float, max_val)?;
            write!(writer, " ")?;
            write_value(&mut writer, g[i], is_float, max_val)?;
            write!(writer, " ")?;
            write_value(&mut writer, b[i], is_float, max_val)?;
            writeln!(writer)?;
        }
    } else {
        // Separate blocks
        for &val in r.iter() {
            write_value(&mut writer, val, is_float, max_val)?;
            writeln!(writer)?;
        }
        for &val in g.iter() {
            write_value(&mut writer, val, is_float, max_val)?;
            writeln!(writer)?;
        }
        for &val in b.iter() {
            write_value(&mut writer, val, is_float, max_val)?;
            writeln!(writer)?;
        }
    }

    Ok(())
}

/// Write a single value.
fn write_value<W: Write>(writer: &mut W, val: f32, is_float: bool, max_val: f32) -> LutResult<()> {
    if is_float {
        write!(writer, "{:.6}", val)?;
    } else {
        let int_val = (val.clamp(0.0, 1.0) * max_val).round() as u32;
        write!(writer, "{}", int_val)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parse_old_format_8bit() {
        // 256 entries = 8-bit old format
        let mut content = String::new();
        for i in 0..256 {
            content.push_str(&format!("{}\n", i));
        }

        let reader = Cursor::new(content);
        let lut = parse_1dl(reader).expect("parse failed");

        assert_eq!(lut.size(), 256);
        assert!(lut.is_mono());

        // Check endpoints
        assert!((lut.r[0] - 0.0).abs() < 0.01);
        assert!((lut.r[255] - 1.0).abs() < 0.01);
        // Check midpoint (127/255 ~ 0.498)
        assert!((lut.r[127] - 0.498).abs() < 0.01);
    }

    #[test]
    fn parse_new_format_mono() {
        let mut content = String::from("LUT: 1 256 8\n");
        for i in 0..256 {
            content.push_str(&format!("{}\n", i));
        }

        let reader = Cursor::new(content);
        let (lut, info) = parse_1dl_with_info(reader).expect("parse failed");

        assert_eq!(info.num_tables, 1);
        assert_eq!(info.length, 256);
        assert_eq!(info.dst_depth, BitDepth::Int8);
        assert_eq!(lut.size(), 256);
    }

    #[test]
    fn parse_new_format_rgb_interleaved() {
        let mut content = String::from("LUT: 3 4 10\n");
        // 4 entries, 10-bit
        content.push_str("0 0 0\n");
        content.push_str("341 341 341\n");
        content.push_str("682 682 682\n");
        content.push_str("1023 1023 1023\n");

        let reader = Cursor::new(content);
        let (lut, info) = parse_1dl_with_info(reader).expect("parse failed");

        assert_eq!(info.num_tables, 3);
        assert_eq!(info.length, 4);
        assert!(!lut.is_mono());
        assert_eq!(lut.size(), 4);

        // Check values (should be roughly 0, 1/3, 2/3, 1)
        assert!((lut.r[0] - 0.0).abs() < 0.01);
        assert!((lut.r[3] - 1.0).abs() < 0.01);
    }

    #[test]
    fn parse_new_format_float() {
        let mut content = String::from("LUT: 1 4 32f\n");
        content.push_str("0.0\n");
        content.push_str("0.333333\n");
        content.push_str("0.666666\n");
        content.push_str("1.0\n");

        let reader = Cursor::new(content);
        let (lut, info) = parse_1dl_with_info(reader).expect("parse failed");

        assert_eq!(info.dst_depth, BitDepth::Float32);
        assert!((lut.r[0] - 0.0).abs() < 0.001);
        assert!((lut.r[3] - 1.0).abs() < 0.001);
    }

    #[test]
    fn roundtrip_mono() {
        let lut = Lut1D::gamma(256, 2.2);
        // `tempfile::tempdir()` (not `std::env::temp_dir()`): a per-call
        // unique, auto-cleaned directory instead of a fixed filename in the
        // shared OS temp dir — the old fixed name collided under
        // concurrent/parallel test runs, and the manual `remove_file` below
        // it never ran if an assertion above it panicked.
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let path = temp_dir.path().join("test_1dl_mono.1dl");

        write_1dl(&path, &lut).expect("write failed");
        let loaded = read_1dl(&path).expect("read failed");

        assert_eq!(loaded.size(), 256);

        // Check gamma curve preserved (with some quantization error)
        let test_idx = 128; // mid-point
        let expected = lut.r[test_idx];
        let actual = loaded.r[test_idx];
        assert!(
            (expected - actual).abs() < 0.01,
            "gamma mismatch at {test_idx}: expected {expected}, got {actual}"
        );
    }

    #[test]
    fn roundtrip_rgb() {
        // Create RGB LUT with different curves per channel
        let size = 256;
        let r: Vec<f32> = (0..size).map(|i| (i as f32 / 255.0).powf(2.2)).collect();
        let g: Vec<f32> = (0..size).map(|i| (i as f32 / 255.0).powf(2.0)).collect();
        let b: Vec<f32> = (0..size).map(|i| (i as f32 / 255.0).powf(1.8)).collect();

        let lut = Lut1D::from_rgb(r, g, b, 0.0, 1.0).expect("create failed");

        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let path = temp_dir.path().join("test_1dl_rgb.1dl");

        write_1dl_with_options(&path, &lut, BitDepth::Int10, true).expect("write failed");
        let loaded = read_1dl(&path).expect("read failed");

        assert_eq!(loaded.size(), 256);
        assert!(!loaded.is_mono());

        // Per-channel gammas must survive the 10-bit round-trip (tol ~1/1023).
        // idx 128: (128/255)^gamma -> r^2.2=0.21952, g^2.0=0.25196, b^1.8=0.28920.
        let mid = 128;
        let tol = 0.002; // 10-bit quantization headroom
        assert!((loaded.r[mid] - (128.0f32 / 255.0).powf(2.2)).abs() < tol);
        assert!((loaded.g.as_ref().unwrap()[mid] - (128.0f32 / 255.0).powf(2.0)).abs() < tol);
        assert!((loaded.b.as_ref().unwrap()[mid] - (128.0f32 / 255.0).powf(1.8)).abs() < tol);
        // Channels must stay distinct (the whole point of per-channel curves).
        assert!((loaded.r[mid] - loaded.g.as_ref().unwrap()[mid]).abs() > 0.02);
    }

    #[test]
    fn bit_depth_from_length() {
        assert_eq!(BitDepth::from_length(256), Some(BitDepth::Int8));
        assert_eq!(BitDepth::from_length(1024), Some(BitDepth::Int10));
        assert_eq!(BitDepth::from_length(4096), Some(BitDepth::Int12));
        assert_eq!(BitDepth::from_length(65536), Some(BitDepth::Int16));
        assert_eq!(BitDepth::from_length(512), None);
    }

    #[test]
    fn bit_depth_parse() {
        assert_eq!(BitDepth::parse("8"), Some(BitDepth::Int8));
        assert_eq!(BitDepth::parse("16f"), Some(BitDepth::Float16));
        assert_eq!(BitDepth::parse("32F"), Some(BitDepth::Float32));
        assert_eq!(BitDepth::parse("invalid"), None);
    }
}
