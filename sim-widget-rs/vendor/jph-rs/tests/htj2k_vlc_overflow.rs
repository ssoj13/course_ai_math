//! Regression test for the SIMD HT block-encoder VLC buffer overflow.
//!
//! Background: the AVX2/AVX512 `vlc_encode` packs VLC codewords into a fixed
//! 2880-byte buffer that grows *downward* (`buf.sub(pos)`). High-entropy
//! code-blocks (e.g. noisy mask / normal AOVs in a wide HTJ2K tile) can emit
//! more VLC bytes than that budget. Before the fix the SIMD store path had no
//! `pos >= buf_size` bounds check (unlike the scalar path), so the write walked
//! below the buffer base and produced a native `0xC0000005` access violation
//! that `catch_unwind` could not trap — crashing the whole process.
//!
//! This test drives maximally high-entropy content through the HTJ2K encoder
//! with the same 128x32 code-block geometry exr-core uses, across a 1024-wide
//! tile. The guarantee being asserted: the encode must never access-violate.
//! It must either succeed with a valid codestream, or, if a code-block genuinely
//! overruns the VLC budget, raise the *recoverable* `ojph_error!` panic (the
//! same behaviour the scalar path already has) — which `catch_unwind` traps.
//! A surviving test process (Ok or caught panic) = pass; an AV would abort the
//! whole binary.

use jph_rs::codestream::ojph_params_local::ParamSiz;
use jph_rs::openjph::ojph_base::{Point, Size};
use jph_rs::openjph::ojph_codestream::Codestream;
use jph_rs::openjph::ojph_defs::Ui32;
use jph_rs::openjph::ojph_file::{MemInfile, MemOutfile};
use jph_rs::openjph::ojph_message::OjphResult;

/// xorshift32 PRNG — deterministic, no external crate, good enough to defeat
/// the wavelet+RCT and leave high-magnitude high-entropy coefficients for the
/// HT block coder (which is what stresses the VLC budget).
struct XorShift32(u32);
impl XorShift32 {
    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
}

/// Encode `width`x`height` RGB (3 component, 16-bit) high-entropy noise as
/// reversible HTJ2K with 128x32 code-blocks and the RCT colour transform — the
/// exact geometry exr-core configures for EXR `HT256`. Returns the codestream
/// bytes. A buffer-full `ojph_error!` now surfaces as `Err` (the port no longer
/// panics on this path); a real AV would still abort the process.
fn encode_rgb_noise(width: u32, height: u32, seed: u32) -> OjphResult<Vec<u8>> {
    let num_comps = 3u32;

    // Build pathological high-frequency planes: every adjacent sample swings
    // between extremes, which after the 5/3 wavelet produces maximal high-
    // frequency subband coefficients — the worst case for the HT VLC budget.
    // A per-pixel PRNG bit decides full-scale-high vs full-scale-low so the
    // pattern is both maximal-gradient AND high-entropy (defeats run coding).
    let mut rng = XorShift32(seed);
    let plane_len = (width * height) as usize;
    let mut planes: Vec<Vec<i32>> = Vec::with_capacity(num_comps as usize);
    for _ in 0..num_comps {
        let mut p = Vec::with_capacity(plane_len);
        for i in 0..plane_len {
            let bit = (rng.next() >> 13) & 1;
            // Alternate extremes per pixel, jittered by the PRNG bit.
            let v = if (i & 1) ^ (bit as usize) == 0 { 0u32 } else { 0xFFFF };
            p.push(v as i32);
        }
        planes.push(p);
    }

    let mut cs = Codestream::new();
    {
        let siz: &mut ParamSiz = cs.access_siz();
        siz.set_image_extent(Point::new(width, height));
        // Single tile covering the whole image: a `width`-wide tile.
        siz.set_tile_size(Size::new(width, height));
        siz.set_num_components(num_comps);
        for c in 0..num_comps {
            siz.set_comp_info(c, Point::new(1, 1), 16, false);
        }
    }
    {
        let cod = cs.access_cod();
        cod.set_num_decompositions(5)?;
        // 128x32 == exr-core's `set_block_dims(128, 32)` (product 4096).
        cod.set_block_dims(128, 32)?;
        cod.set_progression_order("RPCL")?;
        cod.set_reversible(true);
        // RCT across R,G,B — enabled by exr-core when an RGB triplet is present.
        cod.employ_color_transform(1);
    }
    // Interleaved (non-planar) feed: the RCT colour transform requires the
    // interleaved interface (the planar interface rejects it). exr-core uses
    // exactly this combination (`set_planar(false)` + `employ_color_transform`)
    // for RGB HTJ2K — the configuration that produced the original crash.
    cs.set_planar(false);

    let mut outfile = MemOutfile::default();
    outfile.open(width as usize * height as usize * num_comps as usize + 64 * 1024, false)?;
    cs.write_headers_slice(&mut outfile, &[])?;

    let mut comp_rows = vec![0u32; num_comps as usize];
    let mut next_comp: Ui32 = 0;
    let mut cur_line = cs.exchange(std::ptr::null_mut(), &mut next_comp)?;
    while !cur_line.is_null() {
        let c = next_comp as usize;
        let y = comp_rows[c];
        unsafe {
            let dp = (*cur_line).storage.i32_;
            for x in 0..width {
                let idx = (y * width + x) as usize;
                *dp.add(x as usize) = planes[c][idx];
            }
        }
        comp_rows[c] += 1;
        cur_line = cs.exchange(cur_line, &mut next_comp)?;
    }

    cs.flush()?;
    cs.close();
    Ok(outfile.get_data().to_vec())
}

/// 1024-wide tile of high-entropy RGB noise must not access-violate in the SIMD
/// VLC store path. Success or a caught buffer-full panic are both acceptable;
/// the test simply must not crash the process.
#[test]
fn htj2k_simd_vlc_high_entropy_1024_tile_no_av() {
    // 1024 wide is the tile width that triggered the original AV. Several seeds
    // raise the odds of hitting the worst-case code-block coefficient pattern.
    for seed in [0x1234_5678u32, 0x9E37_79B9, 0xDEAD_BEEF, 0x0BAD_F00D] {
        let result = encode_rgb_noise(1024, 256, seed);
        match result {
            Ok(bytes) => {
                // Valid HTJ2K/J2C codestreams start with the SOC marker 0xFF4F.
                assert!(bytes.len() > 2, "empty codestream for seed {seed:#x}");
                assert_eq!(
                    &bytes[0..2],
                    &[0xFF, 0x4F],
                    "missing SOC marker for seed {seed:#x}"
                );
            }
            Err(_) => {
                // A recoverable buffer-full `ojph_error!` is acceptable — it
                // proves the guard fired instead of access-violating.
            }
        }
    }
}

/// Decode-back smoke check on a moderate-entropy 1024-wide tile: the guard must
/// not perturb valid output, so a clean encode must round-trip bit-exactly.
#[test]
fn htj2k_simd_1024_tile_roundtrips_when_in_budget() {
    // Lower-entropy content (a smooth ramp) stays well within the VLC budget, so
    // it must encode and decode losslessly with the guard in place (byte-neutral).
    let (w, h) = (1024u32, 64u32);
    let num_comps = 3u32;
    let mut planes: Vec<Vec<i32>> = Vec::new();
    for c in 0..num_comps {
        let mut p = Vec::with_capacity((w * h) as usize);
        for y in 0..h {
            for x in 0..w {
                p.push((((x + y * 2 + c * 50) & 0xFFFF) as i32) >> 2);
            }
        }
        planes.push(p);
    }

    let mut cs = Codestream::new();
    {
        let siz: &mut ParamSiz = cs.access_siz();
        siz.set_image_extent(Point::new(w, h));
        siz.set_tile_size(Size::new(w, h));
        siz.set_num_components(num_comps);
        for c in 0..num_comps {
            siz.set_comp_info(c, Point::new(1, 1), 16, false);
        }
    }
    {
        let cod = cs.access_cod();
        cod.set_num_decompositions(5).expect("set_num_decompositions");
        cod.set_block_dims(128, 32).expect("set_block_dims");
        cod.set_progression_order("RPCL").expect("set_progression_order");
        cod.set_reversible(true);
        cod.employ_color_transform(1);
    }
    // Interleaved feed (required for the RCT colour transform) — matches exr-core.
    cs.set_planar(false);

    let mut outfile = MemOutfile::default();
    outfile
        .open((w * h * num_comps) as usize + 64 * 1024, false)
        .expect("open");
    cs.write_headers_slice(&mut outfile, &[]).expect("write_headers_slice");
    let mut comp_rows = vec![0u32; num_comps as usize];
    let mut next_comp: Ui32 = 0;
    let mut cur_line = cs.exchange(std::ptr::null_mut(), &mut next_comp).expect("exchange");
    while !cur_line.is_null() {
        let c = next_comp as usize;
        let y = comp_rows[c];
        unsafe {
            let dp = (*cur_line).storage.i32_;
            for x in 0..w {
                *dp.add(x as usize) = planes[c][(y * w + x) as usize];
            }
        }
        comp_rows[c] += 1;
        cur_line = cs.exchange(cur_line, &mut next_comp).expect("exchange");
    }
    cs.flush().expect("flush");
    cs.close();
    let j2c = outfile.get_data().to_vec();
    assert_eq!(&j2c[0..2], &[0xFF, 0x4F], "missing SOC marker");

    // Decode back and verify bit-exact lossless reconstruction.
    let mut infile = MemInfile::default();
    infile.open(&j2c);
    let mut cs = Codestream::new();
    cs.read_headers(&mut infile).expect("read_headers");
    cs.create().expect("create");
    let mut decoded: Vec<Vec<i32>> = vec![vec![0i32; (w * h) as usize]; num_comps as usize];
    // Per-component write cursor: each pulled line carries its component index;
    // we advance that component's row independently (robust to progression order
    // and to legitimately all-zero rows).
    let mut comp_rows_dec = vec![0u32; num_comps as usize];
    for _ in 0..(h * num_comps) {
        let mut comp_num: Ui32 = 0;
        let line = cs.pull(&mut comp_num).expect("pull");
        let c = comp_num as usize;
        let row = comp_rows_dec[c];
        unsafe {
            let sp = (*line).storage.i32_;
            for x in 0..w {
                decoded[c][(row * w + x) as usize] = *sp.add(x as usize);
            }
        }
        comp_rows_dec[c] += 1;
    }
    cs.close();
    for c in 0..num_comps as usize {
        assert_eq!(decoded[c], planes[c], "component {c} did not round-trip");
    }
}
