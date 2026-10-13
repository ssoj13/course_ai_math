//! Sony Pictures Imageworks LUT formats (SPI1D, SPI3D).
//!
//! These formats are used by Sony Pictures Imageworks' OpenColorIO pipeline
//! and other VFX tools. They are simple, human-readable text formats.
//!
//! # SPI1D Format
//!
//! 1-dimensional lookup table with the following structure:
//!
//! ```text
//! Version 1
//! From 0.0 1.0
//! Length 1024
//! Components 3
//! {
//!   0.000000 0.000000 0.000000
//!   0.001000 0.001000 0.001000
//!   ...
//! }
//! ```
//!
//! # SPI3D Format
//!
//! 3-dimensional lookup table with the following structure:
//!
//! ```text
//! SPILUT 1.0
//! 3 3
//! 32 32 32
//! 0 0 0 0.000000 0.000000 0.000000
//! 1 0 0 0.033333 0.000000 0.000000
//! ...
//! ```
//!
//! # Example
//!
//! ```rust,no_run
//! use vfx_lut::spi::{read_spi1d, read_spi3d};
//! use std::path::Path;
//!
//! // Read a 1D LUT
//! let lut1d = read_spi1d(Path::new("gamma.spi1d")).unwrap();
//!
//! // Read a 3D LUT
//! let lut3d = read_spi3d(Path::new("grade.spi3d")).unwrap();
//! ```
//!
//! # References
//!
//! - [OpenColorIO SPI1D](https://opencolorio.readthedocs.io/en/latest/guides/authoring/luts.html)
//! - [OpenColorIO SPI3D](https://opencolorio.readthedocs.io/en/latest/guides/authoring/luts.html)

use crate::{Lut1D, Lut3D, LutError, LutResult};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

/// Reads an SPI1D file from disk.
///
/// # Arguments
///
/// * `path` - Path to the .spi1d file
///
/// # Format
///
/// SPI1D files contain a header with version, domain range, length,
/// and number of components, followed by the LUT data in braces.
///
/// # Errors
///
/// Returns error if file cannot be read or has invalid format.
///
/// # Example
///
/// ```rust,no_run
/// use vfx_lut::spi::read_spi1d;
/// use std::path::Path;
///
/// let lut = read_spi1d(Path::new("gamma.spi1d")).unwrap();
/// let output = lut.apply(0.5);
/// ```
pub fn read_spi1d(path: &Path) -> LutResult<Lut1D> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    parse_spi1d(reader)
}

/// Parses SPI1D from a reader.
///
/// # Arguments
///
/// * `reader` - Any type implementing `BufRead`
pub fn parse_spi1d<R: BufRead>(reader: R) -> LutResult<Lut1D> {
    let mut version_seen = false;
    let mut from_min = 0.0f32;
    let mut from_max = 1.0f32;
    let mut length: Option<usize> = None;
    let mut components: Option<usize> = None;
    let mut in_data = false;
    let mut r_data: Vec<f32> = Vec::new();
    let mut g_data: Vec<f32> = Vec::new();
    let mut b_data: Vec<f32> = Vec::new();

    for line in reader.lines() {
        let line = line?;
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Data section
        if line == "{" {
            in_data = true;
            continue;
        }
        if line == "}" {
            in_data = false;
            continue;
        }

        if in_data {
            let comps = components.ok_or_else(|| {
                LutError::ParseError("SPI1D: data line appears before 'Components' header".into())
            })?;
            let len = length.ok_or_else(|| {
                LutError::ParseError("SPI1D: data line appears before 'Length' header".into())
            })?;

            // FileFormatSpi1D.cpp:222-241: a data line must have exactly `components` tokens,
            // each a valid float; a mismatch throws "Malformed LUT line" rather than being
            // silently dropped or padded.
            let tokens: Vec<&str> = line.split_whitespace().collect();
            if tokens.len() != comps {
                return Err(LutError::ParseError(format!(
                    "SPI1D: malformed LUT line, expected {comps} component(s), found {}: {line:?}",
                    tokens.len()
                )));
            }
            let mut values = [0.0f32; 3];
            for (i, tok) in tokens.iter().enumerate() {
                values[i] = tok.parse().map_err(|_| {
                    LutError::ParseError(format!(
                        "SPI1D: malformed LUT line, component {i} ({tok:?}) is not a float"
                    ))
                })?;
            }

            // FileFormatSpi1D.cpp:243-246: too many entries is an immediate error, not a
            // silently-truncated LUT.
            if r_data.len() >= len {
                return Err(LutError::ParseError(format!(
                    "SPI1D: too many entries found, expected {len}"
                )));
            }

            match comps {
                1 => r_data.push(values[0]),
                3 => {
                    r_data.push(values[0]);
                    g_data.push(values[1]);
                    b_data.push(values[2]);
                }
                // The header parser already rejects anything but 1 or 3, so this arm is
                // unreachable today — but it stays an error rather than `unreachable!` because a
                // panic reachable from parsing untrusted input is a bug even when the invariant
                // that protects it lives in another function.
                other => {
                    return Err(LutError::Unsupported {
                        format: "SPI1D",
                        feature: "Components",
                        detail: format!("{other} components"),
                    });
                }
            }
        } else {
            // Header parsing
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }

            match parts[0].to_lowercase().as_str() {
                "version" => {
                    let v: u32 = parts.get(1).and_then(|s| s.parse().ok()).ok_or_else(|| {
                        LutError::ParseError("SPI1D: invalid 'Version' tag".into())
                    })?;
                    // FileFormatSpi1D.cpp:128-131: only format version 1 is supported.
                    if v != 1 {
                        return Err(LutError::Unsupported {
                            format: "SPI1D",
                            feature: "Version",
                            detail: format!("only version 1 is supported, found {v}"),
                        });
                    }
                    version_seen = true;
                }
                "from" => {
                    if parts.len() < 3 {
                        return Err(LutError::ParseError("SPI1D: invalid 'From' tag".into()));
                    }
                    from_min = parts[1]
                        .parse()
                        .map_err(|_| LutError::ParseError("SPI1D: invalid 'From' tag".into()))?;
                    from_max = parts[2]
                        .parse()
                        .map_err(|_| LutError::ParseError("SPI1D: invalid 'From' tag".into()))?;
                }
                "length" => {
                    let l: usize = parts.get(1).and_then(|s| s.parse().ok()).ok_or_else(|| {
                        LutError::ParseError("SPI1D: invalid 'Length' tag".into())
                    })?;
                    // FileFormatSpi1D.cpp:182-187 rejects Length outside [2, Max1DLUTLength].
                    let l = crate::limits::check_1d_length("SPI1D", l)?;
                    length = Some(l);
                }
                "components" => {
                    let c: usize = parts.get(1).and_then(|s| s.parse().ok()).ok_or_else(|| {
                        LutError::ParseError("SPI1D: invalid 'Components' tag".into())
                    })?;
                    // This crate's Lut1D models mono (1 channel, shared curve) or RGB (3
                    // independent channels) only. `Components 2` is valid in the OCIO spec
                    // (padded with 0 for blue) but has no representation here; the previous
                    // code fell through both branches on `2` and silently produced an empty
                    // LUT for every data line, so this must fail loudly and by name instead.
                    if c != 1 && c != 3 {
                        return Err(LutError::Unsupported {
                            format: "SPI1D",
                            feature: "Components",
                            detail: format!("{c} (only 1 or 3 supported)"),
                        });
                    }
                    components = Some(c);
                }
                other => {
                    return Err(LutError::UnknownToken {
                        format: "SPI1D",
                        kind: "header keyword",
                        token: other.to_string(),
                        line: None,
                    });
                }
            }
        }
    }

    if !version_seen {
        return Err(LutError::ParseError("SPI1D: missing 'Version' tag".into()));
    }
    let length =
        length.ok_or_else(|| LutError::ParseError("SPI1D: missing 'Length' tag".into()))?;
    let components =
        components.ok_or_else(|| LutError::ParseError("SPI1D: missing 'Components' tag".into()))?;

    // FileFormatSpi1D.cpp:303-306: fewer data lines than declared is also an error.
    if r_data.len() != length {
        return Err(LutError::ParseError(format!(
            "SPI1D: not enough entries found, expected {length}, got {}",
            r_data.len()
        )));
    }

    if components == 1 {
        Lut1D::from_data(r_data, from_min, from_max)
    } else {
        Lut1D::from_rgb(r_data, g_data, b_data, from_min, from_max)
    }
}

/// Writes an SPI1D file to disk.
///
/// # Arguments
///
/// * `path` - Output path for the .spi1d file
/// * `lut` - The 1D LUT to write
///
/// # Errors
///
/// Returns error if file cannot be written.
///
/// # Example
///
/// ```rust,no_run
/// use vfx_lut::{Lut1D, spi::write_spi1d};
/// use std::path::Path;
///
/// let lut = Lut1D::gamma(1024, 2.2);
/// write_spi1d(Path::new("gamma.spi1d"), &lut).unwrap();
/// ```
pub fn write_spi1d(path: &Path, lut: &Lut1D) -> LutResult<()> {
    let file = File::create(path)?;
    let writer = BufWriter::new(file);
    write_spi1d_to(writer, lut)
}

/// Writes SPI1D to any writer.
pub fn write_spi1d_to<W: Write>(mut writer: W, lut: &Lut1D) -> LutResult<()> {
    // `g` and `b` are independent Options, so a half-populated Lut1D (g set, b not) would have
    // panicked on the `b` unwrap below. Require both, and require matching lengths, before
    // committing to the 3-component header — the per-sample writes index all three channels.
    let rgb = match (lut.g.as_ref(), lut.b.as_ref()) {
        (Some(g), Some(b)) if g.len() == lut.r.len() && b.len() == lut.r.len() => Some((g, b)),
        (None, None) => None,
        (g, b) => {
            return Err(LutError::InvalidSize(format!(
                "SPI1D write: channel lengths must match r={} (g={:?}, b={:?})",
                lut.r.len(),
                g.map(|c| c.len()),
                b.map(|c| c.len())
            )));
        }
    };
    let is_rgb = rgb.is_some();
    let components = if is_rgb { 3 } else { 1 };

    // Header
    writeln!(writer, "Version 1")?;
    // SPI1D uses scalar domain; use R channel (assume uniform)
    writeln!(writer, "From {} {}", lut.domain_min[0], lut.domain_max[0])?;
    writeln!(writer, "Length {}", lut.size())?;
    writeln!(writer, "Components {}", components)?;
    writeln!(writer, "{{")?;

    // Data
    if let Some((g, b)) = rgb {
        for ((r, g), b) in lut.r.iter().zip(g.iter()).zip(b.iter()) {
            writeln!(writer, "  {r:.6} {g:.6} {b:.6}")?;
        }
    } else {
        for v in &lut.r {
            writeln!(writer, "  {:.6}", v)?;
        }
    }

    writeln!(writer, "}}")?;

    Ok(())
}

/// Reads an SPI3D file from disk.
///
/// # Arguments
///
/// * `path` - Path to the .spi3d file
///
/// # Format
///
/// SPI3D files contain a header with version and size info,
/// followed by grid index and RGB output values per line.
///
/// # Errors
///
/// Returns error if file cannot be read or has invalid format.
///
/// # Example
///
/// ```rust,no_run
/// use vfx_lut::spi::read_spi3d;
/// use std::path::Path;
///
/// let lut = read_spi3d(Path::new("grade.spi3d")).unwrap();
/// let output = lut.apply([0.5, 0.3, 0.2]);
/// ```
pub fn read_spi3d(path: &Path) -> LutResult<Lut3D> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    parse_spi3d(reader)
}

/// Parses SPI3D from a reader.
///
/// # Arguments
///
/// * `reader` - Any type implementing `BufRead`
pub fn parse_spi3d<R: BufRead>(reader: R) -> LutResult<Lut3D> {
    let mut meaningful: Vec<String> = Vec::new();
    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        meaningful.push(trimmed.to_string());
    }
    let mut lines = meaningful.into_iter();

    // FileFormatSpi3D.cpp:90-100: "SPILUT" magic is required there, but this parser has
    // long treated it as optional; keep that leniency; if present, consume it.
    let mut header = lines
        .next()
        .ok_or_else(|| LutError::ParseError("SPI3D: empty file".into()))?;
    if header.to_uppercase().starts_with("SPILUT") {
        header = lines
            .next()
            .ok_or_else(|| LutError::ParseError("SPI3D: missing dimensionality line".into()))?;
    }
    // `header` is now the "3 3" dimensionality line. The reference never actually validates
    // its content either (FileFormatSpi3D.cpp:102: "TODO: Assert 2nd line is 3 3") — it is
    // read and discarded so the following line lines up as the size line.
    let _ = header;

    let size_line = lines
        .next()
        .ok_or_else(|| LutError::ParseError("SPI3D: missing size line".into()))?;
    let size_parts: Vec<&str> = size_line.split_whitespace().collect();
    if size_parts.len() != 3 {
        return Err(LutError::ParseError(format!(
            "SPI3D: expected 3 size values, found {:?}",
            size_line
        )));
    }
    let mut dims = [0usize; 3];
    for (i, tok) in size_parts.iter().enumerate() {
        dims[i] = tok.parse().map_err(|_| {
            LutError::ParseError(format!(
                "SPI3D: invalid size value {tok:?} in {size_line:?}"
            ))
        })?;
    }
    let (r_size, g_size, b_size) = (dims[0], dims[1], dims[2]);

    // COLOR-M2: FileFormatSpi3D.cpp:119-129 rejects non-uniform sizes outright rather than
    // silently taking dims[0] and hoping the other two axes agree.
    if r_size != g_size || r_size != b_size {
        return Err(LutError::SizeOutOfRange {
            format: "SPI3D",
            dims: vec![r_size as u64, g_size as u64, b_size as u64],
            reason: "LUT size must be identical for all three axes".into(),
        });
    }
    let size = r_size;
    // FileFormatSpi3D.cpp:131-140 rejects a declared size outside [2, Max3DLUTLength].
    let total = crate::limits::check_3d_size("SPI3D", size)?;

    let mut data = vec![[0.0f32; 3]; total];
    let mut defined = vec![false; total];
    let mut remaining = total;

    for line in lines {
        let parts: Vec<&str> = line.split_whitespace().collect();
        // FileFormatSpi3D.cpp:158-249: a data line is `idxR idxG idxB r g b` (6 fields via
        // sscanf); anything that doesn't match is silently skipped, exactly like the
        // reference — the eventual "not enough entries" check below catches truncation.
        if parts.len() < 6 {
            continue;
        }
        let (Ok(ri), Ok(gi), Ok(bi)) = (
            parts[0].parse::<i64>(),
            parts[1].parse::<i64>(),
            parts[2].parse::<i64>(),
        ) else {
            continue;
        };
        let (Ok(r), Ok(g), Ok(b)) = (
            parts[3].parse::<f32>(),
            parts[4].parse::<f32>(),
            parts[5].parse::<f32>(),
        ) else {
            continue;
        };

        if ri < 0 || gi < 0 || bi < 0 {
            return Err(LutError::ParseError(format!(
                "SPI3D: negative index [{ri}, {gi}, {bi}]"
            )));
        }
        let (ri, gi, bi) = (ri as usize, gi as usize, bi as usize);
        if ri >= size || gi >= size || bi >= size {
            return Err(LutError::ParseError(format!(
                "SPI3D: index [{ri}, {gi}, {bi}] falls outside the {size}^3 cube"
            )));
        }

        // COLOR-01: address by the declared (r, g, b) triplet, not by file order. Blue-fastest
        // addressing matches GetLut3DIndex_BlueFast (Lut3DOp.h:29-33) and this crate's own
        // Lut3D::index (lut3d.rs:136-138): idx = b + size*(g + size*r).
        let idx = bi + size * (gi + size * ri);

        if defined[idx] {
            return Err(LutError::DuplicateIndex {
                format: "SPI3D",
                r: ri,
                g: gi,
                b: bi,
            });
        }
        data[idx] = [r, g, b];
        defined[idx] = true;
        remaining -= 1;
        if remaining == 0 {
            // FileFormatSpi3D.cpp comment: "after the expected number of entries is found,
            // file can contain anything" — stop reading once the cube is fully covered.
            break;
        }
    }

    if remaining > 0 {
        return Err(LutError::IncompleteCoverage {
            format: "SPI3D",
            missing: remaining,
            expected: total,
        });
    }

    Lut3D::from_data(data, size)
}

/// Writes an SPI3D file to disk.
///
/// # Arguments
///
/// * `path` - Output path for the .spi3d file
/// * `lut` - The 3D LUT to write
///
/// # Errors
///
/// Returns error if file cannot be written.
///
/// # Example
///
/// ```rust,no_run
/// use vfx_lut::{Lut3D, spi::write_spi3d};
/// use std::path::Path;
///
/// let lut = Lut3D::identity(33);
/// write_spi3d(Path::new("identity.spi3d"), &lut).unwrap();
/// ```
pub fn write_spi3d(path: &Path, lut: &Lut3D) -> LutResult<()> {
    let file = File::create(path)?;
    let writer = BufWriter::new(file);
    write_spi3d_to(writer, lut)
}

/// Writes SPI3D to any writer.
pub fn write_spi3d_to<W: Write>(mut writer: W, lut: &Lut3D) -> LutResult<()> {
    let size = lut.size;

    // Header
    writeln!(writer, "SPILUT 1.0")?;
    writeln!(writer, "3 3")?;
    writeln!(writer, "{} {} {}", size, size, size)?;

    // Data with indices
    // Memory is Blue-major: idx = B + dim*G + dim²*R
    for b in 0..size {
        for g in 0..size {
            for r in 0..size {
                // Blue-major index: B + size*G + size²*R
                let idx = b + size * (g + size * r);
                let rgb = lut.data[idx];
                writeln!(
                    writer,
                    "{} {} {} {:.6} {:.6} {:.6}",
                    r, g, b, rgb[0], rgb[1], rgb[2]
                )?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_parse_spi1d_mono() {
        let data = r#"
Version 1
From 0.0 1.0
Length 4
Components 1
{
  0.0
  0.333333
  0.666666
  1.0
}
"#;
        let lut = parse_spi1d(Cursor::new(data)).unwrap();
        assert_eq!(lut.size(), 4);
        assert!(lut.is_mono());
        // Fixture ramp [0.0, 0.333333, 0.666666, 1.0]: interior entries round-trip exactly from
        // the parsed values, and apply(0.5) (index 1.5) lerps between them.
        assert!((lut.r[1] - 0.333333).abs() < 1e-6);
        assert!((lut.r[2] - 0.666666).abs() < 1e-6);
        assert!((lut.apply(0.5) - 0.5).abs() < 1e-4);
    }

    #[test]
    fn test_parse_spi1d_rgb() {
        let data = r#"
Version 1
From 0.0 1.0
Length 3
Components 3
{
  0.0 0.0 0.0
  0.5 0.5 0.5
  1.0 1.0 1.0
}
"#;
        let lut = parse_spi1d(Cursor::new(data)).unwrap();
        assert_eq!(lut.size(), 3);
        assert!(!lut.is_mono());
        // Fixture ramp [0.0, 0.5, 1.0] over domain [0,1]: apply(0.5) hits index 1.0 == 0.5 exactly.
        assert!((lut.apply(0.5) - 0.5).abs() < 0.01);
        // Endpoints must round-trip from the parsed values, not just have the right count.
        assert!((lut.apply(0.0) - 0.0).abs() < 0.01);
        assert!((lut.apply(1.0) - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_spi1d_roundtrip() {
        let lut = Lut1D::gamma(64, 2.2);

        let mut buf = Vec::new();
        write_spi1d_to(&mut buf, &lut).unwrap();

        let parsed = parse_spi1d(Cursor::new(buf)).unwrap();

        assert_eq!(parsed.size(), 64);
        assert!((parsed.apply(0.5) - lut.apply(0.5)).abs() < 0.001);
    }

    #[test]
    fn test_parse_spi3d() {
        let data = r#"
SPILUT 1.0
3 3
2 2 2
0 0 0 0.0 0.0 0.0
1 0 0 1.0 0.0 0.0
0 1 0 0.0 1.0 0.0
1 1 0 1.0 1.0 0.0
0 0 1 0.0 0.0 1.0
1 0 1 1.0 0.0 1.0
0 1 1 0.0 1.0 1.0
1 1 1 1.0 1.0 1.0
"#;
        let lut = parse_spi3d(Cursor::new(data)).unwrap();
        assert_eq!(lut.size, 2);

        // Test black corner
        let black = lut.apply([0.0, 0.0, 0.0]);
        assert!((black[0]).abs() < 0.01);

        // Test white corner
        let white = lut.apply([1.0, 1.0, 1.0]);
        assert!((white[0] - 1.0).abs() < 0.01);

        // Interior grid point (r=1,g=0,b=0), fixture line "1 0 0 1.0 0.0 0.0": not a corner of
        // the black/white diagonal, so a transposed axis (e.g. reading it as g=1 or b=1) would
        // be caught here.
        let idx = lut.size * lut.size; // Blue-fastest memory index.
        assert_eq!(lut.data[idx], [1.0, 0.0, 0.0]);
    }

    #[test]
    fn test_spi3d_roundtrip() {
        let lut = Lut3D::identity(8);

        let mut buf = Vec::new();
        write_spi3d_to(&mut buf, &lut).unwrap();

        let parsed = parse_spi3d(Cursor::new(buf)).unwrap();

        assert_eq!(parsed.size, 8);

        // An identity LUT interpolates to itself at any continuous position, grid-aligned or
        // not (0.5 falls between grid indices 3 and 4 for size 8) -- tight tolerance, not 0.1.
        let result = parsed.apply([0.5, 0.5, 0.5]);
        assert!((result[0] - 0.5).abs() < 1e-5);
        assert!((result[1] - 0.5).abs() < 1e-5);
        assert!((result[2] - 0.5).abs() < 1e-5);

        // Interior grid point (r=3,g=5,b=2 out of 0..7): identity(8) means value == index/7.
        let idx = 2 + parsed.size * (5 + parsed.size * 3);
        let expected = [3.0 / 7.0, 5.0 / 7.0, 2.0 / 7.0];
        for c in 0..3 {
            assert!(
                (parsed.data[idx][c] - expected[c]).abs() < 1e-5,
                "channel {c}: got {:?}, expected {expected:?}",
                parsed.data[idx]
            );
        }
    }

    #[test]
    fn test_spi3d_no_indices_now_rejected() {
        // FileFormatSpi3D.cpp requires every data line to carry the "idxR idxG idxB r g b"
        // sscanf-matched sextuple (FileFormatSpi3D.cpp:158-249); a 3-value-only line simply
        // never matches, so the grid position it should have filled stays undefined and
        // "not enough entries" fires once the file is exhausted. The old parser treated
        // index-less files as a supported dialect (fell back to file order); that dialect
        // doesn't exist in the reference, so this must now be a hard error rather than a
        // silently-accepted parse.
        let data = r#"
SPILUT 1.0
3 3
2 2 2
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
"#;
        let err = parse_spi3d(Cursor::new(data)).unwrap_err();
        assert!(
            matches!(err, LutError::IncompleteCoverage { .. }),
            "expected IncompleteCoverage, got {err:?}"
        );
    }

    #[test]
    fn test_parse_spi3d_shuffled_order_addresses_by_index() {
        // COLOR-01: data lines deliberately NOT in blue-fastest file order. Each of the 8
        // corners of a 2^3 cube gets a distinct, index-derived output value
        // (value = [0.1+0.1r, 0.2+0.1g, 0.3+0.1b]) so that addressing samples by file
        // position instead of by their (r,g,b) triplet lands the wrong value in the wrong
        // grid cell. Against the pre-fix parser (which pushed samples in file order and
        // assumed that was already blue-fastest memory order) this fails; after the fix,
        // querying each corner returns exactly the value written for that corner's index.
        let data = r#"
SPILUT 1.0
3 3
2 2 2
1 1 1 0.2 0.3 0.4
0 0 0 0.1 0.2 0.3
1 0 1 0.2 0.2 0.4
0 1 0 0.1 0.3 0.3
1 1 0 0.2 0.3 0.3
0 0 1 0.1 0.2 0.4
1 0 0 0.2 0.2 0.3
0 1 1 0.1 0.3 0.4
"#;
        let lut = parse_spi3d(Cursor::new(data)).unwrap();
        assert_eq!(lut.size, 2);

        for r in 0..2usize {
            for g in 0..2usize {
                for b in 0..2usize {
                    let input = [r as f32, g as f32, b as f32];
                    let expected = [
                        0.1 + 0.1 * r as f32,
                        0.2 + 0.1 * g as f32,
                        0.3 + 0.1 * b as f32,
                    ];
                    let got = lut.apply(input);
                    for c in 0..3 {
                        assert!(
                            (got[c] - expected[c]).abs() < 1e-4,
                            "corner ({r},{g},{b}) channel {c}: got {got:?}, expected {expected:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn test_parse_spi3d_duplicate_index_rejected() {
        let data = r#"
SPILUT 1.0
3 3
2 2 2
0 0 0 0.0 0.0 0.0
0 0 0 1.0 1.0 1.0
1 0 0 1.0 0.0 0.0
0 1 0 0.0 1.0 0.0
1 1 0 1.0 1.0 0.0
0 0 1 0.0 0.0 1.0
1 0 1 1.0 0.0 1.0
0 1 1 0.0 1.0 1.0
1 1 1 1.0 1.0 1.0
"#;
        let err = parse_spi3d(Cursor::new(data)).unwrap_err();
        assert!(
            matches!(
                err,
                LutError::DuplicateIndex {
                    r: 0,
                    g: 0,
                    b: 0,
                    ..
                }
            ),
            "expected DuplicateIndex(0,0,0), got {err:?}"
        );
    }

    #[test]
    fn test_parse_spi3d_incomplete_coverage_rejected() {
        // Declares a 2^3 cube (8 entries) but supplies only 7 — one grid position is
        // never written.
        let data = r#"
SPILUT 1.0
3 3
2 2 2
0 0 0 0.0 0.0 0.0
1 0 0 1.0 0.0 0.0
0 1 0 0.0 1.0 0.0
1 1 0 1.0 1.0 0.0
0 0 1 0.0 0.0 1.0
1 0 1 1.0 0.0 1.0
0 1 1 0.0 1.0 1.0
"#;
        let err = parse_spi3d(Cursor::new(data)).unwrap_err();
        match err {
            LutError::IncompleteCoverage {
                missing, expected, ..
            } => {
                assert_eq!(missing, 1);
                assert_eq!(expected, 8);
            }
            other => panic!("expected IncompleteCoverage, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_spi3d_non_uniform_size_rejected() {
        // COLOR-M2: "32 16 8" must be rejected outright, not silently truncated to
        // parts[0] == 32.
        let data = "SPILUT 1.0\n3 3\n32 16 8\n0 0 0 0.0 0.0 0.0\n";
        let err = parse_spi3d(Cursor::new(data)).unwrap_err();
        assert!(
            matches!(err, LutError::SizeOutOfRange { .. }),
            "expected SizeOutOfRange, got {err:?}"
        );
    }

    #[test]
    fn test_parse_spi3d_hostile_size_rejected_not_oom() {
        // A cube size of 129 is the OCIO Max3DLUTLength boundary (accepted); one past it,
        // or an absurdly large hostile value, must be a clean, immediate error rather than
        // an attempt to allocate size^3 entries.
        let too_big = "SPILUT 1.0\n3 3\n130 130 130\n";
        let err = parse_spi3d(Cursor::new(too_big)).unwrap_err();
        assert!(matches!(err, LutError::SizeOutOfRange { .. }));

        let absurd = "SPILUT 1.0\n3 3\n999999999 999999999 999999999\n";
        let err = parse_spi3d(Cursor::new(absurd)).unwrap_err();
        assert!(matches!(err, LutError::SizeOutOfRange { .. }));
    }

    #[test]
    fn test_parse_spi3d_size_cap_is_129_not_256() {
        // The old code accepted a declared size of, e.g., 200 (<= its stray 256 cap); the
        // real OCIO limit (Max3DLUTLength, LutLimits.h:15) is 129. A size in (129, 256] must
        // now be rejected.
        let data = "SPILUT 1.0\n3 3\n200 200 200\n";
        let err = parse_spi3d(Cursor::new(data)).unwrap_err();
        assert!(matches!(err, LutError::SizeOutOfRange { .. }));
    }

    #[test]
    fn test_spi1d_components_2_is_unsupported_not_misparsed() {
        // Components 2 is valid per the OCIO spec but has no representation in this
        // crate's Lut1D (mono or RGB only); the old code silently produced an empty LUT
        // (`r_data` stays empty) for every data line instead of erroring.
        let data = "Version 1\nFrom 0.0 1.0\nLength 2\nComponents 2\n{\n0.0 0.0\n1.0 1.0\n}\n";
        let err = parse_spi1d(Cursor::new(data)).unwrap_err();
        assert!(
            matches!(
                err,
                LutError::Unsupported {
                    feature: "Components",
                    ..
                }
            ),
            "expected Unsupported(Components), got {err:?}"
        );
    }

    #[test]
    fn test_spi1d_bad_version_rejected() {
        let data = "Version 2\nFrom 0.0 1.0\nLength 2\nComponents 1\n{\n0.0\n1.0\n}\n";
        let err = parse_spi1d(Cursor::new(data)).unwrap_err();
        assert!(matches!(
            err,
            LutError::Unsupported {
                feature: "Version",
                ..
            }
        ));
    }

    #[test]
    fn test_spi1d_malformed_data_line_rejected() {
        // A line with a non-numeric token must error, not be silently dropped from the LUT
        // (the old `filter_map(...ok())` absorbed it, shrinking the LUT below `Length`).
        let data = "Version 1\nFrom 0.0 1.0\nLength 2\nComponents 1\n{\n0.0\nNaNsense\n}\n";
        let err = parse_spi1d(Cursor::new(data)).unwrap_err();
        assert!(matches!(err, LutError::ParseError(_)));
    }

    #[test]
    fn test_spi1d_length_mismatch_rejected() {
        let data = "Version 1\nFrom 0.0 1.0\nLength 3\nComponents 1\n{\n0.0\n1.0\n}\n";
        let err = parse_spi1d(Cursor::new(data)).unwrap_err();
        assert!(matches!(err, LutError::ParseError(_)));
    }
}
