use glam::Mat4;

/// Screen-space label (glyphon path).
#[derive(Debug, Clone)]
pub struct ScreenLabel {
    pub text: String,
    pub screen_left: f32,
    pub screen_top: f32,
    pub font_size: f32,
    pub line_height: f32,
    pub color: [f32; 4],
    pub clip: [i32; 4],
    pub layout_width: Option<f32>,
}

/// Text drawn on a widget/graph surface in local pixels, transformed by `surface_xfm`.
#[derive(Debug, Clone)]
pub struct PlaneLabel {
    pub surface_xfm: Mat4,
    /// Layout box in surface-local pixels (origin top-left, +y down).
    pub local_rect: [f32; 4],
    pub text: String,
    pub font_size: f32,
    pub line_height: f32,
    pub color: [f32; 4],
    /// Glyph bake resolution multiplier (1 = layout size; >1 when zoomed in on the plane).
    pub raster_scale: f32,
    /// Surface-local clip rect `[min_x, min_y, max_x, max_y]`. Fragments outside are
    /// discarded by the plane shader. Use [`PlaneLabel::NO_CLIP`] for unclipped text.
    pub clip_rect: [f32; 4],
    /// May the text WRAP inside `local_rect`, or is it one line?
    ///
    /// A plane label used to wrap unconditionally, because the box width was handed straight to
    /// cosmic-text. For a one-line strip - a node title, a port name - that is never right: the box
    /// is a single line tall, so the second line is drawn outside it, and a narrow strip breaks
    /// mid-WORD ("Com" / "p") because a word that does not fit is split per glyph.
    ///
    /// `false` lays the text out unbounded and lets `clip_rect` cut it, which is what a strip with
    /// a computed width wanted all along. The screen path has always had this choice
    /// (`ScreenLabel::layout_width: Option`); the plane path did not.
    pub wrap: bool,
}

impl PlaneLabel {
    /// Effectively-infinite clip (no visual culling).
    pub const NO_CLIP: [f32; 4] = [-1.0e6, -1.0e6, 1.0e6, 1.0e6];

    pub fn from_local_rect(xfm: Mat4, x: f32, y: f32, w: f32, h: f32, text: String) -> Self {
        Self {
            surface_xfm: xfm,
            local_rect: [x, y, w, h],
            text,
            font_size: 12.0,
            line_height: 14.0,
            color: [0.92, 0.93, 0.95, 1.0],
            raster_scale: 1.0,
            wrap: true,
            clip_rect: Self::NO_CLIP,
        }
    }
}

/// Quantize screen pixels-per-graph-unit into a small set of atlas bake scales.
pub fn quantize_plane_raster_scale(pixels_per_graph_unit: f32) -> f32 {
    let raw = pixels_per_graph_unit.clamp(1.0, 6.0);
    (raw * 2.0).round() / 2.0
}
