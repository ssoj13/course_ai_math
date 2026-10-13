//! Iridas Look 3D LUT format parser.
//!
//! XML format used by Iridas/Adobe SpeedGrade containing baked shader data.
//!
//! # Format
//!
//! ```xml
//! <?xml version="1.0" ?>
//! <look>
//!   <shaders>...</shaders>
//!   <LUT>
//!     <size>"8"</size>
//!     <data>"0000803F0000803F..."</data>
//!   </LUT>
//! </look>
//! ```
//!
//! Data is hex-encoded 32-bit floats in little-endian byte order.
//! Red coordinate changes fastest.

use std::io::{BufReader, Read};
use std::path::Path;

use crate::error::{LutError, LutResult};
use crate::lut3d::Lut3D;

/// Parses an Iridas Look file from a reader.
pub fn parse_look<R: Read>(reader: R) -> LutResult<Lut3D> {
    // A read error (including invalid UTF-8) is an error, not an empty line.
    let mut content = String::new();
    BufReader::new(reader).read_to_string(&mut content)?;

    // Simple XML parsing - find <size> and <data> elements
    let size = extract_element(&content, "size")
        .ok_or_else(|| LutError::ParseError("missing <size> element".into()))?;
    let data = extract_element(&content, "data")
        .ok_or_else(|| LutError::ParseError("missing <data> element".into()))?;

    // Parse size (strip quotes)
    let size_str = size.trim().trim_matches(|c| c == '"' || c == '\'');
    let lut_size: usize = size_str
        .parse()
        .map_err(|_| LutError::ParseError(format!("invalid size: {}", size_str)))?;
    // FileFormatIridasLook.cpp: the size must be in [2, Max3DLUTLength].
    let entries = crate::limits::check_3d_size("Iridas Look", lut_size)?;

    // Parse hex data
    let hex_data: String = data.chars().filter(|c| c.is_ascii_hexdigit()).collect();

    if !hex_data.len().is_multiple_of(8) {
        return Err(LutError::ParseError(format!(
            "hex data length {} not multiple of 8",
            hex_data.len()
        )));
    }

    let expected = entries * 3;
    let num_floats = hex_data.len() / 8;
    if num_floats != expected {
        return Err(LutError::ParseError(format!(
            "expected {} floats, got {}",
            expected, num_floats
        )));
    }

    // Convert hex to floats
    let mut floats = Vec::with_capacity(num_floats);
    for i in 0..num_floats {
        let hex_str = &hex_data[i * 8..(i + 1) * 8];
        let f = hex_to_float_le(hex_str)?;
        floats.push(f);
    }

    // Convert to RGB triplets
    let mut data = Vec::with_capacity(entries);
    for chunk in floats.chunks(3) {
        data.push([chunk[0], chunk[1], chunk[2]]);
    }

    // Look uses R-fastest order, but Lut3D expects B-fastest order
    // Reorder from file order (R-fastest) to memory order (B-fastest)
    // Reference: OCIO FileFormatIridasLook.cpp:537 calls setArrayFromRedFastestOrder()
    let mut reordered = vec![[0.0f32; 3]; data.len()];
    for b in 0..lut_size {
        for g in 0..lut_size {
            for r in 0..lut_size {
                let file_idx = r + g * lut_size + b * lut_size * lut_size; // R-fastest
                let mem_idx = b + g * lut_size + r * lut_size * lut_size; // B-fastest
                reordered[mem_idx] = data[file_idx];
            }
        }
    }

    Lut3D::from_data(reordered, lut_size)
}

/// Reads an Iridas Look file from disk.
pub fn read_look<P: AsRef<Path>>(path: P) -> LutResult<Lut3D> {
    let file = std::fs::File::open(path.as_ref())?;
    parse_look(file)
}

/// Extracts content between XML element tags.
fn extract_element(content: &str, tag: &str) -> Option<String> {
    let start_tag = format!("<{}", tag);
    let end_tag = format!("</{}>", tag);

    let start_pos = content.find(&start_tag)?;
    let after_start = &content[start_pos..];

    // Find > after start tag (handles attributes)
    let content_start = after_start.find('>')? + 1;
    let remaining = &after_start[content_start..];

    let end_pos = remaining.find(&end_tag)?;
    Some(remaining[..end_pos].to_string())
}

/// Converts 8-char hex string to little-endian f32.
fn hex_to_float_le(hex: &str) -> LutResult<f32> {
    if hex.len() != 8 {
        return Err(LutError::ParseError("hex string must be 8 chars".into()));
    }

    let mut bytes = [0u8; 4];
    for i in 0..4 {
        let byte_hex = &hex[i * 2..(i + 1) * 2];
        bytes[i] = u8::from_str_radix(byte_hex, 16)
            .map_err(|_| LutError::ParseError(format!("invalid hex: {}", byte_hex)))?;
    }

    // Little-endian byte order
    Ok(f32::from_le_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_conversion() {
        // "0000003F" -> 0.5f
        assert!((hex_to_float_le("0000003F").unwrap() - 0.5).abs() < 1e-6);
        // "0000803F" -> 1.0f
        assert!((hex_to_float_le("0000803F").unwrap() - 1.0).abs() < 1e-6);
        // "00000000" -> 0.0f
        assert!((hex_to_float_le("00000000").unwrap() - 0.0).abs() < 1e-6);
    }

    #[test]
    fn parse_minimal_look() {
        let xml = r#"<?xml version="1.0" ?>
<look>
  <LUT>
    <size>"2"</size>
    <data>"000000000000000000000000
0000803F000000000000000000000000000080
3F000000000000803F0000803F00000000000000000000803F00000000
0000803F000000000000803F000000000000803F0000803F0000803F0000803F0000803F"</data>
  </LUT>
</look>"#;
        let lut = parse_look(xml.as_bytes()).unwrap();
        assert_eq!(lut.size, 2);
        assert_eq!(lut.data.len(), 8);

        // Check black corner
        assert!((lut.data[0][0] - 0.0).abs() < 1e-6);
        assert!((lut.data[0][1] - 0.0).abs() < 1e-6);
        assert!((lut.data[0][2] - 0.0).abs() < 1e-6);

        // Check white corner
        assert!((lut.data[7][0] - 1.0).abs() < 1e-6);
        assert!((lut.data[7][1] - 1.0).abs() < 1e-6);
        assert!((lut.data[7][2] - 1.0).abs() < 1e-6);

        // Interior (non-black/white) entry: hex row 1 is "0000803F0000000000000000" ->
        // (1.0, 0.0, 0.0), the (r=1,g=0,b=0) grid point in the file's Red-fastest order.
        // Reference: `FileFormatIridasLook.cpp:537` (`setArrayFromRedFastestOrder`). Memory
        // (Blue-fastest): idx = b + size*(g + size*r) = 0 + 2*(0 + 2*1) = 4.
        assert!((lut.data[4][0] - 1.0).abs() < 1e-6);
        assert!((lut.data[4][1] - 0.0).abs() < 1e-6);
        assert!((lut.data[4][2] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn extract_element_works() {
        let xml = r#"<root><size>"8"</size><data>ABC</data></root>"#;
        assert_eq!(extract_element(xml, "size"), Some("\"8\"".to_string()));
        assert_eq!(extract_element(xml, "data"), Some("ABC".to_string()));
    }

    #[test]
    fn error_missing_size() {
        let xml = r#"<look><LUT><data>00000000</data></LUT></look>"#;
        assert!(parse_look(xml.as_bytes()).is_err());
    }
}
