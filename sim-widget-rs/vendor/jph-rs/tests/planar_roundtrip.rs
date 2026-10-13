//! Planar feed mode: encode → decode roundtrip (lossless gray, no colour transform).

use jph_rs::codestream::ojph_params_local::ParamSiz;
use jph_rs::openjph::ojph_base::{Point, Size};
use jph_rs::openjph::ojph_codestream::Codestream;
use jph_rs::openjph::ojph_defs::Ui32;
use jph_rs::openjph::ojph_file::{MemInfile, MemOutfile};

fn encode_planar_gray(w: u32, h: u32, pixels: &[u8], planar: bool) -> Vec<u8> {
    let mut cs = Codestream::new();
    {
        let siz: &mut ParamSiz = cs.access_siz();
        siz.set_image_extent(Point::new(w, h));
        siz.set_tile_size(Size::new(w, h));
        siz.set_num_components(1);
        siz.set_comp_info(0, Point::new(1, 1), 8, false);
    }
    {
        let cod = cs.access_cod();
        cod.set_num_decompositions(2).expect("set_num_decompositions");
        cod.set_block_dims(32, 32).expect("set_block_dims");
        cod.set_progression_order("RPCL").expect("set_progression_order");
        cod.set_reversible(true);
        cod.employ_color_transform(0);
    }
    cs.set_planar(planar);

    let mut outfile = MemOutfile::default();
    outfile.open(64 * 1024, false).expect("open");
    cs.write_headers_slice(&mut outfile, &[]).expect("write_headers_slice");

    let mut next_comp: Ui32 = 0;
    let mut cur_line = cs.exchange(std::ptr::null_mut(), &mut next_comp).expect("exchange");
    let mut y = 0u32;
    while !cur_line.is_null() {
        unsafe {
            let dp = (*cur_line).storage.i32_;
            for x in 0..w {
                let idx = (y * w + x) as usize;
                *dp.add(x as usize) = pixels[idx] as i32;
            }
        }
        y += 1;
        cur_line = cs.exchange(cur_line, &mut next_comp).expect("exchange");
    }

    cs.flush().expect("flush");
    cs.close();
    outfile.get_data().to_vec()
}

fn decode_gray(j2c: &[u8], w: u32, h: u32) -> Vec<u8> {
    let mut infile = MemInfile::default();
    infile.open(&j2c);

    let mut cs = Codestream::new();
    cs.read_headers(&mut infile).expect("read_headers");
    cs.create().expect("create");

    let mut out = vec![0u8; (w * h) as usize];
    for y in 0..h {
        let mut comp_num: Ui32 = 0;
        let line = cs.pull(&mut comp_num).expect("pull");
        unsafe {
            let sp = (*line).storage.i32_;
            for x in 0..w {
                out[(y * w + x) as usize] = (*sp.add(x as usize)).clamp(0, 255) as u8;
            }
        }
    }
    cs.close();
    out
}

#[test]
fn planar_and_interleaved_gray_roundtrip_match() {
    let w = 32u32;
    let h = 32u32;
    let mut pixels = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            pixels.push(((x * 3 + y * 5) & 0xff) as u8);
        }
    }

    let j2c_planar = encode_planar_gray(w, h, &pixels, true);
    let j2c_inter = encode_planar_gray(w, h, &pixels, false);
    assert!(!j2c_planar.is_empty());
    assert!(!j2c_inter.is_empty());

    let dec_planar = decode_gray(&j2c_planar, w, h);
    let dec_inter = decode_gray(&j2c_inter, w, h);
    assert_eq!(dec_planar, pixels);
    assert_eq!(dec_inter, pixels);
}

#[test]
fn planar_rgb_component_major_roundtrip() {
    let w = 16u32;
    let h = 8u32;
    let num_comps = 3u32;
    let mut pixels = vec![0u8; (w * h * num_comps) as usize];
    for y in 0..h {
        for x in 0..w {
            for c in 0..num_comps {
                let idx = (y * w * num_comps + x * num_comps + c) as usize;
                pixels[idx] = ((x + y * 2 + c * 40) & 0xff) as u8;
            }
        }
    }

    let mut cs = Codestream::new();
    {
        let siz: &mut ParamSiz = cs.access_siz();
        siz.set_image_extent(Point::new(w, h));
        siz.set_tile_size(Size::new(w, h));
        siz.set_num_components(num_comps);
        for c in 0..num_comps {
            siz.set_comp_info(c, Point::new(1, 1), 8, false);
        }
    }
    {
        let cod = cs.access_cod();
        cod.set_reversible(true);
        cod.employ_color_transform(0);
    }
    cs.set_planar(true);

    let mut outfile = MemOutfile::default();
    outfile.open(128 * 1024, false).expect("open");
    cs.write_headers_slice(&mut outfile, &[]).expect("write_headers_slice");

    let mut next_comp: Ui32 = 0;
    let mut cur_line = cs.exchange(std::ptr::null_mut(), &mut next_comp).expect("exchange");
    let mut comp_lines = [0u32; 3];
    while !cur_line.is_null() {
        let comp = next_comp as usize;
        let y = comp_lines[comp];
        unsafe {
            let dp = (*cur_line).storage.i32_;
            for x in 0..w {
                let idx = (y * w * num_comps + x * num_comps + next_comp) as usize;
                *dp.add(x as usize) = pixels[idx] as i32;
            }
        }
        comp_lines[comp] += 1;
        cur_line = cs.exchange(cur_line, &mut next_comp).expect("exchange");
    }

    cs.flush().expect("flush");
    cs.close();
    let j2c = outfile.get_data().to_vec();

    let mut infile = MemInfile::default();
    infile.open(&j2c);
    let mut cs = Codestream::new();
    cs.read_headers(&mut infile).expect("read_headers");
    cs.create().expect("create");

    let mut decoded = vec![0u8; pixels.len()];
    for c in 0..num_comps {
        for y in 0..h {
            let mut comp_num: Ui32 = 0;
            let line = cs.pull(&mut comp_num).expect("pull");
            assert_eq!(comp_num, c);
            unsafe {
                let sp = (*line).storage.i32_;
                for x in 0..w {
                    let idx = (y * w * num_comps + x * num_comps + c) as usize;
                    decoded[idx] = (*sp.add(x as usize)).clamp(0, 255) as u8;
                }
            }
        }
    }
    cs.close();
    assert_eq!(decoded, pixels);
}
