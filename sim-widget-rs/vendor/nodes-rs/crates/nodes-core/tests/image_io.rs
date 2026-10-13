//! LoadImage / SaveImage builtin round-trip: author a tiny RGBA image, load it through a
//! `LoadImage` node (→ `BoxValue::Buffer` RGBA8 [h,w,4]), pipe it into `SaveImage`, and confirm the
//! re-saved file decodes back to the identical pixels.

use nodes_core::{BoxValue, Graph, NodeTypeRegistry, ValueTypes, cook, register_builtins};

// `register_builtins` also registers the builtin VALUE types (they are the `Builtins` library's
// `NodeLibrary::value_types`), so calling `register_builtin_types` beside it would register them
// twice — which now logs an override warning.
fn fixtures() -> (NodeTypeRegistry, ValueTypes) {
    let mut types = ValueTypes::new();
    let mut reg = NodeTypeRegistry::new();
    register_builtins(&mut reg, &mut types);
    (reg, types)
}

#[test]
fn image_load_save_roundtrip() {
    let (registry, types) = fixtures();

    // Author a 2x3 RGBA image (w=2, h=3) with distinct per-pixel colors, write it to a temp PNG.
    // `process::id` keeps the temp names unique across concurrent test binaries.
    let dir = std::env::temp_dir();
    let pid = std::process::id();
    let src = dir.join(format!("nodes_core_img_src_{pid}.png"));
    let dst = dir.join(format!("nodes_core_img_dst_{pid}.png"));
    let (w, h) = (2u32, 3u32);
    let mut authored = image::RgbaImage::new(w, h);
    for (i, px) in authored.pixels_mut().enumerate() {
        let i = i as u8;
        *px = image::Rgba([
            i.wrapping_mul(10),
            i.wrapping_mul(20),
            i.wrapping_mul(30),
            255,
        ]);
    }
    authored.save(&src).expect("write source png");

    // Graph: LoadImage(path=src) -> SaveImage(path=dst).
    let mut graph = Graph::new();
    let load = graph
        .create_node("LoadImage", [0.0, 0.0], &registry, &types)
        .unwrap();
    graph
        .nodes
        .get_mut(&load)
        .unwrap()
        .set_input_value("path", BoxValue::String(src.to_string_lossy().into_owned()));
    let save = graph
        .create_node("SaveImage", [200.0, 0.0], &registry, &types)
        .unwrap();
    graph
        .nodes
        .get_mut(&save)
        .unwrap()
        .set_input_value("path", BoxValue::String(dst.to_string_lossy().into_owned()));
    graph
        .connect(load, "image", save, "image", &registry, &types)
        .unwrap();

    cook(&mut graph, &registry, &types);

    // LoadImage output is an RGBA8 buffer shaped [h, w, 4].
    let out = graph
        .nodes
        .get(&load)
        .unwrap()
        .output_value("image")
        .unwrap();
    let (_dtype, shape, data) = out.as_buffer().expect("LoadImage output must be a Buffer");
    assert_eq!(shape, &[h as usize, w as usize, 4], "shape is [h, w, 4]");
    assert_eq!(data.len(), (w * h * 4) as usize);

    // SaveImage wrote dst; reload it and confirm the pixels round-tripped exactly.
    let reloaded = image::open(&dst).expect("reload saved png").to_rgba8();
    assert_eq!(reloaded.dimensions(), (w, h));
    assert_eq!(
        reloaded.into_raw(),
        authored.into_raw(),
        "pixels survive the round-trip"
    );

    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&dst);
}
