//! ICC Matrix/TRC display profiles (`.icc`, `.icm`, `.pf`) as OCIO reads them for a
//! `FileTransform` (`fileformats/FileFormatICC.cpp`, `ext/sampleicc`).
//!
//! OCIO supports only the Matrix/TRC model: three colorant tags (`rXYZ`, `gXYZ`, `bXYZ`, the
//! device RGB -> PCS XYZ D50 matrix) and three tone curves (`rTRC`, `gTRC`, `bTRC`) of one kind:
//! - `curv` with one entry: a gamma (u8.8 fixed point),
//! - `curv` with a table: a 1D LUT (device code -> linear), 16-bit,
//! - `para` type 0: a gamma; types 1-4: sampled into a 1024-entry 1D LUT, clamped to [0, 1].
//!
//! LUT-based (A2B/B2A), CMYK, Lab-PCS and grey profiles are rejected with OCIO's messages. This
//! module only reads; vfx-ocio builds the ops (matrix, D50->D65 Bradford, curve), as
//! `FileFormatICC::buildFileOps` does. (vfx-icc wraps lcms for general ICC colour management; that
//! is a different job from OCIO's FileTransform.)

use crate::{Lut1D, LutError, LutResult};
use std::path::Path;

/// The tone curves of a Matrix/TRC profile.
#[derive(Debug, Clone)]
pub enum IccTrc {
    /// Per-channel gamma (single-entry `curv`, or `para` type 0). OCIO applies it without clamping,
    /// mirrored below 0.
    Gamma([f32; 3]),
    /// Per-channel 1D LUT, device code -> linear (a `curv` table, or `para` types 1-4 sampled).
    Lut(Lut1D),
}

/// A Matrix/TRC ICC profile, as OCIO's cached file holds it.
#[derive(Debug, Clone)]
pub struct IccMatrixTrc {
    /// Device RGB -> PCS XYZ (D50), row-major 3x3, from the colorant tags (s15.16 / 65536).
    pub matrix: [f64; 9],
    /// The tone curves.
    pub trc: IccTrc,
}

fn err(file: &str, msg: &str) -> LutError {
    LutError::ParseError(format!("Error parsing .icc file ({file}).  {msg}"))
}

fn be32(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn be16(d: &[u8], o: usize) -> Option<u16> {
    d.get(o..o + 2).map(|b| u16::from_be_bytes([b[0], b[1]]))
}

/// s15.16 fixed point (`icFtoD`).
fn s15f16(v: u32) -> f32 {
    (v as i32) as f32 / 65536.0
}

/// Reads an ICC profile file.
pub fn read_icc(path: &Path) -> LutResult<IccMatrixTrc> {
    let data = std::fs::read(path)?;
    parse_icc(&data, &path.display().to_string())
}

/// Parses an ICC profile (`file` names it in error messages).
pub fn parse_icc(d: &[u8], file: &str) -> LutResult<IccMatrixTrc> {
    // Header (128 bytes) + tag count.
    if d.len() < 128 {
        return Err(err(file, "Error loading header."));
    }
    if &d[36..40] != b"acsp" {
        return Err(err(file, "Wrong magic number."));
    }
    let count = be32(d, 128).ok_or_else(|| err(file, "Error loading number of tags."))?;
    if count > 100 {
        return Err(err(file, "Too many tags in ICC profile."));
    }
    let mut tags = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let base = 132 + 12 * i;
        match (d.get(base..base + 4), be32(d, base + 4), be32(d, base + 8)) {
            (Some(sig), Some(off), Some(size)) => tags.push((sig, off as usize, size as usize)),
            _ => return Err(err(file, "Error loading tag offset table from header.")),
        }
    }
    // SampleICC `IccContent::Validate`.
    let class = &d[12..16];
    if ![
        b"scnr", b"mntr", b"prtr", b"link", b"spac", b"abst", b"nmcl",
    ]
    .iter()
    .any(|c| class == *c)
    {
        return Err(err(
            file,
            &format!("Unknown profile class: {}. ", be32(d, 12).unwrap_or(0)),
        ));
    }
    let intent = be32(d, 64).unwrap_or(u32::MAX);
    if intent > 3 {
        return Err(err(file, &format!("Unknown rendering intent: {intent}. ")));
    }
    if tags.is_empty() {
        return Err(err(file, "No tags present. "));
    }

    // A tag's data: (type signature, body after the 8-byte type header).
    let tag = |sig: &[u8]| -> Option<(&[u8], &[u8])> {
        let &(_, off, size) = tags.iter().find(|(s, _, _)| *s == sig)?;
        let t = d.get(off..off.checked_add(size)?)?;
        (t.len() >= 8).then(|| (&t[..4], &t[8..]))
    };

    // Matrix part.
    let xyz = |sig: &[u8]| -> Option<[f64; 3]> {
        let (ty, body) = tag(sig)?;
        // IccXYZArrayTypeReader: exactly one XYZ number.
        if ty != b"XYZ " || body.len() != 12 {
            return None;
        }
        Some(std::array::from_fn(|i| {
            (be32(body, 4 * i).unwrap() as i32) as f64 / 65536.0
        }))
    };
    let (Some(r), Some(g), Some(b)) = (xyz(b"rXYZ"), xyz(b"gXYZ"), xyz(b"bXYZ")) else {
        return Err(err(file, "Illegal matrix tag in ICC profile."));
    };
    let matrix = [r[0], g[0], b[0], r[1], g[1], b[1], r[2], g[2], b[2]];

    // Curve part.
    let same_type = "All curves in the ICC profile must be of the same type.";
    let (Some(rt), Some(gt), Some(bt)) = (tag(b"rTRC"), tag(b"gTRC"), tag(b"bTRC")) else {
        return Err(err(file, "Illegal curve tag in ICC profile."));
    };
    let known = |t: &[u8]| t == b"curv" || t == b"para";
    if !known(rt.0) || !known(gt.0) || !known(bt.0) {
        return Err(err(file, "Illegal curve tag in ICC profile."));
    }
    let trc = if rt.0 == b"para" {
        if gt.0 != b"para" || bt.0 != b"para" {
            return Err(err(file, same_type));
        }
        let para = |body: &[u8]| -> Option<(u16, Vec<f32>)> {
            let ty = be16(body, 0)?;
            let n = body.len().checked_sub(4)? / 4;
            if n == 0 {
                return None;
            }
            Some((
                ty,
                (0..n)
                    .map(|i| s15f16(be32(body, 4 + 4 * i).unwrap()))
                    .collect(),
            ))
        };
        let (Some(pr), Some(pg), Some(pb)) = (para(rt.1), para(gt.1), para(bt.1)) else {
            return Err(err(file, "Illegal curve tag in ICC profile."));
        };
        if pr.0 != pg.0 || pr.0 != pb.0 {
            return Err(err(file, same_type));
        }
        for p in [&pr, &pg, &pb] {
            validate_parametric(p.0, &p.1).map_err(|m| err(file, &m))?;
        }
        if pr.0 == 0 {
            IccTrc::Gamma([pr.1[0], pg.1[0], pb.1[0]])
        } else {
            const N: usize = 1024;
            let sample = |p: &(u16, Vec<f32>)| -> Vec<f32> {
                (0..N)
                    .map(|i| apply_parametric(i as f32 / (N as f32 - 1.0), p.0, &p.1))
                    .collect()
            };
            IccTrc::Lut(Lut1D::from_rgb(
                sample(&pr),
                sample(&pg),
                sample(&pb),
                0.0,
                1.0,
            )?)
        }
    } else {
        if gt.0 == b"para" || bt.0 == b"para" {
            return Err(err(file, same_type));
        }
        let curv = |body: &[u8]| -> Option<Vec<f32>> {
            let n = be32(body, 0)? as usize;
            if n > 65536 {
                return None;
            }
            (0..n)
                .map(|i| be16(body, 4 + 2 * i).map(|v| v as f32 / 65535.0))
                .collect()
        };
        let (Some(cr), Some(cg), Some(cb)) = (curv(rt.1), curv(gt.1), curv(bt.1)) else {
            return Err(err(file, "Illegal curve tag in ICC profile."));
        };
        if cg.len() != cr.len() || cb.len() != cr.len() {
            return Err(err(
                file,
                "All curves in the ICC profile must be of the same length.",
            ));
        }
        match cr.len() {
            0 => return Err(err(file, "Curves with no values in ICC profile.")),
            // u8.8 gamma; SampleICC normalised the u16 by 65535.
            1 => IccTrc::Gamma([cr[0], cg[0], cb[0]].map(|v| v * 65535.0 / 256.0)),
            _ => IccTrc::Lut(Lut1D::from_rgb(cr, cg, cb, 0.0, 1.0)?),
        }
    };
    Ok(IccMatrixTrc { matrix, trc })
}

/// `LocalFileFormat::ValidateParametricCurve` (the errors; OCIO's warnings are not reported).
fn validate_parametric(ty: u16, p: &[f32]) -> Result<(), String> {
    let args = p
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    let fail = |m: &str| {
        Err(format!(
            "Error parsing ICC Parametric Curve (with arguments {args}): {m}"
        ))
    };
    let expected = match ty {
        0 => 1,
        1 => 3,
        2 => 4,
        3 => 5,
        4 => 7,
        _ => return fail("Unknown parametric curve type."),
    };
    if p.len() != expected {
        return fail(&format!("Expecting {expected}param(s)."));
    }
    let q = |v: f32| (v * 1023.0).round() / 1023.0;
    let g = p[0];
    if g <= 0.0 {
        return fail("Expecting monotonically non-decreasing power-law.");
    }
    if ty != 0 && p[1] <= 0.0 {
        return fail("Expecting strictly increasing argument to power-law.");
    }
    if (ty == 3 || ty == 4) && p[3] < 0.0 {
        return fail("Expecting flat or increasing linear segment.");
    }
    if ty == 3 && q(p[3] * p[4]) > q((p[1] * p[4] + p[2]).powf(g)) {
        return fail("Expecting no negative discontinuity at linear segment boundary.");
    }
    if ty == 4 && q(p[3] * p[4] + p[6]) > q((p[1] * p[4] + p[2]).powf(g) + p[5]) {
        return fail("Expecting no negative discontinuity at linear segment boundary.");
    }
    if (ty == 3 || ty == 4) && p[1] * p[4] + p[2] < 0.0 {
        return fail("Expecting no negative arguments to the power law.");
    }
    Ok(())
}

/// `LocalFileFormat::ApplyParametricCurve`: the ICC curve functions, clipped to [0, 1] on both
/// sides as the ICC specifies.
fn apply_parametric(v: f32, ty: u16, p: &[f32]) -> f32 {
    let v = v.clamp(0.0, 1.0);
    let v = match ty {
        1 => {
            if v >= -p[2] / p[1] {
                (p[1] * v + p[2]).powf(p[0])
            } else {
                0.0
            }
        }
        2 => {
            if v >= -p[2] / p[1] {
                (p[1] * v + p[2]).powf(p[0]) + p[3]
            } else {
                p[3]
            }
        }
        3 => {
            if v >= p[4] {
                (p[1] * v + p[2]).powf(p[0])
            } else {
                p[3] * v
            }
        }
        4 => {
            if v >= p[4] {
                (p[1] * v + p[2]).powf(p[0]) + p[5]
            } else {
                p[3] * v + p[6]
            }
        }
        _ => v,
    };
    v.clamp(0.0, 1.0)
}
