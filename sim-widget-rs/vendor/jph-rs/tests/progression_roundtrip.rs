//! Encoder -> decoder round trip over every progression order x tile-part division, with several
//! tiles, 4 precincts per resolution and subsampled chroma. Guards the packet iterator shared by
//! `Tile::flush` and `Tile::parse_packets` (`walk_pos`): if the writer and reader ever disagree on
//! the packet order, samples come back scrambled and this fails.
mod common;
use jph_rs::mem::{Image, try_decode};
use jph_rs::{Codestream, MemOutfile, Point, Size, Ui32};

fn encode(img: &Image, order: &str, at_res: bool, at_comp: bool) -> Vec<u8> {
    let mut cs = Codestream::new();
    let siz = cs.access_siz();
    siz.set_image_extent(Point::new(img.width, img.height));
    siz.set_tile_size(Size::new(64, 64));
    siz.set_num_components(img.planes.len() as Ui32);
    for (c, p) in img.planes.iter().enumerate() {
        let d = if p.width == img.width { 1 } else { 2 };
        siz.set_comp_info(c as Ui32, Point::new(d, d), 8, false);
    }
    let cod = cs.access_cod();
    cod.set_reversible(true);
    cod.set_num_decompositions(3).expect("decomps");
    cod.set_block_dims(16, 16).expect("blocks");
    cod.set_progression_order(order).expect("order");
    cod.employ_color_transform(0);
    cod.set_precinct_size(&[Size::new(32, 32); 4]).expect("precincts");
    cs.set_planar(true);
    cs.set_tilepart_divisions(at_res, at_comp);

    let mut out = MemOutfile::default();
    out.open(1 << 16, false).expect("open");
    cs.write_headers_slice(&mut out, &[]).expect("headers");
    let mut next_row = vec![0u32; img.planes.len()];
    let mut next_comp: Ui32 = 0;
    let mut line = cs.exchange(core::ptr::null_mut(), &mut next_comp).expect("exchange");
    while !line.is_null() {
        cs.fill_and_exchange(&mut line, &mut next_comp, |l, comp| {
            let p = &img.planes[comp as usize];
            let w = p.width as usize;
            let r = next_row[comp as usize] as usize;
            l.get_i32_mut(w).copy_from_slice(&p.samples[r * w..r * w + w]);
            next_row[comp as usize] += 1;
        })
        .expect("line");
    }
    cs.flush().expect("flush");
    cs.close();
    out.get_data().to_vec()
}

#[test]
fn every_order_and_tilepart_split_roundtrips() {
    let img = Image { width: 150, height: 110, planes: vec![common::synth_plane(150, 110, 1), common::synth_plane(75, 55, 2), common::synth_plane(75, 55, 3)] };
    for order in ["LRCP", "RLCP", "RPCL", "PCRL", "CPRL"] {
        for (at_res, at_comp) in [(false, false), (true, false), (false, true), (true, true)] {
            let bytes = encode(&img, order, at_res, at_comp);
            let back = try_decode(&bytes).unwrap_or_else(|e| panic!("{order} res={at_res} comp={at_comp}: {e}"));
            assert_eq!(back, img, "{order} res={at_res} comp={at_comp}");
        }
    }
}
