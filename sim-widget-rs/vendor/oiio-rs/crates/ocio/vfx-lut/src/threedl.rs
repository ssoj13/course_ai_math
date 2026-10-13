//! Autodesk/Discreet .3dl LUT format support.
//!
//! The .3DL format is used by Autodesk Flame, Smoke, and Lustre.
//! It stores 3D LUT data with integer values (typically 10-bit or 12-bit), and may optionally
//! carry a 1D shaper LUT (a pre-LUT applied to the input before the 3D lookup).
//!
//! # Format
//!
//! ```text
//! # Comment (optional)
//! 0 32 64 96 128 ... 1023    (optional shaper LUT: more than 3 ints on one line)
//! 0 0 0                       (3D LUT data starts: exactly 3 ints per line)
//! 1023 0 0
//! ...
//! 1023 1023 1023
//! ```
//!
//! # Variants
//!
//! - **Lustre format**: `3DMESH` / `Mesh <in> <out>` keyword lines (ignored, non-numeric),
//!   shaper LUT, then 3D data.
//! - **Flame format**: shaper LUT directly followed by 3D data, no keyword lines.
//!
//! Both the shaper and the 3D LUT carry no explicit bit-depth field — each one's bit depth is
//! inferred independently from its own maximum sample value (reference:
//! `FileFormat3DL.cpp::GetLikelyLutBitDepth()`). The two are NOT related: a file can legally have
//! a 12-bit shaper feeding a 10-bit 3D LUT, or vice versa.
//!
//! # Example
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use vfx_lut::threedl;
//!
//! // 3D-LUT-only convenience API (errors if the file has a non-identity shaper it can't
//! // silently drop):
//! let lut = threedl::read_3dl("grade.3dl")?;
//! let rgb = lut.apply([0.5, 0.3, 0.2]);
//!
//! // Full API: carries the shaper through instead of erroring.
//! let file = threedl::read_3dl_full("grade.3dl")?;
//! if let Some(shaper) = &file.shaper {
//!     // apply shaper.apply_rgb(...) first, then file.lut3d.apply(...)
//! }
//! # Ok(())
//! # }
//! ```

use crate::{Lut1D, Lut3D, LutError, LutResult};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

/// Minimum plausible maximum code value found in a shaper or 3D LUT table.
///
/// Below this, the "LUT" is probably not really a .3dl (e.g. a small-range float format that
/// happens to parse as ints). Reference: `FileFormat3DL.cpp`,
/// `FORMAT3DL_CODEVALUE_LOWEST_PLAUSIBLE_MAXINT = 128`.
const LOWEST_PLAUSIBLE_MAXINT: i32 = 128;

// `.3dl` has no size field, so the cube size is only known after all data lines are read: the raw
// sample count is capped at MAX_3D_LUT_LENGTH^3 while streaming, so a hostile file fails fast
// instead of growing an unbounded `Vec`.
use crate::limits::MAX_3D_LUT_LENGTH;

/// A parsed `.3dl` file: an optional 1D shaper pre-LUT plus the 3D LUT.
///
/// Real `.3dl` files apply the shaper BEFORE the 3D LUT (it maps input code values into the 3D
/// LUT's domain). The two are scaled independently: Discreet's format never requires the shaper
/// and 3D LUT to share a bit depth (reference: `FileFormat3DL.cpp::read()`, which derives
/// `out1DBD` from `shapermax` and `out3DBD` from `lut3dmax` as two unrelated inferences).
#[derive(Debug, Clone, Default)]
pub struct ThreeDLFile {
    /// The shaper (1D pre-LUT), present only when the file has one that is not an identity ramp.
    pub shaper: Option<Lut1D>,
    /// The 3D LUT, when the file contains 3D data.
    pub lut3d: Option<Lut3D>,
}

/// Infers the bit depth implied by the largest code value seen in a shaper or 3D LUT table.
///
/// `.3dl` carries no explicit bit-depth field, so both OCIO and this parser reverse-engineer it
/// from the data itself, allowing up to 2x overshoot (a table is still considered N-bit as long
/// as its max sample is under `2 * (2^N - 1)`). Only 8/10/12/16-bit are ever returned; the
/// reference bumps a nominal 14-bit result up to 16-bit since 14-bit scaling isn't used in
/// practice. Returns `None` for a negative max (should not happen for values parsed as `i32`
/// maxed against 0, but guarded rather than assumed).
///
/// Reference: `FileFormat3DL.cpp::GetLikelyLutBitDepth()`.
fn likely_bit_depth(max_val: i32) -> Option<u32> {
    if max_val < 0 {
        return None;
    }
    for bits in [8u32, 10, 12, 14, 16] {
        let maxcode = 1i64 << bits;
        let adjusted_max = maxcode * 2 - 1;
        if i64::from(max_val) <= adjusted_max {
            return Some(if bits == 14 { 16 } else { bits });
        }
    }
    Some(16)
}

/// The maximum representable value for an inferred integer bit depth (e.g. 10-bit -> 1023).
fn bit_depth_scale(bits: u32) -> f32 {
    ((1u64 << bits) - 1) as f32
}

/// Whether a shaper table is close enough to a straight ramp that applying it is a no-op.
///
/// Real-world `.3dl` files commonly carry an identity shaper as boilerplate; the reference drops
/// these rather than materializing a pass-through 1D LUT, using a loose +/-2 code tolerance
/// (comment in the reference: "usage in the industry is quite inconsistent"). `raw` always has
/// more than 3 entries here (that's the classification rule that made it a shaper in the first
/// place), so `dim - 1` never underflows.
///
/// Reference: `FileFormat3DL.cpp::IsIdentity()`.
fn shaper_is_identity(raw: &[i32], scale: f32) -> bool {
    let dim = raw.len();
    let step = scale / (dim as f32 - 1.0);
    raw.iter()
        .enumerate()
        .all(|(i, &v)| ((i as f32) * step - v as f32).abs() < 2.0)
}

/// Reads a 3D LUT from a .3dl file.
///
/// Convenience wrapper over [`read_3dl_full`] for callers that only want the 3D LUT. Errors if
/// the file carries a non-identity shaper, since this signature has nowhere to put it — see
/// [`ThreeDLFile`] / [`read_3dl_full`] for the full-fidelity API.
pub fn read_3dl<P: AsRef<Path>>(path: P) -> LutResult<Lut3D> {
    let file = File::open(path.as_ref())?;
    let reader = BufReader::new(file);
    parse_3dl(reader)
}

/// Reads a `.3dl` file, carrying through both the shaper (if any) and the 3D LUT.
pub fn read_3dl_full<P: AsRef<Path>>(path: P) -> LutResult<ThreeDLFile> {
    let file = File::open(path.as_ref())?;
    let reader = BufReader::new(file);
    parse_3dl_full(reader)
}

/// Parses a 3D LUT from a .3dl reader.
///
/// Convenience wrapper over [`parse_3dl_full`]: errors with [`LutError::Unsupported`] if the file
/// has a non-identity shaper LUT, rather than silently discarding it (COLOR-04) — a `Lut3D`-only
/// return value has nowhere to carry a pre-LUT. Use [`parse_3dl_full`] for files that need it.
pub fn parse_3dl<R: BufRead>(reader: R) -> LutResult<Lut3D> {
    let file = parse_3dl_full(reader)?;
    if file.shaper.is_some() {
        return Err(LutError::Unsupported {
            format: "3DL",
            feature: "shaper (1D pre-LUT)",
            detail: "file has a non-identity shaper LUT that this Lut3D-only entry point cannot \
                      apply; use threedl::parse_3dl_full()/read_3dl_full() to get both the \
                      shaper and the 3D LUT instead of silently discarding the shaper"
                .into(),
        });
    }
    file.lut3d
        .ok_or_else(|| LutError::ParseError("3DL: file has no 3D LUT data".into()))
}

/// Parses a `.3dl` reader, carrying through both the shaper (if any) and the 3D LUT.
///
/// Follows the reference's line classification exactly (`FileFormat3DL.cpp::read()`):
/// - A line of ints with more than 3 values is the shaper LUT (at most one per file).
/// - A line of exactly 3 ints is a 3D LUT data line (blue-fastest order, same as memory layout).
/// - A line of 1 or 2 ints is malformed and rejected.
/// - A line that isn't all-integer (comments, `3DMESH`, `Mesh 4 12`, `LUT8`, `gamma 1.0`, ...) is
///   silently skipped — those keywords are valid `.3dl` syntax that carries no LUT data.
///
/// The shaper's bit depth and the 3D LUT's bit depth are inferred **independently**, each from
/// its own maximum sample value — this is the fix for COLOR-04, which previously scaled the 3D
/// data by whatever value happened to be last on the first numeric line.
pub fn parse_3dl_full<R: BufRead>(reader: R) -> LutResult<ThreeDLFile> {
    let mut shaper_raw: Option<Vec<i32>> = None;
    let mut raw3d: Vec<i32> = Vec::new();
    let mut lut3d_max: i32 = 0;

    for (line_no, line) in reader.lines().enumerate() {
        let line = line?;
        let line = line.trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('<') {
            // Reference: reject files that look like they could be XML, e.g. a mis-routed CLF.
            return Err(LutError::ParseError(format!(
                "3DL: line {} starts with '<' (looks like XML, not a .3dl file): {line:?}",
                line_no + 1
            )));
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        let values: Result<Vec<i32>, _> = parts.iter().map(|s| s.parse::<i32>()).collect();
        let vals = match values {
            Ok(v) => v,
            // Keyword lines (3DMESH, Mesh 4 12, LUT8, gamma 1.0, ...) are valid .3dl syntax that
            // carries no data for us. Reference: "don't reject them" — any line that doesn't
            // parse fully as ints is simply skipped, not treated as a format error.
            Err(_) => continue,
        };

        match vals.len() {
            0 => continue,
            1 | 2 => {
                return Err(LutError::ParseError(format!(
                    "3DL: line {} has {} value(s), need exactly 3 (3D LUT data) or more than 3 \
                     (shaper LUT): {line:?}",
                    line_no + 1,
                    vals.len()
                )));
            }
            3 => {
                if raw3d.len() / 3 >= MAX_3D_LUT_LENGTH.pow(3) {
                    return Err(LutError::SizeOutOfRange {
                        format: "3DL",
                        dims: vec![(raw3d.len() / 3 + 1) as u64],
                        reason: format!(
                            "too many 3D LUT entries (max edge length {MAX_3D_LUT_LENGTH})"
                        ),
                    });
                }
                lut3d_max = lut3d_max.max(vals[0]).max(vals[1]).max(vals[2]);
                raw3d.extend_from_slice(&vals);
            }
            _ => {
                if shaper_raw.is_some() {
                    return Err(LutError::ParseError(format!(
                        "3DL: more than one shaper LUT found (second one at line {})",
                        line_no + 1
                    )));
                }
                shaper_raw = Some(vals);
            }
        }
    }

    if raw3d.is_empty() && shaper_raw.is_none() {
        return Err(LutError::ParseError(
            "3DL: does not appear to contain a shaper LUT or 3D LUT data".into(),
        ));
    }

    // --- Shaper: inferred and scaled from its OWN max value, independent of the 3D LUT. ---
    let shaper = match shaper_raw {
        Some(raw) => {
            let shaper_max = raw.iter().copied().max().unwrap_or(0);
            if shaper_max < LOWEST_PLAUSIBLE_MAXINT {
                return Err(LutError::InvalidBitDepth {
                    format: "3DL",
                    value: format!(
                        "shaper max value {shaper_max} is below the plausible minimum \
                         {LOWEST_PLAUSIBLE_MAXINT}"
                    ),
                });
            }
            let bits = likely_bit_depth(shaper_max).ok_or_else(|| LutError::InvalidBitDepth {
                format: "3DL",
                value: format!("shaper max value {shaper_max}"),
            })?;
            let scale = bit_depth_scale(bits);
            if shaper_is_identity(&raw, scale) {
                None
            } else {
                let entries: Vec<f32> = raw.iter().map(|&v| v as f32 / scale).collect();
                Some(Lut1D::from_data(entries, 0.0, 1.0)?)
            }
        }
        None => None,
    };

    // --- 3D LUT: inferred and scaled from its OWN max value, independent of the shaper. ---
    let lut3d = if raw3d.is_empty() {
        None
    } else {
        let count = raw3d.len() / 3;
        let size = crate::limits::cube_edge_from_count("3DL", count)?;

        if lut3d_max < LOWEST_PLAUSIBLE_MAXINT {
            return Err(LutError::InvalidBitDepth {
                format: "3DL",
                value: format!(
                    "3D LUT max value {lut3d_max} is below the plausible minimum \
                     {LOWEST_PLAUSIBLE_MAXINT}"
                ),
            });
        }
        let bits = likely_bit_depth(lut3d_max).ok_or_else(|| LutError::InvalidBitDepth {
            format: "3DL",
            value: format!("3D LUT max value {lut3d_max}"),
        })?;
        let scale = bit_depth_scale(bits);

        let data: Vec<[f32; 3]> = raw3d
            .as_chunks::<3>().0.iter()
            .map(|c| {
                [
                    c[0] as f32 / scale,
                    c[1] as f32 / scale,
                    c[2] as f32 / scale,
                ]
            })
            .collect();

        // 3DL files are ALREADY in B-fastest order (same as Lut3D internal format)
        // Reference: OCIO FileFormat3DL.cpp:457-459:
        //   "The 3dl format stores the LUT entries in blue-fastest order,
        //    which is the same order used by Lut3DOpData, so no
        //    transposition of LUT entries is needed in this case."
        Some(Lut3D::from_data(data, size)?)
    };

    Ok(ThreeDLFile { shaper, lut3d })
}

/// Writes a 3D LUT to a .3dl file.
///
/// Uses 10-bit output (0-1023 range) by default.
pub fn write_3dl<P: AsRef<Path>>(path: P, lut: &Lut3D) -> LutResult<()> {
    write_3dl_with_depth(path, lut, 10)
}

/// Writes a 3D LUT to a .3dl file with specified bit depth.
///
/// # Arguments
///
/// * `path` - Output file path
/// * `lut` - The 3D LUT to write
/// * `bit_depth` - Output bit depth (10 or 12)
pub fn write_3dl_with_depth<P: AsRef<Path>>(path: P, lut: &Lut3D, bit_depth: u32) -> LutResult<()> {
    let file = File::create(path.as_ref())?;
    let mut writer = BufWriter::new(file);

    let max_value = (1 << bit_depth) - 1;
    let size = lut.size;

    // Write header comment
    writeln!(writer, "# Generated by vfx-lut")?;

    // Write mesh line (evenly spaced input values)
    let mesh: Vec<String> = (0..size)
        .map(|i| {
            let t = i as f32 / (size - 1) as f32;
            ((t * max_value as f32).round() as i32).to_string()
        })
        .collect();
    writeln!(writer, "{}", mesh.join(" "))?;

    // Write LUT data in B-fastest order (same as memory layout)
    // 3DL format: B varies fastest, then G, then R
    // Memory layout: idx = B + size*G + size²*R (B-fastest)
    for r_idx in 0..size {
        for g_idx in 0..size {
            for b_idx in 0..size {
                // B-fastest index matches both file and memory order
                let idx = b_idx + size * (g_idx + size * r_idx);
                let rgb = lut.data[idx];

                let r = (rgb[0].clamp(0.0, 1.0) * max_value as f32).round() as i32;
                let g = (rgb[1].clamp(0.0, 1.0) * max_value as f32).round() as i32;
                let b = (rgb[2].clamp(0.0, 1.0) * max_value as f32).round() as i32;

                writeln!(writer, "{} {} {}", r, g, b)?;
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
    fn parse_3dl_basic() {
        // 2x2x2 cube with 10-bit values. No shaper line: the 3D LUT's bit depth is inferred
        // purely from the 3D data's own max value (1023), per COLOR-04's fix.
        let content = r#"
# Test 3DL
0 0 0
1023 0 0
0 1023 0
1023 1023 0
0 0 1023
1023 0 1023
0 1023 1023
1023 1023 1023
"#;
        let reader = Cursor::new(content);
        let lut = parse_3dl(reader).expect("parse failed");

        assert_eq!(lut.size, 2);

        // Check first entry (black)
        assert!((lut.data[0][0] - 0.0).abs() < 0.01);
        assert!((lut.data[0][1] - 0.0).abs() < 0.01);
        assert!((lut.data[0][2] - 0.0).abs() < 0.01);

        // Check last entry (white)
        let last = lut.data.len() - 1;
        assert!((lut.data[last][0] - 1.0).abs() < 0.01);
        assert!((lut.data[last][1] - 1.0).abs() < 0.01);
        assert!((lut.data[last][2] - 1.0).abs() < 0.01);

        // Interior (non-corner-of-the-diagonal) entry: file row 1, "1023 0 0". `.3dl` is ALREADY
        // blue-fastest in the file (`FileFormat3DL.cpp:457-459`), same order as `Lut3D`'s own
        // memory layout, so the row index IS the memory index directly, with no reordering.
        assert!((lut.data[1][0] - 1.0).abs() < 0.01);
        assert!((lut.data[1][1] - 0.0).abs() < 0.01);
        assert!((lut.data[1][2] - 0.0).abs() < 0.01);
    }

    #[test]
    fn roundtrip_3dl() {
        let lut = Lut3D::identity(4);
        // `tempfile::tempdir()`: per-call unique, auto-cleaned — the fixed
        // filename collided under concurrent/parallel test runs, and the
        // manual `remove_file` below never ran on an earlier panic.
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let path = temp_dir.path().join("test_roundtrip.3dl");

        write_3dl(&path, &lut).expect("write failed");
        let loaded = read_3dl(&path).expect("read failed");

        assert_eq!(loaded.size, 4);

        // Check identity values are preserved
        for i in 0..4 {
            for j in 0..4 {
                for k in 0..4 {
                    // Blue-major order: idx = B + size*G + size²*R
                    let idx = i * 16 + j * 4 + k;
                    let expected_r = i as f32 / 3.0;
                    let expected_g = j as f32 / 3.0;
                    let expected_b = k as f32 / 3.0;

                    // Allow some quantization error (10-bit = 1/1023 resolution)
                    assert!((loaded.data[idx][0] - expected_r).abs() < 0.002);
                    assert!((loaded.data[idx][1] - expected_g).abs() < 0.002);
                    assert!((loaded.data[idx][2] - expected_b).abs() < 0.002);
                }
            }
        }
    }

    #[test]
    fn parse_12bit_3dl() {
        // 2x2x2 cube with 12-bit values.
        let content = r#"
0 0 0
4095 0 0
0 4095 0
4095 4095 0
0 0 4095
4095 0 4095
0 4095 4095
4095 4095 4095
"#;
        let reader = Cursor::new(content);
        let lut = parse_3dl(reader).expect("parse failed");

        assert_eq!(lut.size, 2);

        // Check last entry (white at 12-bit)
        let last = lut.data.len() - 1;
        assert!((lut.data[last][0] - 1.0).abs() < 0.01);
    }

    /// COLOR-04 regression: a near-identity 12-bit shaper mesh line precedes 10-bit 3D LUT data.
    /// Before the fix, `parse_3dl` used the shaper mesh's LAST value (4095) as the scale divisor
    /// for the 3D data too, producing white = 1023/4095 ≈ 0.24982 instead of 1.0. The fix infers
    /// each table's bit depth from its OWN max value (reference: `FileFormat3DL.cpp::read()`,
    /// `out3DBD` derived from `lut3dmax`, independent of `out1DBD`/`shapermax`).
    #[test]
    fn threedl_3d_scale_uses_own_bit_depth_not_shaper_max() {
        // Shaper mesh: 5 entries, near-identity 12-bit ramp (rounds within the +/-2 code
        // tolerance of a straight line from 0 to 4095), so it is correctly dropped as a no-op.
        let content = r#"
0 1024 2048 3072 4095
0 0 0
1023 0 0
0 1023 0
1023 1023 0
0 0 1023
1023 0 1023
0 1023 1023
1023 1023 1023
"#;
        let lut = parse_3dl(Cursor::new(content)).expect("parse failed");
        assert_eq!(lut.size, 2);

        let white = lut.data.len() - 1;
        // Before fix: 1023.0 / 4095.0 ~= 0.24982 (wrongly scaled by the shaper's 12-bit max).
        // After fix:  1023.0 / 1023.0 == 1.0    (correctly scaled by the 3D data's own max).
        let pre_fix_value = 1023.0f32 / 4095.0f32;
        assert!(
            (lut.data[white][0] - 1.0).abs() < 0.002,
            "expected white corner ~1.0, got {} (pre-fix bug would give {:.5})",
            lut.data[white][0],
            pre_fix_value
        );
        assert!((lut.data[white][1] - 1.0).abs() < 0.002);
        assert!((lut.data[white][2] - 1.0).abs() < 0.002);
    }

    /// COLOR-04 regression: a genuinely non-identity shaper must survive through
    /// `parse_3dl_full` (not be silently dropped), while `parse_3dl` (which has nowhere to put
    /// it) surfaces an error instead of silently discarding it.
    #[test]
    fn threedl_non_identity_shaper_is_not_silently_dropped() {
        // Deviates far more than the +/-2 code identity tolerance from a straight ramp.
        let content = r#"
0 500 1000 3000 3800 4095
0 0 0
1023 0 0
0 1023 0
1023 1023 0
0 0 1023
1023 0 1023
0 1023 1023
1023 1023 1023
"#;
        let file = parse_3dl_full(Cursor::new(content)).expect("parse failed");

        let shaper = file
            .shaper
            .expect("non-identity shaper must not be dropped");
        assert_eq!(shaper.size(), 6);
        assert!((shaper.r[0] - 0.0).abs() < 1e-6);
        assert!((shaper.r[5] - 1.0).abs() < 1e-6); // 4095 / 4095
        assert!((shaper.r[1] - 500.0 / 4095.0).abs() < 1e-5);

        // 3D LUT is still scaled by its OWN bit depth (1023), not the shaper's (4095).
        let lut3d = file.lut3d.expect("3D LUT data present");
        let white = lut3d.data.len() - 1;
        assert!((lut3d.data[white][0] - 1.0).abs() < 0.002);

        // parse_3dl() cannot honour the shaper through a Lut3D-only signature; it must error
        // rather than silently discard it.
        let err = parse_3dl(Cursor::new(content))
            .expect_err("must surface the un-applicable shaper as an error, not drop it");
        assert!(matches!(err, LutError::Unsupported { .. }));
    }
}
