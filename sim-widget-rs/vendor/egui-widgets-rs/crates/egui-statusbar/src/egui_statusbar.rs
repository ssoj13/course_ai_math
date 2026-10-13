//! A reusable sectioned, resizable status bar for egui.
//!
//! Lay out `N` sections separated by interactive vertical splitters. Sections
//! `0..N-1` have a stored, drag-resizable width (double-click a splitter to reset
//! it); the last section flexes to fill the remainder. Each section's content is a
//! caller closure drawn into a clipped child [`egui::Ui`], so overflow truncates
//! at the section boundary instead of spilling into its neighbour.
//!
//! Widths persist via [`StatusBarLayout`] (serde) — store it in your app config and
//! pass it back each frame; [`StatusBar::show`] returns `true` when the user
//! resized so you know to re-save.

use egui::{Sense, Ui};
use serde::{Deserialize, Serialize};

/// Persisted per-section widths. Holds one width per *resizable* section
/// (`sections - 1`); the last section flexes and is not stored.
///
/// [`Default`] is an empty layout: the first [`StatusBar::show`] detects the
/// length mismatch and fills it with the sections' own default widths, so callers
/// can persist `Default::default()` without knowing the section count up front.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StatusBarLayout {
    /// Widths (points) for the resizable sections, left to right.
    pub widths: Vec<f32>,
}

impl StatusBarLayout {
    /// A layout with the given default widths (one per resizable section).
    pub fn new(widths: Vec<f32>) -> Self {
        Self { widths }
    }

    /// Reset `widths` to `defaults` when its length no longer matches the section
    /// count (e.g. a config from a build with a different number of sections).
    /// Returns `true` if it reset.
    fn ensure_len(&mut self, defaults: &[f32]) -> bool {
        if self.widths.len() != defaults.len() {
            self.widths = defaults.to_vec();
            true
        } else {
            false
        }
    }
}

/// One status-bar section: a start width and a closure drawing its content.
/// The LAST section passed to [`StatusBar::show`] flexes — its `default_width` is
/// ignored.
pub struct Section<'a> {
    default_width: f32,
    draw: Box<dyn FnMut(&mut Ui) + 'a>,
}

impl<'a> Section<'a> {
    /// A section with a default/starting width and a content closure.
    pub fn new(default_width: f32, draw: impl FnMut(&mut Ui) + 'a) -> Self {
        Self {
            default_width,
            draw: Box::new(draw),
        }
    }
}

/// The status-bar widget (layout constants). Cheap to construct each frame.
#[derive(Clone, Copy, Debug)]
pub struct StatusBar {
    /// Minimum width a resizable section may be dragged to.
    pub min_width: f32,
    /// Width of the interactive splitter between sections.
    pub splitter_width: f32,
    /// Frozen widths: the dividers are drawn but have no hit zone, so there is no resize
    /// cursor, no drag and no double-click reset (see [`Self::locked`]).
    pub locked: bool,
}

impl Default for StatusBar {
    fn default() -> Self {
        Self {
            min_width: 60.0,
            splitter_width: 4.0,
            locked: false,
        }
    }
}

impl StatusBar {
    pub fn new() -> Self {
        Self::default()
    }

    /// Freeze the section widths (a host's "lock" toggle). The layout is still read, never
    /// written; the dividers keep their idle look. A host must not fake this by drawing on a
    /// copy of the layout: the live splitters would still show the resize cursor and spring
    /// back after every drag.
    pub fn locked(mut self, on: bool) -> Self {
        self.locked = on;
        self
    }

    /// Draw the bar. `sections` must be non-empty; the last one flexes.
    /// Returns `true` when a splitter drag / reset changed `layout.widths`.
    pub fn show(
        &self,
        ui: &mut Ui,
        layout: &mut StatusBarLayout,
        sections: &mut [Section<'_>],
    ) -> bool {
        let defaults: Vec<f32> = sections.iter().map(|s| s.default_width).collect();
        self.show_with(ui, layout, &defaults, |i, child| (sections[i].draw)(child))
    }

    /// Draw sections with a borrowed callback and reusable default widths.
    ///
    /// One width identifies each section; the final section flexes. Unlike
    /// boxed `Section` construction, this path allocates only when the stored
    /// section count changes, never for callbacks or defaults on idle repaint.
    pub fn show_with(
        &self,
        ui: &mut Ui,
        layout: &mut StatusBarLayout,
        defaults: &[f32],
        mut draw: impl FnMut(usize, &mut Ui),
    ) -> bool {
        if defaults.is_empty() {
            return false;
        }
        let n = defaults.len();
        let n_fixed = n - 1;
        let mut changed = layout.ensure_len(&defaults[..n_fixed]);
        let metrics = egui_widgets_config::WidgetsConfig::get(ui).metrics;

        // Reserve a fixed-height row in the parent so the auto-sized bottom panel
        // keeps a stable height (child UIs / painter calls don't advance the cursor).
        let row_h = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
        let (full, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), Sense::hover());
        let height = full.height();
        let total_w = full.width();

        let splitter_sum = self.splitter_width * n_fixed as f32;
        let fixed_sum: f32 = layout.widths.iter().sum();
        let flex_w = (total_w - fixed_sum - splitter_sum).max(self.min_width);

        let mut x = full.left();
        for (i, &default_width) in defaults.iter().enumerate() {
            let w = if i < n_fixed {
                layout.widths[i]
            } else {
                flex_w
            };
            let rect = egui::Rect::from_min_size(egui::pos2(x, full.top()), egui::vec2(w, height));

            // Draw the section content into a clipped child UI.
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            child.set_clip_rect(rect.intersect(ui.clip_rect()));
            child.add_space(4.0);
            draw(i, &mut child);
            x += w;

            // Splitter after every section except the last. `cx` matches the
            // old paint centre (`split.center().x`); hit_half_width =
            // splitter_width/2 reproduces the old `[x, x+splitter_width]` hit
            // rect exactly (no hit-zone change).
            if i < n_fixed && self.locked {
                egui_widgets_config::splitter::paint(ui, full, x + self.splitter_width / 2.0, 2.0, &metrics, false);
                x += self.splitter_width;
            } else if i < n_fixed {
                let cx = x + self.splitter_width / 2.0;
                let id = ui.id().with(("egui_statusbar_splitter", i));
                // This section can only grow by the slack the flex tail can give up.
                let max_w = layout.widths[i] + (flex_w - self.min_width).max(0.0);
                let hit = egui_widgets_config::splitter::interact(
                    ui,
                    full,
                    cx,
                    id,
                    self.splitter_width / 2.0,
                    2.0,
                    &metrics,
                );
                if let Some(dx) = hit.delta {
                    layout.widths[i] = clamp_width(layout.widths[i] + dx, self.min_width, max_w);
                    changed = true;
                }
                if hit.double_clicked {
                    layout.widths[i] = default_width;
                    changed = true;
                }
                x += self.splitter_width;
            }
        }
        changed
    }
}

/// Clamp a dragged section width to `[min, max]` (with `max >= min`).
fn clamp_width(w: f32, min: f32, max: f32) -> f32 {
    w.clamp(min, max.max(min))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borrowed_and_legacy_paths_share_geometry_and_clip() {
        let defaults = [80.0, 120.0, 0.0];
        let initial = StatusBarLayout::new(vec![95.0, 120.0]);
        let draw = || {
            let ctx = egui::Context::default();
            let mut layout = initial.clone();
            let mut rects = Vec::new();
            let mut clips = Vec::new();
            let mut output = ctx.run_ui(Default::default(), |ui| {
                ui.set_max_width(360.0);
                ui.set_clip_rect(egui::Rect::from_min_size(
                    ui.max_rect().min,
                    egui::vec2(250.0, 80.0),
                ));
                let parent_clip = ui.clip_rect();
                let mut capture = |_: usize, child: &mut Ui| {
                    rects.push(child.max_rect());
                    clips.push(child.clip_rect());
                    assert_eq!(child.clip_rect(), child.max_rect().intersect(parent_clip));
                };
                assert!(!StatusBar::new().show_with(ui, &mut layout, &defaults, &mut capture));
            });
            output.textures_delta.clear();
            (layout, rects, clips)
        };
        let (_, expected, expected_clips) = draw();
        let ctx = egui::Context::default();
        let mut layout = initial.clone();
        let mut rects = [egui::Rect::NOTHING; 3];
        let mut clips = [egui::Rect::NOTHING; 3];
        let mut output = ctx.run_ui(Default::default(), |ui| {
            ui.set_max_width(360.0);
            ui.set_clip_rect(egui::Rect::from_min_size(
                ui.max_rect().min,
                egui::vec2(250.0, 80.0),
            ));
            let (r0, rest) = rects.split_at_mut(1);
            let (r1, r2) = rest.split_at_mut(1);
            let (c0, rest) = clips.split_at_mut(1);
            let (c1, c2) = rest.split_at_mut(1);
            let mut sections = [
                Section::new(defaults[0], |ui| {
                    r0[0] = ui.max_rect();
                    c0[0] = ui.clip_rect();
                }),
                Section::new(defaults[1], |ui| {
                    r1[0] = ui.max_rect();
                    c1[0] = ui.clip_rect();
                }),
                Section::new(defaults[2], |ui| {
                    r2[0] = ui.max_rect();
                    c2[0] = ui.clip_rect();
                }),
            ];
            assert!(!StatusBar::new().show(ui, &mut layout, &mut sections));
        });
        output.textures_delta.clear();
        assert_eq!(rects.as_slice(), expected.as_slice());
        assert_eq!(clips.as_slice(), expected_clips.as_slice());
        assert_eq!(layout.widths, vec![95.0, 120.0]);
    }

    #[test]
    fn borrowed_splitter_drag_preserves_updated_width_on_next_frame() {
        let ctx = egui::Context::default();
        let mut layout = StatusBarLayout::default();
        let defaults = [100.0, 0.0];
        let mut split_pos = egui::Pos2::ZERO;
        let mut output = ctx.run_ui(Default::default(), |ui| {
            ui.set_max_width(400.0);
            StatusBar::new().show_with(ui, &mut layout, &defaults, |i, child| {
                if i == 0 {
                    split_pos =
                        egui::pos2(child.max_rect().right() + 2.0, child.max_rect().center().y);
                }
            });
        });
        output.textures_delta.clear();
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![
                    egui::Event::PointerMoved(split_pos),
                    egui::Event::PointerButton {
                        pos: split_pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                ],
                ..Default::default()
            },
            |ui| {
                ui.set_max_width(400.0);
                StatusBar::new().show_with(ui, &mut layout, &defaults, |_, _| {});
            },
        );
        output.textures_delta.clear();
        let moved = split_pos + egui::vec2(30.0, 0.0);
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::PointerMoved(moved)],
                ..Default::default()
            },
            |ui| {
                ui.set_max_width(400.0);
                assert!(StatusBar::new().show_with(ui, &mut layout, &defaults, |_, _| {}));
            },
        );
        output.textures_delta.clear();
        let dragged_width = layout.widths[0];
        assert!(dragged_width > defaults[0]);
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::PointerButton {
                    pos: moved,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                }],
                ..Default::default()
            },
            |ui| {
                ui.set_max_width(400.0);
                StatusBar::new().show_with(ui, &mut layout, &defaults, |_, _| {});
            },
        );
        output.textures_delta.clear();
        assert_eq!(layout.widths[0], dragged_width);
    }

    /// A locked bar ignores the same drag the test above performs: no width change, no
    /// `changed`, and no resize cursor over the divider (which a layout-copy fake still showed).
    #[test]
    fn a_locked_bar_neither_resizes_nor_shows_the_resize_cursor() {
        let ctx = egui::Context::default();
        let mut layout = StatusBarLayout::new(vec![100.0]);
        let defaults = [100.0, 0.0];
        let bar = StatusBar::new().locked(true);
        // One frame; returns `show_with`'s result, the cursor egui will set and the divider's
        // position.
        let frame = |events: Vec<egui::Event>, layout: &mut StatusBarLayout| {
            let mut changed = false;
            let mut split = egui::Pos2::ZERO;
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                ui.set_max_width(400.0);
                changed = bar.show_with(ui, layout, &defaults, |i, child| {
                    if i == 0 {
                        split = egui::pos2(child.max_rect().right() + 2.0, child.max_rect().center().y);
                    }
                });
            });
            let cursor = output.platform_output.cursor_icon;
            output.textures_delta.clear();
            (changed, cursor, split)
        };
        let at = frame(Vec::new(), &mut layout).2;
        let press = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let (_, cursor, _) = frame(vec![egui::Event::PointerMoved(at), press(at, true)], &mut layout);
        assert_ne!(cursor, egui::CursorIcon::ResizeHorizontal, "a locked divider offers a resize");
        let moved = at + egui::vec2(30.0, 0.0);
        let (changed, _, _) = frame(vec![egui::Event::PointerMoved(moved)], &mut layout);
        frame(vec![press(moved, false)], &mut layout);
        assert!(!changed);
        assert_eq!(layout.widths, vec![100.0], "a locked bar was resized");
    }

    #[test]
    fn ensure_len_resets_on_mismatch() {
        let mut l = StatusBarLayout::new(vec![100.0, 200.0]);
        assert!(!l.ensure_len(&[10.0, 20.0])); // same length → no reset
        assert_eq!(l.widths, vec![100.0, 200.0]);
        assert!(l.ensure_len(&[10.0, 20.0, 30.0])); // length changed → reset
        assert_eq!(l.widths, vec![10.0, 20.0, 30.0]);
    }

    #[test]
    fn clamp_keeps_within_bounds() {
        assert_eq!(clamp_width(10.0, 60.0, 300.0), 60.0); // below min
        assert_eq!(clamp_width(500.0, 60.0, 300.0), 300.0); // above max
        assert_eq!(clamp_width(120.0, 60.0, 300.0), 120.0);
        // Degenerate max < min: never below min.
        assert_eq!(clamp_width(10.0, 60.0, 20.0), 60.0);
    }
}
