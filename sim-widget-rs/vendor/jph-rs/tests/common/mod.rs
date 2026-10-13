//! Helpers shared by the integration tests (`mod common;`). Not every test uses every helper.
#![allow(dead_code)]
use std::path::Path;

use jph_rs::mem::{Image, Plane};

/// Minimal binary PNM reader (P5 gray / P6 RGB, maxval 255 or 65535).
pub fn read_pnm(path: &Path) -> Image {
    let b = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let (mut pos, mut tok) = (0usize, Vec::<String>::new());
    while tok.len() < 4 {
        while b[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if b[pos] == b'#' {
            while b[pos] != b'\n' {
                pos += 1;
            }
            continue;
        }
        let s = pos;
        while !b[pos].is_ascii_whitespace() {
            pos += 1;
        }
        tok.push(String::from_utf8_lossy(&b[s..pos]).into_owned());
    }
    pos += 1; // single whitespace after maxval
    let comps = match tok[0].as_str() {
        "P5" => 1usize,
        "P6" => 3,
        m => panic!("{}: unsupported magic {m}", path.display()),
    };
    let (w, h, max): (u32, u32, u32) = (tok[1].parse().unwrap(), tok[2].parse().unwrap(), tok[3].parse().unwrap());
    let (depth, bps) = if max > 255 { (16, 2) } else { (8, 1) };
    let n = (w * h) as usize;
    let planes = (0..comps)
        .map(|c| Plane {
            width: w,
            height: h,
            bit_depth: depth,
            signed: false,
            samples: (0..n)
                .map(|i| {
                    let o = pos + (i * comps + c) * bps;
                    if bps == 1 { i32::from(b[o]) } else { i32::from(u16::from_be_bytes([b[o], b[o + 1]])) }
                })
                .collect(),
        })
        .collect();
    Image { width: w, height: h, planes }
}


/// Binary PGM -> `(width, height, samples)`; panics with the path on failure (test helper).
pub fn pgm(path: &Path) -> (u32, u32, Vec<i32>) {
    let img = read_pnm(path);
    assert_eq!(img.planes.len(), 1, "{}: not a PGM", path.display());
    let p = &img.planes[0];
    (p.width, p.height, p.samples.clone())
}

/// Deterministic, position-dependent 8-bit plane (any packet/block mix-up changes it).
pub fn synth_plane(w: u32, h: u32, seed: u32) -> Plane {
    let samples = (0..w * h).map(|i| ((i.wrapping_mul(2654435761).wrapping_add(seed * 977) >> 8) & 0xFF) as i32).collect();
    Plane { width: w, height: h, bit_depth: 8, signed: false, samples }
}

/// Lets a test build an [`Image`] of equally-sized synthetic planes.
pub fn synth_image(w: u32, h: u32, comps: u32) -> Image {
    Image { width: w, height: h, planes: (0..comps).map(|c| synth_plane(w, h, c + 1)).collect() }
}
