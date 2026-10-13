//! Screen-space labels via glyphon.

use glyphon::{
    Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache,
    TextArea, TextAtlas, TextBounds, TextRenderer as GlyphonRenderer, Viewport,
};

// `Cache` is consumed only by `Viewport::new` and `TextAtlas::new`; glyphon clones the
// Arc internally, so we drop our handle after construction.
use wgpu::{Device, MultisampleState, Queue, RenderPass, TextureFormat};

use crate::color::rgba_to_glyphon;
use crate::labels::ScreenLabel;

pub struct ScreenTextRenderer {
    viewport: Viewport,
    atlas: TextAtlas,
    renderer: GlyphonRenderer,
    buffers: Vec<Buffer>,
}

impl ScreenTextRenderer {
    pub fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
        let cache = Cache::new(device);
        let viewport = Viewport::new(device, &cache);
        let mut atlas = TextAtlas::new(device, queue, &cache, format);
        let renderer = GlyphonRenderer::new(&mut atlas, device, MultisampleState::default(), None);
        Self {
            viewport,
            atlas,
            renderer,
            buffers: Vec::new(),
        }
    }

    pub fn prepare(
        &mut self,
        device: &Device,
        queue: &Queue,
        font_system: &mut FontSystem,
        swash_cache: &mut SwashCache,
        labels: &[ScreenLabel],
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        if labels.is_empty() {
            return Ok(());
        }

        self.viewport.update(
            queue,
            Resolution { width, height },
        );

        while self.buffers.len() < labels.len() {
            self.buffers
                .push(Buffer::new(font_system, Metrics::new(13.0, 16.0)));
        }

        let mut areas = Vec::with_capacity(labels.len());
        for (label, buffer) in labels.iter().zip(self.buffers.iter_mut()) {
            let metrics = Metrics::new(label.font_size, label.line_height);
            buffer.set_metrics(metrics);
            if let Some(w) = label.layout_width {
                buffer.set_size(Some(w), None);
            } else {
                buffer.set_size(None, None);
            }

            let attrs = Attrs::new().family(Family::SansSerif);
            buffer.set_text(&label.text,
                &attrs,
                Shaping::Advanced,
                None,
            );
            buffer.shape_until_scroll(font_system, false);

            let color: Color = rgba_to_glyphon(label.color);
            let bounds = TextBounds {
                left: label.clip[0],
                top: label.clip[1],
                right: label.clip[2],
                bottom: label.clip[3],
            };

            areas.push(TextArea {
                buffer,
                left: label.screen_left,
                top: label.screen_top,
                scale: 1.0,
                bounds,
                default_color: color,
                custom_glyphs: &[],
            });
        }

        self.renderer
            .prepare(
                device,
                queue,
                font_system,
                &mut self.atlas,
                &self.viewport,
                areas,
                swash_cache,
            )
            .map_err(|e| format!("glyphon prepare: {e}"))?;

        self.atlas.trim();
        Ok(())
    }

    pub fn render<'pass>(&self, pass: &mut RenderPass<'pass>) -> Result<(), String> {
        self.renderer
            .render(&self.atlas, &self.viewport, pass)
            .map_err(|e| format!("glyphon render: {e}"))
    }
}
