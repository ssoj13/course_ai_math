//! The viewer backdrop: what shows under the image's alpha.
//!
//! **Semantics (the contract every host keeps).** The backdrop is display-referred
//! viewer chrome. The image composites OVER it after the view transform, in linear
//! display light relative to SDR reference white, inside the present pass and before
//! the output encoding ([`crate::PresentPass`]): so it is right on every
//! [`crate::Output`], and on an HDR display it sits at SDR-white-relative levels
//! instead of blinding. It is visible only where the image is not opaque, and only
//! under the image. It never reaches caches, exports, scopes or OCIO: it exists only in
//! the frame on screen (and in a screenshot of it).
//!
//! **Colours** are sRGB swatches relative to SDR white, the canvas encoding of
//! [`crate::transfer`] (1.0 = [`crate::Target::white`] nits): what the colour picker
//! shows. The default checker cells are 0.18 and 0.25 of reference white in linear
//! light; "50% Gray" is the code value 0.5 (0.214 of white in linear light).
//!
//! **The canvas contract (what a host writes into the image rect).** Under the backdrop
//! the canvas holds *encoded associated light*: for a pixel of linear, premultiplied
//! (associated) light `L = straight colour x alpha` and coverage `a`, the canvas stores
//! `rgb = oetf(L)` (the canvas encoding of [`crate::transfer`], extended range) and
//! alpha `a`. The present pass then needs no division by alpha:
//! ```text
//! out = oetf( eotf(rgb) + backdrop_lin * (1 - a) )        (a clamped to 0..)
//! ```
//! which is Porter-Duff OVER in linear light for associated alpha. Consequences: an
//! emissive pixel (`a = 0`, `rgb > 0`: a lens flare, a glow, fire) keeps its light; the
//! result is continuous as `a -> 0`; opaque pixels (`a >= 1`) are untouched, so a host
//! whose image is opaque pays nothing. The tile blend onto the cleared canvas is
//! `src + dst * (1 - src.a)` (egui's own), which preserves this representation.
//!
//! *egui's own UI* drawn in the same canvas is `rgb = encoded straight colour x alpha`
//! (gamma-space premultiplication). The two conventions agree exactly where it matters
//! for correctness of the picture: at `a = 1` (opaque UI, and every UI pixel over an
//! opaque image or panel; egui blends there in encoded space as it always did) and at
//! `a = 0`. They differ only for a UI pixel of partial alpha `u` with colour `S`, over
//! a canvas pixel that is not opaque (UI over a transparent part of the image, inside
//! the cleared rect): the backdrop sees `eotf(enc(S) x u)` instead of `eotf(enc(S)) x u`.
//! Black and dark fills are exact (`S = 0`); a white UI pixel reads 0.215 instead of 0.5
//! of SDR white at `u = 0.5`, 0.05 instead of 0.25 at `u = 0.25`, 0.79 instead of 0.9 at
//! `u = 0.9`: only the anti-aliased edges of light text and light translucent fills over
//! transparent image areas are darker. A host that needs them exact draws that UI
//! opaque, or after [`crate::PresentPass`] into the surface.
//!
//! **How a host shows it (the PresentPass path).**
//! ```text
//! viewport:  show_backdrop(painter, image_rect, &backdrop, &view)
//!              -> a paint callback clears image_rect of the canvas to alpha 0, so the
//!                 image drawn next leaves its own alpha there (associated, see above)
//!              -> publishes the frame's BackdropUniform in the egui context
//! host:      present.backdrop = take_backdrop(&ctx); present.draw(..)
//!              -> where canvas alpha < 1: decode, add the backdrop x (1 - alpha) in
//!                 linear light, re-encode, then the output's own encoding
//! ```
//! [`BackdropUniform::composite`] is the CPU twin; [`crate::encode_over`] adds the
//! output encoding. Hosts without a [`crate::PresentPass`] (a plain eframe app) use
//! [`paint_backdrop`], an SDR-only painter path.
//!
//! **Checker scale.** By default ([`CheckerScale::Screen`]) a checker cell is
//! [`Checker::size_px`] physical screen pixels at any zoom, pinned to the viewport's
//! top-left corner, so panning or zooming the image never makes it crawl or shimmer.
//! [`CheckerScale::Image`] makes a cell `size_px` IMAGE pixels instead: it is pinned
//! to the image's corner and zooms with it.

use serde::{Deserialize, Serialize};

use crate::transfer::{eotf, oetf};

/// The WGSL of the pattern: the [`BackdropUniform`] struct and `backdrop_color`,
/// spliced into the present shader.
pub(crate) const BACKDROP_WGSL: &str = include_str!("backdrop.wgsl");

/// Which backdrop is shown. The menu order is [`Self::ALL`]; [`Self::next`] cycles it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BackdropKind {
    /// The transparency checkerboard ([`Backdrop::checker`]).
    #[default]
    Checker,
    /// Black.
    Black,
    /// The code value 0.5 (0.214 of SDR white in linear light).
    Gray50,
    /// The user's colour ([`Backdrop::color`]).
    Color,
}

impl BackdropKind {
    /// Every kind, in menu and cycle order.
    pub const ALL: [Self; 4] = [Self::Checker, Self::Black, Self::Gray50, Self::Color];

    /// UI name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Checker => "Checker",
            Self::Black => "Black",
            Self::Gray50 => "50% Gray",
            Self::Color => "Custom",
        }
    }

    /// Phosphor glyph (the host registers the font, as for every icon here).
    pub fn icon(self) -> &'static str {
        use egui_widgets_config::icons;
        match self {
            Self::Checker => icons::BACKDROP_CHECKER,
            Self::Black => icons::BACKDROP_BLACK,
            Self::Gray50 => icons::BACKDROP_GRAY,
            Self::Color => icons::BACKDROP_CUSTOM,
        }
    }

    /// The kind after this one in [`Self::ALL`], wrapping: the hotkey's step.
    pub fn next(self) -> Self {
        match self {
            Self::Checker => Self::Black,
            Self::Black => Self::Gray50,
            Self::Gray50 => Self::Color,
            Self::Color => Self::Checker,
        }
    }
}

/// How a checker cell's size is measured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CheckerScale {
    /// Physical screen pixels, pinned to the viewport: fixed at any zoom (the default).
    #[default]
    Screen,
    /// Image pixels, pinned to the image's corner: the checker zooms with the image.
    Image,
}

/// The checkerboard's parameters.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Checker {
    /// Cell side: physical screen pixels, or image pixels under [`CheckerScale::Image`].
    pub size_px: f32,
    /// The cell at the grid's origin: an sRGB swatch relative to SDR white (alpha
    /// unused, the backdrop is opaque).
    pub a: [f32; 4],
    /// The other cell, the same way.
    pub b: [f32; 4],
    /// How [`Self::size_px`] is measured.
    pub scale: CheckerScale,
}

impl Checker {
    /// The cell sizes the size slider offers; [`Backdrop::grid`] clamps to it.
    pub const SIZE: std::ops::RangeInclusive<f32> = 2.0..=128.0;
}

impl Default for Checker {
    /// 16-pixel screen-fixed cells at 0.18 and 0.25 of SDR white in linear light (codes
    /// 0.461 / 0.537): readable as transparency, and dim on an HDR display, where SDR
    /// white is the reference white, not the peak.
    fn default() -> Self {
        let level = |l: f32| {
            let c = oetf(l);
            [c, c, c, 1.0]
        };
        Self {
            size_px: 16.0,
            a: level(0.18),
            b: level(0.25),
            scale: CheckerScale::Screen,
        }
    }
}

/// The viewer backdrop setting the host persists.
///
/// One struct rather than a data-carrying enum so switching kind never forgets a
/// parameter: cycling Checker -> Black -> Checker keeps the checker's size, and the
/// custom colour survives a visit to the presets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Backdrop {
    /// What is shown.
    pub kind: BackdropKind,
    /// The checkerboard, used when [`Self::kind`] is [`BackdropKind::Checker`].
    pub checker: Checker,
    /// The custom colour: an sRGB swatch relative to SDR white (alpha unused), used
    /// when [`Self::kind`] is [`BackdropKind::Color`].
    pub color: [f32; 4],
}

/// Where the viewport and the image sit on screen this frame: what pins the checker.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BackdropView {
    /// The viewport's top-left corner, egui points.
    pub viewport_min: egui::Pos2,
    /// The image's top-left corner on screen, egui points.
    pub image_min: egui::Pos2,
    /// Screen points per image pixel (the viewer's zoom).
    pub zoom: f32,
    /// Physical pixels per egui point (`Context::pixels_per_point`).
    pub pixels_per_point: f32,
}

impl BackdropView {
    /// A view whose image sits at the viewport's corner at zoom 1: enough for
    /// [`CheckerScale::Screen`], which reads only the viewport.
    pub fn new(viewport_min: egui::Pos2, pixels_per_point: f32) -> Self {
        Self {
            viewport_min,
            image_min: viewport_min,
            zoom: 1.0,
            pixels_per_point,
        }
    }

    /// The image's on-screen corner and zoom (points per image pixel), which
    /// [`CheckerScale::Image`] pins to.
    pub fn with_image(mut self, image_min: egui::Pos2, zoom: f32) -> Self {
        self.image_min = image_min;
        self.zoom = zoom;
        self
    }
}

/// The checker grid on screen, in framebuffer (physical) pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    /// The pixel cell (0, 0) starts at; that cell is [`Checker::a`].
    pub origin_px: [f32; 2],
    /// Cell side, at least one pixel.
    pub cell_px: f32,
}

/// Black.
const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
/// The code value 0.5.
const GRAY50: [f32; 4] = [0.5, 0.5, 0.5, 1.0];

impl Backdrop {
    /// UI name of the current kind.
    pub fn label(&self) -> &'static str {
        self.kind.label()
    }

    /// Phosphor glyph of the current kind.
    pub fn icon(&self) -> &'static str {
        self.kind.icon()
    }

    /// Step to the next kind (the hotkey), keeping every parameter.
    pub fn cycle(&mut self) {
        self.kind = self.kind.next();
    }

    /// The solid swatch shown, or `None` for the checker.
    pub fn solid(&self) -> Option<[f32; 4]> {
        match self.kind {
            BackdropKind::Checker => None,
            BackdropKind::Black => Some(BLACK),
            BackdropKind::Gray50 => Some(GRAY50),
            BackdropKind::Color => Some(self.color),
        }
    }

    /// The two colours a preview swatch shows, as egui's 8-bit sRGB: the checker's
    /// cells, or the solid colour twice.
    pub fn swatch(&self) -> [egui::Color32; 2] {
        match self.solid() {
            Some(c) => [color32(c); 2],
            None => [color32(self.checker.a), color32(self.checker.b)],
        }
    }

    /// The checker grid for `view`: pinned to the viewport and `size_px` pixels under
    /// [`CheckerScale::Screen`], to the image and `size_px * zoom` under
    /// [`CheckerScale::Image`]. Never below one pixel: a sub-pixel checker is a moire,
    /// not a backdrop.
    pub fn grid(&self, view: &BackdropView) -> Grid {
        let ppp = view.pixels_per_point;
        let size = self
            .checker
            .size_px
            .clamp(*Checker::SIZE.start(), *Checker::SIZE.end());
        let (origin, cell) = match self.checker.scale {
            CheckerScale::Screen => (view.viewport_min, size),
            CheckerScale::Image => (view.image_min, size * view.zoom * ppp),
        };
        Grid {
            origin_px: [origin.x * ppp, origin.y * ppp],
            // `f32::max` drops a NaN (a zero or broken zoom) to the one-pixel floor.
            cell_px: cell.max(1.0),
        }
    }

    /// The present pass's uniform for `view`, shown only inside `rect` (egui points; the
    /// image's on-screen rect, as [`show_backdrop`] clears it).
    pub fn uniform(&self, view: &BackdropView, rect: egui::Rect) -> BackdropUniform {
        // Rounded the way egui-wgpu rounds a paint callback's viewport, so the composite
        // covers exactly the pixels the clear did.
        let ppp = view.pixels_per_point;
        let px = |v: f32| (v * ppp).round();
        let clip_px = [
            px(rect.min.x),
            px(rect.min.y),
            px(rect.max.x),
            px(rect.max.y),
        ];
        match self.solid() {
            Some(c) => BackdropUniform {
                a: c,
                b: c,
                origin_px: [0.0; 2],
                cell_px: 0.0,
                pad: 0.0,
                clip_px,
            },
            None => {
                let grid = self.grid(view);
                BackdropUniform {
                    a: self.checker.a,
                    b: self.checker.b,
                    origin_px: grid.origin_px,
                    cell_px: grid.cell_px,
                    pad: 0.0,
                    clip_px,
                }
            }
        }
    }
}

/// The backdrop as the present shader sees it: the WGSL `BackdropUniform`, byte for
/// byte (64 bytes, 16-byte aligned). Build it with [`Backdrop::uniform`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct BackdropUniform {
    /// Cell A, or the solid colour (an sRGB swatch relative to SDR white; alpha unused).
    pub a: [f32; 4],
    /// Cell B (a solid backdrop repeats A).
    pub b: [f32; 4],
    /// Framebuffer pixel the grid is pinned to.
    pub origin_px: [f32; 2],
    /// Cell side in framebuffer pixels; 0 = solid A.
    pub cell_px: f32,
    /// Keeps `clip_px` 16-byte aligned; always 0.
    pub pad: f32,
    /// The framebuffer pixels the backdrop may show in, `[x0, y0, x1, y1)`: the cleared
    /// image rect. Outside it a non-opaque canvas pixel is presented as it is, so a
    /// translucent UI fill on a canvas that is not opaque underneath never shows the
    /// backdrop.
    pub clip_px: [f32; 4],
}

impl BackdropUniform {
    /// The backdrop's swatch at framebuffer pixel `px`: the WGSL `backdrop_color`.
    pub fn color_at(&self, px: [f32; 2]) -> [f32; 3] {
        let [r, g, b, _] = if self.cell_px <= 0.0 {
            self.a
        } else {
            let cx = ((px[0] - self.origin_px[0]) / self.cell_px).floor();
            let cy = ((px[1] - self.origin_px[1]) / self.cell_px).floor();
            let s = cx + cy;
            if s - 2.0 * (s * 0.5).floor() > 0.5 {
                self.b
            } else {
                self.a
            }
        };
        [r, g, b]
    }

    /// Whether framebuffer pixel `px` lies in [`Self::clip_px`].
    pub fn contains(&self, px: [f32; 2]) -> bool {
        let [x0, y0, x1, y1] = self.clip_px;
        px[0] >= x0 && px[0] < x1 && px[1] >= y0 && px[1] < y1
    }

    /// A canvas pixel (encoded associated light, see the module docs; NaN-free RGB) OVER
    /// the backdrop at framebuffer pixel `px`, in linear light, returned in the canvas
    /// encoding: the present shader's composite. Opaque pixels (alpha >= 1, or a NaN
    /// alpha) and pixels outside [`Self::clip_px`] pass unchanged; alpha below 0 counts
    /// as 0. No division by alpha: `a = 0` with `rgb > 0` is emission and stays.
    pub fn composite(&self, canvas: [f32; 4], px: [f32; 2]) -> [f32; 3] {
        let [r, g, b, a] = canvas;
        if a.is_nan() || a >= 1.0 || !self.contains(px) {
            return [r, g, b];
        }
        let a = a.max(0.0);
        let bd = self.color_at(px).map(eotf);
        let rgb = [r, g, b];
        std::array::from_fn(|i| oetf(eotf(rgb[i]) + bd[i] * (1.0 - a)))
    }
}

/// The frame's backdrop in the egui context, from [`show_backdrop`] to [`take_backdrop`].
fn published() -> egui::Id {
    egui::Id::new("egui_display.backdrop")
}

/// Show `backdrop` under `rect` (the image's on-screen rect) on a [`crate::PresentPass`]
/// canvas. Call it where the backdrop belongs in the paint order: before the image.
///
/// Adds a paint callback that clears `rect` of the canvas to transparent, so the image
/// drawn next leaves its own (associated) alpha there, and publishes the frame's
/// [`BackdropUniform`] for the host, which hands it to the pass with [`take_backdrop`].
/// One backdrop per frame: a second call replaces the published uniform. Needs the
/// host's egui-wgpu renderer to target [`crate::CANVAS_FORMAT`] without MSAA, which is
/// what a `PresentPass` host does.
pub fn show_backdrop(
    painter: &egui::Painter,
    rect: egui::Rect,
    backdrop: &Backdrop,
    view: &BackdropView,
) {
    let shown = rect.intersect(painter.clip_rect());
    if !shown.is_positive() {
        return;
    }
    painter.add(egui_wgpu::Callback::new_paint_callback(
        shown,
        ClearToBackdrop,
    ));
    let uniform = backdrop.uniform(view, shown);
    painter
        .ctx()
        .data_mut(|d| d.insert_temp(published(), uniform));
}

/// The backdrop [`show_backdrop`] published this frame, removed so a frame that shows
/// none presents none. The host calls it once per frame, after the UI ran, and sets
/// [`crate::PresentPass::backdrop`] from it.
pub fn take_backdrop(ctx: &egui::Context) -> Option<BackdropUniform> {
    ctx.data_mut(|d| d.remove_temp::<BackdropUniform>(published()))
}

/// The paint callback of [`show_backdrop`]: one triangle over the callback's viewport
/// writing transparent black with blending off.
struct ClearToBackdrop;

/// Its pipeline, made once per renderer and kept in the callback resources.
struct ClearPipeline(wgpu::RenderPipeline);

const CLEAR_WGSL: &str = "
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}
@fragment fn fs() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0);
}";

impl egui_wgpu::CallbackTrait for ClearToBackdrop {
    fn prepare(
        &self,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if resources.get::<ClearPipeline>().is_none() {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("egui_display.backdrop_clear"),
                source: wgpu::ShaderSource::Wgsl(CLEAR_WGSL.into()),
            });
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("egui_display.backdrop_clear"),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: crate::CANVAS_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            });
            resources.insert(ClearPipeline(pipeline));
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        // egui-wgpu has set the viewport to the callback rect and the scissor to its clip.
        if let Some(ClearPipeline(pipeline)) = resources.get::<ClearPipeline>() {
            render_pass.set_pipeline(pipeline);
            render_pass.draw(0..3, 0..1);
        }
    }
}

/// The SDR-only painter path, for a host without a [`crate::PresentPass`] (a plain
/// eframe app): paints `backdrop` over `rect` (the image's on-screen rect) for the
/// image to be drawn on top with the app's own blend. Composites in the canvas
/// encoding, not linear light, and clips to `[0, 1]`: a `PresentPass` host uses
/// [`show_backdrop`] instead.
///
/// A solid backdrop is one filled rect; the checker is ONE textured quad (a 2x2
/// repeat-wrapped, nearest-sampled tile scaled to the cell), not a shape per cell.
pub fn paint_backdrop(
    painter: &egui::Painter,
    rect: egui::Rect,
    backdrop: &Backdrop,
    view: &BackdropView,
) {
    let shown = rect.intersect(painter.clip_rect());
    if !shown.is_positive() {
        return;
    }
    if let Some(c) = backdrop.solid() {
        painter.rect_filled(shown, 0.0, color32(c));
        return;
    }
    let grid = backdrop.grid(view);
    let ppp = view.pixels_per_point;
    let texture = checker_tile(painter.ctx(), backdrop.checker.a, backdrop.checker.b);
    // The tile is two cells wide; a point maps to its distance from the grid origin in
    // tiles, so the quad's UVs pin the pattern to the origin wherever `rect` lies.
    let tile_pt = 2.0 * grid.cell_px / ppp;
    let origin = egui::pos2(grid.origin_px[0] / ppp, grid.origin_px[1] / ppp);
    let uv = |p: egui::Pos2| egui::pos2((p.x - origin.x) / tile_pt, (p.y - origin.y) / tile_pt);
    let mut mesh = egui::Mesh::with_texture(texture.id());
    mesh.add_rect_with_uv(
        shown,
        egui::Rect::from_min_max(uv(shown.min), uv(shown.max)),
        egui::Color32::WHITE,
    );
    painter.add(mesh);
}

/// A swatch as egui's 8-bit sRGB (opaque; clipped to `[0, 1]`).
fn color32(c: [f32; 4]) -> egui::Color32 {
    let code = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    egui::Color32::from_rgb(code(c[0]), code(c[1]), code(c[2]))
}

/// The last checker tile made, by its two colours: one slot, rebuilt when they change.
#[derive(Clone, Default)]
struct TileSlot(Option<([egui::Color32; 2], egui::TextureHandle)>);

/// The 2x2 checker tile of `a` / `b` (A at texel (0, 0) and (1, 1)), repeat-wrapped
/// and nearest-sampled so a magnified cell stays a crisp square.
fn checker_tile(ctx: &egui::Context, a: [f32; 4], b: [f32; 4]) -> egui::TextureHandle {
    let id = egui::Id::new("egui_display.backdrop_tile");
    let key = [color32(a), color32(b)];
    if let Some((k, tex)) = ctx.data(|d| d.get_temp::<TileSlot>(id)).and_then(|s| s.0)
        && k == key
    {
        return tex;
    }
    let image = egui::ColorImage::new([2, 2], vec![key[0], key[1], key[1], key[0]]);
    let tex = ctx.load_texture(
        "egui_display.backdrop_checker",
        image,
        egui::TextureOptions {
            magnification: egui::TextureFilter::Nearest,
            minification: egui::TextureFilter::Nearest,
            wrap_mode: egui::TextureWrapMode::Repeat,
            ..Default::default()
        },
    );
    ctx.data_mut(|d| d.insert_temp(id, TileSlot(Some((key, tex.clone())))));
    tex
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_round_trips_every_kind_and_fills_missing_fields() {
        for kind in BackdropKind::ALL {
            let b = Backdrop {
                kind,
                checker: Checker {
                    size_px: 24.0,
                    a: [0.1, 0.2, 0.3, 1.0],
                    b: [0.4, 0.5, 0.6, 1.0],
                    scale: CheckerScale::Image,
                },
                color: [0.9, 0.1, 0.4, 1.0],
            };
            let json = serde_json::to_string(&b).expect("serialize");
            let back: Backdrop = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, b);
        }
        // A config that only names the kind loads the rest from the defaults.
        let back: Backdrop = serde_json::from_str(r#"{"kind":"Gray50"}"#).expect("partial");
        assert_eq!(back.kind, BackdropKind::Gray50);
        assert_eq!(back.checker, Checker::default());
        let back: Backdrop = serde_json::from_str("{}").expect("empty");
        assert_eq!(back, Backdrop::default());
        assert_eq!(
            back.kind,
            BackdropKind::Checker,
            "the default is the checker"
        );
        assert_eq!(back.checker.scale, CheckerScale::Screen);
    }

    #[test]
    fn cycling_visits_every_kind_and_keeps_the_parameters() {
        let mut b = Backdrop {
            checker: Checker {
                size_px: 40.0,
                ..Checker::default()
            },
            color: [0.7, 0.2, 0.1, 1.0],
            ..Backdrop::default()
        };
        let mut seen = Vec::new();
        for _ in 0..BackdropKind::ALL.len() {
            seen.push(b.kind);
            b.cycle();
        }
        assert_eq!(seen, BackdropKind::ALL, "one lap in menu order");
        assert_eq!(b.kind, BackdropKind::Checker, "and back to the start");
        assert_eq!(b.checker.size_px, 40.0);
        assert_eq!(b.color, [0.7, 0.2, 0.1, 1.0]);
    }

    /// The levels the docs promise: checker cells at 0.18 / 0.25 of SDR white in linear
    /// light, 50% Gray the code value 0.5, black 0.
    #[test]
    fn default_levels_are_sdr_white_relative() {
        let c = Checker::default();
        assert!((eotf(c.a[0]) - 0.18).abs() < 1e-6, "{:?}", c.a);
        assert!((eotf(c.b[0]) - 0.25).abs() < 1e-6, "{:?}", c.b);
        let mut b = Backdrop {
            color: [0.25, 0.5, 0.75, 1.0],
            ..Backdrop::default()
        };
        assert_eq!(b.solid(), None);
        b.kind = BackdropKind::Black;
        assert_eq!(b.solid(), Some([0.0, 0.0, 0.0, 1.0]));
        b.kind = BackdropKind::Gray50;
        assert_eq!(b.solid(), Some([0.5, 0.5, 0.5, 1.0]), "code value 0.5");
        b.kind = BackdropKind::Color;
        assert_eq!(b.solid(), Some([0.25, 0.5, 0.75, 1.0]));
    }

    #[test]
    fn uniform_matches_the_wgsl_layout() {
        assert_eq!(std::mem::size_of::<BackdropUniform>(), 64);
        let rect = egui::Rect::from_min_max(egui::pos2(10.25, 20.0), egui::pos2(50.0, 60.75));
        let u = Backdrop::default().uniform(&BackdropView::new(egui::pos2(10.0, 20.0), 2.0), rect);
        let floats: &[f32] = bytemuck::cast_slice(bytemuck::bytes_of(&u));
        assert_eq!(
            &floats[8..12],
            &[20.0, 40.0, 16.0, 0.0],
            "origin_px, cell_px, pad"
        );
        // Rounded like egui-wgpu's callback viewport: 20.5 -> 21, 121.5 -> 122.
        assert_eq!(&floats[12..16], &[21.0, 40.0, 100.0, 122.0], "clip_px");
    }

    /// Screen scale: physical pixels at the viewport, whatever the zoom. Image scale:
    /// the image's corner, `size * zoom` pixels.
    #[test]
    fn grid_is_screen_fixed_by_default_and_zooms_under_image_scale() {
        let view =
            BackdropView::new(egui::pos2(10.0, 20.0), 1.5).with_image(egui::pos2(-30.0, 5.0), 4.0);
        let mut b = Backdrop::default();
        assert_eq!(
            b.grid(&view),
            Grid {
                origin_px: [15.0, 30.0],
                cell_px: 16.0
            }
        );
        b.checker.scale = CheckerScale::Image;
        assert_eq!(
            b.grid(&view),
            Grid {
                origin_px: [-45.0, 7.5],
                cell_px: 16.0 * 4.0 * 1.5
            }
        );
        // A zoomed-out image never makes cells smaller than a pixel.
        let far = view.with_image(egui::pos2(0.0, 0.0), 0.001);
        assert_eq!(b.grid(&far).cell_px, 1.0);
        let broken = view.with_image(egui::pos2(0.0, 0.0), f32::NAN);
        assert_eq!(b.grid(&broken).cell_px, 1.0);
    }

    /// The pattern's parity (negative cells too) and the composite against hand-worked
    /// linear-light values.
    #[test]
    fn composite_is_over_in_linear_light() {
        let a = oetf(0.2);
        let b = oetf(0.6);
        let u = BackdropUniform {
            a: [a, a, a, 1.0],
            b: [b, b, b, 1.0],
            origin_px: [100.0, 100.0],
            cell_px: 10.0,
            pad: 0.0,
            clip_px: [0.0, 0.0, 200.0, 200.0],
        };
        assert_eq!(u.color_at([100.5, 100.5]), [a; 3], "origin cell is A");
        assert_eq!(u.color_at([110.5, 100.5]), [b; 3]);
        assert_eq!(u.color_at([110.5, 110.5]), [a; 3]);
        assert_eq!(u.color_at([99.5, 100.5]), [b; 3], "cell -1 is B");
        assert_eq!(
            u.color_at([89.5, 99.5]),
            [b; 3],
            "cell (-2, -1): odd sum, B"
        );
        // Straight linear [0.5, 0, 4.0] at alpha 0.25 = associated light [0.125, 0, 1.0],
        // stored encoded. Over A (linear 0.2): light + 0.2 * 0.75.
        let canvas = [oetf(0.125), 0.0, oetf(1.0), 0.25];
        let got = u.composite(canvas, [100.5, 100.5]);
        let want = [0.125 + 0.15, 0.15, 1.0 + 0.15].map(oetf);
        for i in 0..3 {
            assert!((got[i] - want[i]).abs() < 1e-5, "{got:?} vs {want:?}");
        }
        // Opaque passes unchanged (even above white); clear shows the backdrop.
        assert_eq!(
            u.composite([0.3, 1.7, 0.0, 1.0], [110.5, 100.5]),
            [0.3, 1.7, 0.0]
        );
        let clear = u.composite([0.0, 0.0, 0.0, 0.0], [110.5, 100.5]);
        assert!(clear.iter().all(|c| (c - b).abs() < 1e-6), "{clear:?}");
        assert_eq!(u.composite([0.4, 0.4, 0.4, f32::NAN], [0.0, 0.0]), [0.4; 3]);
        // Outside the cleared rect a translucent canvas pixel is presented as it is: no
        // backdrop bleeds into UI that is not over the image.
        assert_eq!(u.composite([0.2, 0.2, 0.2, 0.5], [200.5, 100.5]), [0.2; 3]);
        assert_eq!(u.composite([0.2, 0.2, 0.2, 0.5], [100.5, -0.5]), [0.2; 3]);
    }

    /// A solid or checker backdrop uniform whose clip covers the 64 x 64 test area.
    fn uniform_of(backdrop: Backdrop) -> BackdropUniform {
        backdrop.uniform(
            &BackdropView::new(egui::Pos2::ZERO, 1.0),
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(64.0, 64.0)),
        )
    }

    /// The three backdrop looks the tests run over: Black, 50% Gray, the checker.
    fn looks() -> [BackdropUniform; 3] {
        [
            BackdropKind::Black,
            BackdropKind::Gray50,
            BackdropKind::Checker,
        ]
        .map(|kind| {
            uniform_of(Backdrop {
                kind,
                ..Backdrop::default()
            })
        })
    }

    /// Straight linear colour `s` at coverage `a`, stored the way the contract says:
    /// encoded associated light.
    fn assoc(s: [f32; 3], a: f32) -> [f32; 4] {
        let [r, g, b] = s.map(|v| oetf(v * a));
        [r, g, b, a]
    }

    /// An emissive pixel (a lens flare: light with no coverage) keeps all its light over
    /// every backdrop: OVER is `light + backdrop * (1 - 0)`. The old straight-colour
    /// formula dropped it (`a == 0` -> 0) or blew it up (`c / a`).
    #[test]
    fn emission_at_zero_alpha_keeps_its_light() {
        for u in looks() {
            for px in [[0.5, 0.5], [17.5, 0.5]] {
                let bd = u.color_at(px).map(eotf);
                let light = [2.0f32, 0.5, 0.01];
                let got = u.composite([oetf(light[0]), oetf(light[1]), oetf(light[2]), 0.0], px);
                for i in 0..3 {
                    let want = oetf(light[i] + bd[i]);
                    assert!((got[i] - want).abs() < 1e-5, "{px:?} ch{i}: {got:?}");
                }
            }
        }
    }

    /// A tiny alpha is continuous with zero: the result moves by about the coverage,
    /// and a huge light never explodes (the old `eotf(c / a)` of c = 3, a = 1e-6).
    #[test]
    fn tiny_alpha_is_continuous_with_zero() {
        let u = uniform_of(Backdrop {
            kind: BackdropKind::Gray50,
            ..Backdrop::default()
        });
        let px = [1.5, 1.5];
        for light in [0.0f32, 0.3, 4.0, 60.0] {
            let c = [oetf(light); 3];
            let zero = u.composite([c[0], c[1], c[2], 0.0], px);
            let mut prev = f32::MAX;
            for a in [1e-3f32, 1e-5, 1e-7] {
                let got = u.composite([c[0], c[1], c[2], a], px);
                let d = (got[0] - zero[0]).abs();
                assert!(
                    d.is_finite() && d <= prev,
                    "light {light} a {a}: {got:?} vs {zero:?}"
                );
                assert!(d < 1e-3 * (1.0 + zero[0].abs()), "light {light} a {a}: {d}");
                prev = d;
            }
        }
    }

    /// An ordinary partial-alpha image pixel: straight colour `s` at coverage `a` over
    /// the backdrop is `s * a + backdrop * (1 - a)` in linear light, computed here
    /// from the straight colour, independently of how the canvas stores it.
    #[test]
    fn partial_alpha_is_porter_duff_over_in_linear_light() {
        for u in looks() {
            for px in [[3.5, 3.5], [19.5, 3.5], [19.5, 19.5]] {
                let bd = u.color_at(px).map(eotf);
                for a in [0.05f32, 0.25, 0.5, 0.9] {
                    let s = [0.8f32, 0.1, 3.0];
                    let got = u.composite(assoc(s, a), px);
                    for i in 0..3 {
                        let want = oetf(s[i] * a + bd[i] * (1.0 - a));
                        assert!((got[i] - want).abs() < 1e-4, "a {a} ch{i}: {got:?}");
                    }
                }
            }
        }
    }

    /// Every shape `paint_backdrop` emits for `rect`.
    fn painted(backdrop: &Backdrop, rect: egui::Rect, view: &BackdropView) -> Vec<egui::Shape> {
        let ctx = egui::Context::default();
        ctx.begin_pass(egui::RawInput::default());
        let painter = ctx
            .layer_painter(egui::LayerId::background())
            .with_clip_rect(egui::Rect::EVERYTHING);
        paint_backdrop(&painter, rect, backdrop, view);
        let mut out = ctx.end_pass();
        // No GPU here: the uploads are dropped explicitly (epaint asserts otherwise).
        out.textures_delta.clear();
        out.shapes.into_iter().map(|c| c.shape).collect()
    }

    /// The checker is one quad whose UVs measure tiles (two cells) from the grid origin;
    /// a solid is one rect.
    #[test]
    fn painter_path_is_one_quad_pinned_to_the_grid() {
        let view = BackdropView::new(egui::pos2(100.0, 50.0), 1.0);
        let rect = egui::Rect::from_min_max(egui::pos2(132.0, 50.0), egui::pos2(432.0, 250.0));
        let shapes = painted(&Backdrop::default(), rect, &view);
        assert_eq!(shapes.len(), 1, "one shape");
        let egui::Shape::Mesh(mesh) = &shapes[0] else {
            panic!("the checker is a mesh, got {:?}", shapes[0]);
        };
        assert_eq!(mesh.vertices.len(), 4, "one quad");
        let u_min = mesh
            .vertices
            .iter()
            .map(|v| v.uv.x)
            .fold(f32::INFINITY, f32::min);
        // 32 points right of the origin with 16-pixel cells = one whole tile.
        assert!((u_min - 1.0).abs() < 1e-6, "u_min {u_min}");
        let gray = Backdrop {
            kind: BackdropKind::Gray50,
            ..Backdrop::default()
        };
        let shapes = painted(&gray, rect, &view);
        assert_eq!(shapes.len(), 1);
        assert!(
            matches!(shapes[0], egui::Shape::Rect(_)),
            "a solid is one rect"
        );
    }

    /// `show_backdrop` adds one paint callback over the visible rect and publishes the
    /// uniform; `take_backdrop` hands it over once.
    #[test]
    fn show_backdrop_clears_its_rect_and_publishes_once() {
        let ctx = egui::Context::default();
        ctx.begin_pass(egui::RawInput::default());
        let painter = ctx
            .layer_painter(egui::LayerId::background())
            .with_clip_rect(egui::Rect::from_min_max(
                egui::pos2(0.0, 0.0),
                egui::pos2(200.0, 200.0),
            ));
        let view = BackdropView::new(egui::pos2(0.0, 0.0), 2.0);
        let rect = egui::Rect::from_min_max(egui::pos2(50.0, 50.0), egui::pos2(400.0, 400.0));
        show_backdrop(&painter, rect, &Backdrop::default(), &view);
        let mut out = ctx.end_pass();
        out.textures_delta.clear();
        assert_eq!(out.shapes.len(), 1);
        let egui::Shape::Callback(cb) = &out.shapes[0].shape else {
            panic!("a paint callback, got {:?}", out.shapes[0].shape);
        };
        assert_eq!(
            cb.rect,
            egui::Rect::from_min_max(egui::pos2(50.0, 50.0), egui::pos2(200.0, 200.0)),
            "clipped to what is visible"
        );
        assert_eq!(
            take_backdrop(&ctx),
            Some(Backdrop::default().uniform(
                &view,
                egui::Rect::from_min_max(egui::pos2(50.0, 50.0), egui::pos2(200.0, 200.0))
            ))
        );
        assert_eq!(take_backdrop(&ctx), None, "taken once");
    }

    /// The pattern WGSL compiles inside the present shader, and the struct sizes agree.
    #[test]
    fn wgsl_struct_matches_the_rust_layout() {
        let src = format!(
            "{BACKDROP_WGSL}
@group(0) @binding(0) var<uniform> bd: BackdropUniform;
@fragment fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {{
    return vec4<f32>(backdrop_color(bd, p.xy), 1.0);
}}"
        );
        let module = naga::front::wgsl::parse_str(&src).expect("parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("validates");
        let ty = module
            .types
            .iter()
            .find(|(_, t)| t.name.as_deref() == Some("BackdropUniform"))
            .expect("the uniform struct");
        assert_eq!(
            ty.1.inner.size(module.to_ctx()),
            std::mem::size_of::<BackdropUniform>() as u32
        );
    }
}
