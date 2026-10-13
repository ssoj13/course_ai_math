// Viewer backdrop pattern: `egui_display::backdrop`, spliced into the present shader
// (`present.rs`), which composites the canvas over it in linear light before the
// output encoding. The CPU twin is `BackdropUniform::color_at` / `composite`.

// Byte-for-byte the Rust `BackdropUniform` (64 bytes, 16-byte aligned).
struct BackdropUniform {
    // Cell A of the checker, or the solid colour: an sRGB swatch relative to SDR
    // white (the canvas encoding; w unused).
    a: vec4<f32>,
    // Cell B of the checker (a solid backdrop repeats A here).
    b: vec4<f32>,
    // Framebuffer pixel the checker grid is pinned to: cell (0, 0), an A cell,
    // starts here.
    origin_px: vec2<f32>,
    // Cell side in framebuffer pixels; 0 = solid A.
    cell_px: f32,
    pad: f32,
    // Framebuffer pixels the backdrop may show in, [x0, y0, x1, y1): the cleared image
    // rect. Outside it the canvas is presented as it is.
    clip_px: vec4<f32>,
}

// Whether framebuffer pixel `px` lies in the backdrop's clip rect.
fn backdrop_contains(bd: BackdropUniform, px: vec2<f32>) -> bool {
    return all(px >= bd.clip_px.xy) && all(px < bd.clip_px.zw);
}

// The backdrop's swatch (canvas encoding) at framebuffer pixel `frag_px` (a
// fragment's `@builtin(position).xy`).
fn backdrop_color(bd: BackdropUniform, frag_px: vec2<f32>) -> vec3<f32> {
    if bd.cell_px <= 0.0 {
        return bd.a.rgb;
    }
    let cell = floor((frag_px - bd.origin_px) / bd.cell_px);
    // Float parity, not `i32(..) & 1`: exact for every whole cell index a float
    // holds, and it matches the CPU twin's `floor` on negative cells too.
    let s = cell.x + cell.y;
    let odd = s - 2.0 * floor(s * 0.5);
    return select(bd.a.rgb, bd.b.rgb, odd > 0.5);
}
