//! A tiny, fixed-size, custom-painted progress bar for egui.
//!
//! Unlike egui's built-in `ProgressBar` (which flexes to the available width),
//! this one reserves an exact `width × height` rect — handy inside a status row
//! or a toolbar where space is budgeted. It paints a dark track, a coloured
//! fill, and a centred monospace `current/total` overlay. No app types.

/// A fixed-size progress bar. Build it once with the dimensions/colour, then
/// update the counts each frame with [`ProgressBar::set_progress`] before
/// [`ProgressBar::render`].
pub struct ProgressBar {
    current: usize,
    total: usize,
    width: f32,
    height: f32,
    fill_color: egui::Color32,
}

impl ProgressBar {
    /// New bar with the default fill (light grey).
    pub fn new(width: f32, height: f32) -> Self {
        Self::with_color(width, height, egui::Color32::from_rgb(178, 178, 178))
    }

    /// New bar with a custom fill colour.
    pub fn with_color(width: f32, height: f32, fill_color: egui::Color32) -> Self {
        Self {
            current: 0,
            total: 0,
            width,
            height,
            fill_color,
        }
    }

    /// Update the progress counts. `total == 0` renders an empty `0/0` bar.
    pub fn set_progress(&mut self, current: usize, total: usize) {
        self.current = current;
        self.total = total;
    }

    /// Reserve the bar's rect and paint track + fill + count overlay.
    pub fn render(&self, ui: &mut egui::Ui) {
        let progress = if self.total > 0 {
            self.current as f32 / self.total as f32
        } else {
            0.0
        };

        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(self.width, self.height), egui::Sense::hover());

        // Track.
        ui.painter()
            .rect_filled(rect, 2.0, egui::Color32::from_gray(40));

        // Fill.
        if progress > 0.0 {
            let fill_w = rect.width() * progress.clamp(0.0, 1.0);
            let fill_rect = egui::Rect::from_min_size(rect.min, egui::vec2(fill_w, rect.height()));
            ui.painter().rect_filled(fill_rect, 2.0, self.fill_color);
        }

        // Count overlay.
        let font = egui_widgets_config::mono(egui_widgets_config::WidgetsConfig::get(ui).fonts.xs);
        let text = format!("{}/{}", self.current, self.total);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            font,
            egui::Color32::from_gray(220),
        );
    }
}
