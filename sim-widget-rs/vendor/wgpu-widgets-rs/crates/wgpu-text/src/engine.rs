//! Combined screen (glyphon) + plane (atlas quads) text engine.

use glam::Mat4;
use glyphon::{
    Attrs, Buffer as TextBuffer, Family, FontSystem, Metrics, Shaping, SwashCache,
};
use wgpu::{Device, Queue, RenderPass, TextureFormat};

use crate::atlas::{GlyphAtlas, TextFilterQuality};
use crate::labels::{PlaneLabel, ScreenLabel};
use crate::plane::PlaneTextRenderer;
use crate::screen::ScreenTextRenderer;

pub struct TextEngine {
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    atlas: GlyphAtlas,
    screen: ScreenTextRenderer,
    plane: PlaneTextRenderer,
    plane_buffers: Vec<TextBuffer>,
}

impl TextEngine {
    /// Create the screen + plane text engine (default quality: [`TextFilterQuality::Linear`]).
    pub fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
        Self::with_quality(device, queue, format, TextFilterQuality::Linear)
    }

    /// Create the engine with an explicit glyph-atlas [`TextFilterQuality`] — pick
    /// `Anisotropic` for a camera that regularly views the text plane at an angle
    /// (orbit/tilt); `Linear` (the `new()` default) is enough for a top-down view.
    pub fn with_quality(
        device: &Device,
        queue: &Queue,
        format: TextureFormat,
        quality: TextFilterQuality,
    ) -> Self {
        let font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let atlas = GlyphAtlas::with_quality(device, queue, quality);
        let atlas_layout = atlas.bind_group_layout().clone();
        let screen = ScreenTextRenderer::new(device, queue, format);
        let plane = PlaneTextRenderer::new(device, format, &atlas_layout);

        Self {
            font_system,
            swash_cache,
            atlas,
            screen,
            plane,
            plane_buffers: Vec::new(),
        }
    }

    pub fn prepare_screen(
        &mut self,
        device: &Device,
        queue: &Queue,
        labels: &[ScreenLabel],
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        self.screen.prepare(
            device,
            queue,
            &mut self.font_system,
            &mut self.swash_cache,
            labels,
            width,
            height,
        )
    }

    pub fn render_screen<'pass>(&self, pass: &mut RenderPass<'pass>) -> Result<(), String> {
        self.screen.render(pass)
    }

    pub fn prepare_plane(
        &mut self,
        device: &Device,
        queue: &Queue,
        labels: &[PlaneLabel],
    ) -> Result<(), String> {
        self.plane.prepare(
            device,
            queue,
            &mut self.font_system,
            &mut self.swash_cache,
            &mut self.atlas,
            labels,
            &mut self.plane_buffers,
        )?;
        self.plane.upload(device, queue);
        // Batched once per frame (not per glyph) — no-op unless `quality` is
        // `Anisotropic` and a glyph was actually (re)baked above.
        self.atlas.flush_mips(device, queue);
        Ok(())
    }

    pub fn update_plane_view_proj(&self, queue: &Queue, view_proj: Mat4) {
        self.plane.update_view_proj(queue, view_proj);
    }

    pub fn render_plane<'pass>(&'pass self, pass: &mut RenderPass<'pass>) {
        let n = self.plane.instance_count();
        self.plane.draw(pass, self.atlas.bind_group(), n);
    }

    pub fn plane_instance_count(&self) -> u32 {
        self.plane.instance_count()
    }

    /// Color/mask atlas side lengths in pixels (grows on demand; useful for diagnostics).
    pub fn atlas_dims(&self) -> (u32, u32) {
        self.atlas.dims()
    }

    /// Per-character caret x-offsets (px) for `text`, shaped with the SAME font + shaper the
    /// plane-text renderer uses, so a text widget can place a caret, hit-test a click, or draw a
    /// selection box against exactly the glyph positions it will see on screen.
    ///
    /// See [`measure_caret_offsets`] for the returned vector's contract. Measurement needs only
    /// the shaper (no GPU), so it reuses this engine's [`FontSystem`] without touching the atlas.
    pub fn measure_offsets(&mut self, text: &str, font_size: f32, line_height: f32) -> Vec<f32> {
        measure_caret_offsets(&mut self.font_system, text, font_size, line_height)
    }
}

/// Cumulative caret x-offsets (px) at every character boundary of `text`, laid out single-line
/// and unwrapped with `font_size`/`line_height` on `font_system`.
///
/// Returns `text.chars().count() + 1` non-decreasing values: element `i` is the pen x at the left
/// edge of the `i`-th char (element `0` is `0.0`), and the final element is the full advance
/// width. A caret "before char `i`" sits at `offsets[i]`; the click-to-caret index for a local x
/// is the nearest boundary. Complex clusters (ligatures) that a single glyph spans map every
/// covered char to the cluster's start x — accurate enough for Latin UI field editing.
///
/// Pure CPU (shaping only), so callers can measure off the render thread and without a device.
pub fn measure_caret_offsets(
    font_system: &mut FontSystem,
    text: &str,
    font_size: f32,
    line_height: f32,
) -> Vec<f32> {
    let char_count = text.chars().count();
    let mut offsets = vec![0.0f32; char_count + 1];
    if char_count == 0 {
        return offsets;
    }
    // Map each glyph's byte `start` back to a char index.
    let mut byte_to_char = std::collections::HashMap::with_capacity(char_count);
    for (ci, (bi, _)) in text.char_indices().enumerate() {
        byte_to_char.insert(bi, ci);
    }
    let mut buffer = TextBuffer::new(
        font_system,
        Metrics::new(font_size.max(1.0), line_height.max(1.0)),
    );
    // No wrap: a single unbounded line (fields are single-line).
    buffer.set_size(None, None);
    let attrs = Attrs::new().family(Family::SansSerif);
    buffer.set_text(text, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(font_system, false);

    let mut total = 0.0f32;
    for run in buffer.layout_runs() {
        for glyph in run.glyphs.iter() {
            total = total.max(glyph.x + glyph.w);
            if let Some(&ci) = byte_to_char.get(&glyph.start)
                && ci < char_count
            {
                offsets[ci] = glyph.x;
            }
        }
    }
    offsets[char_count] = total;
    // A char boundary no glyph reported (ligature/cluster interior) keeps the previous boundary;
    // also guarantees the sequence is monotonic non-decreasing for correct nearest-boundary math.
    for i in 1..offsets.len() {
        if offsets[i] < offsets[i - 1] {
            offsets[i] = offsets[i - 1];
        }
    }
    offsets
}

/// Nearest character boundary index for a caret at local pen `x` (px), given the ascending
/// [`measure_caret_offsets`] table. Clamps to `0..=offsets.len()-1` (i.e. `0..=char_count`).
pub fn caret_index_for_x(offsets: &[f32], x: f32) -> usize {
    if offsets.len() <= 1 {
        return 0;
    }
    let mut best = 0usize;
    let mut best_d = f32::MAX;
    for (i, &off) in offsets.iter().enumerate() {
        let d = (off - x).abs();
        if d < best_d {
            best_d = d;
            best = i;
        }
    }
    best
}

#[cfg(test)]
mod measure_tests {
    use super::{caret_index_for_x, measure_caret_offsets};
    use glyphon::FontSystem;

    #[test]
    fn offsets_are_monotonic_and_sized() {
        // Measurement is pure shaping — no GPU needed.
        let mut fs = FontSystem::new();
        let offs = measure_caret_offsets(&mut fs, "hello", 14.0, 18.0);
        assert_eq!(offs.len(), 6, "chars + 1 boundaries");
        assert_eq!(offs[0], 0.0, "caret before first char is at 0");
        for w in offs.windows(2) {
            assert!(w[1] >= w[0], "offsets must be non-decreasing: {offs:?}");
        }
        assert!(*offs.last().unwrap() > 0.0, "non-empty text has positive width");
    }

    #[test]
    fn empty_text_has_single_zero_boundary() {
        let mut fs = FontSystem::new();
        let offs = measure_caret_offsets(&mut fs, "", 14.0, 18.0);
        assert_eq!(offs, vec![0.0]);
    }

    #[test]
    fn caret_index_snaps_to_nearest_boundary() {
        // Synthetic evenly-spaced boundaries → exercise the nearest-boundary search directly.
        let offs = [0.0, 10.0, 20.0, 30.0];
        assert_eq!(caret_index_for_x(&offs, -5.0), 0);
        assert_eq!(caret_index_for_x(&offs, 4.0), 0);
        assert_eq!(caret_index_for_x(&offs, 6.0), 1);
        assert_eq!(caret_index_for_x(&offs, 100.0), 3);
    }
}
