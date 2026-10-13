//! Image I/O nodes: `LoadImage` (path → Image) and `SaveImage` (Image + path → file).
//!
//! An image is carried as a `BoxValue::Buffer { dtype: U8, shape: [h, w, 4], data }` (RGBA8) — the
//! Arc-shared typed-binary variant box_rs provides for bulk numeric payloads. So it rides wires as a
//! cheap `Arc` clone and is blanked from saves like any other output value (the pixels never bloat
//! the graph JSON). Decode / encode go through the pure-Rust `image` crate (no FFI). The `Image`
//! value type + its pin visual are registered in `value_types.rs`.
//!
//! A cook returns `CookResult<()>`; `?` works, and a returned `Err` becomes the node's
//! `NodeStatus::Error`.

use crate::box_model::DType;
use crate::prelude::*;

/// Image file I/O.
pub fn types() -> Vec<Arc<dyn NodeType>> {
    // LoadImage: decode the file at `path` into an RGBA8 image buffer on the `image` output.
    let load = fn_node(
        NodeDef::new("LoadImage", "IO")
            .with_inputs(vec![string("path")])
            .with_outputs(vec![PortDef::typed("image", "Image")])
            // `FilePath` makes the editor render a browse-button field for the path.
            .with_schema(vec![
                AttrDef::new("path", "String", sv(""))
                    .ui_hint(UiHint::FilePath)
                    .description("Image file to load."),
            ]),
        |ctx| {
            let path = ctx.node.in_str("path");
            if path.trim().is_empty() {
                // No path yet — emit an empty image rather than erroring on a fresh node.
                ctx.node.set_out("image", BoxValue::Null);
                return Ok(());
            }
            let img = ::image::open(&path)
                .map_err(|e| CookError::Custom(format!("load image '{path}': {e}")))?
                .to_rgba8();
            let (w, h) = (img.width() as usize, img.height() as usize);
            ctx.node.set_out(
                "image",
                BoxValue::buffer(DType::U8, vec![h, w, 4], img.into_raw()),
            );
            Ok(())
        },
    );

    // SaveImage: write the incoming RGBA8 image buffer to `path` (format inferred from the
    // extension). A sink node (no outputs); it runs for its side effect during cook.
    let save = fn_node(
        NodeDef::new("SaveImage", "IO")
            .with_inputs(vec![PortDef::typed("image", "Image"), string("path")])
            .with_schema(vec![
                AttrDef::new("path", "String", sv(""))
                    .ui_hint(UiHint::FilePath)
                    .description("Destination image file (format from the extension)."),
            ]),
        |ctx| {
            let path = ctx.node.in_str("path");
            let img_val = ctx.node.cook_value("image");
            let buf = img_val.as_ref().and_then(BoxValue::as_buffer);
            let Some((_dtype, shape, data)) = buf else {
                // Nothing to write without an image wired in — no-op.
                return Ok(());
            };
            if path.trim().is_empty() {
                return Ok(()); // no target path yet — no-op, not an error
            }
            if shape.len() != 3 || shape[2] != 4 {
                return Err(CookError::Custom(format!(
                    "SaveImage expects an RGBA8 image (shape [h,w,4]), got {shape:?}"
                )));
            }
            let (h, w) = (shape[0] as u32, shape[1] as u32);
            let im = ::image::RgbaImage::from_raw(w, h, data.to_vec()).ok_or_else(|| {
                CookError::Custom("SaveImage: pixel buffer size does not match its shape".into())
            })?;
            im.save(&path)
                .map_err(|e| CookError::Custom(format!("save image '{path}': {e}")))
        },
    );

    vec![load, save]
}
