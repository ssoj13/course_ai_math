//! A generic multi-column table with draggable column splitters for egui.
//!
//! Build a few [`Column`]s (header + default width), then [`SplitterTable::show`]
//! them against a row count and a per-cell draw closure. Columns `0..N-1` have
//! stored, drag-resizable widths (double-click a splitter to reset); the last
//! column flexes to fill the remaining space. Widths persist via
//! [`SplitterTableState`] (serde) — keep it in your app config.
//!
//! The widget owns no data: it draws the chrome (header, stripes, splitters) and
//! calls back `cell(ui, row, col)` for every body cell into a clipped child
//! `Ui`, so content truncates at the column boundary instead of spilling.
//!
//! ```no_run
//! # use egui_splitter_table::{Column, SplitterTable, SplitterTableState};
//! # fn demo(ui: &mut egui::Ui, state: &mut SplitterTableState, rows: &[(String, String, String)]) {
//! let table = SplitterTable::new(vec![
//!     Column::new("Name", 160.0),
//!     Column::new("Type", 90.0),
//!     Column::new("Value", 0.0), // last column flexes
//! ]);
//! table.show(ui, state, rows.len(), |ui, r, c| {
//!     let row = &rows[r];
//!     ui.label(match c { 0 => &row.0, 1 => &row.1, _ => &row.2 });
//! });
//! # }
//! ```

use egui::{Sense, Ui};
use serde::{Deserialize, Serialize};

/// Persisted widths for the resizable columns — one per column except the last,
/// which flexes. [`Default`] is empty: the first [`SplitterTable::show`] detects
/// the length mismatch and fills it from the columns' own defaults, so callers
/// can persist `Default::default()` without knowing the column count up front.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SplitterTableState {
    /// Widths (points) for the resizable columns, left to right.
    pub widths: Vec<f32>,
}

impl SplitterTableState {
    /// A state pre-seeded with the given resizable-column widths.
    pub fn new(widths: Vec<f32>) -> Self {
        Self { widths }
    }

    /// Reset `widths` to `defaults` when the length no longer matches the column
    /// count (e.g. a config from a build with a different number of columns).
    /// Returns `true` if it reset.
    fn ensure_len(&mut self, defaults: impl ExactSizeIterator<Item = f32>) -> bool {
        if self.widths.len() != defaults.len() {
            self.widths.clear();
            self.widths.extend(defaults);
            true
        } else {
            false
        }
    }
}

/// One column: a header label and a default/starting width. The LAST column
/// passed to [`SplitterTable::new`] flexes — its `default_width` is ignored.
pub struct Column {
    pub header: String,
    pub default_width: f32,
}

impl Column {
    pub fn new(header: impl Into<String>, default_width: f32) -> Self {
        Self {
            header: header.into(),
            default_width,
        }
    }
}

/// The splitter-table widget (columns + layout knobs). Cheap to build per frame.
pub struct SplitterTable {
    columns: Vec<Column>,
    min_width: f32,
    splitter_width: f32,
    row_height: f32,
    striped: bool,
    show_header: bool,
    max_body_height: Option<f32>,
}

impl SplitterTable {
    /// Default gap between columns, shared with hosts projecting table rows.
    pub const DEFAULT_SPLITTER_WIDTH: f32 = 4.0;
    pub const DEFAULT_MIN_WIDTH: f32 = 40.0;
    /// Insets applied to every table cell before invoking its editor.
    pub const CELL_PADDING: egui::Vec2 = egui::vec2(4.0, 1.0);
    /// New table for the given columns (left-to-right; the last one flexes).
    pub fn new(columns: Vec<Column>) -> Self {
        Self {
            columns,
            min_width: Self::DEFAULT_MIN_WIDTH,
            splitter_width: Self::DEFAULT_SPLITTER_WIDTH,
            // `Metrics::default()` needs no `ui`/`Context`, so this can reference
            // the token directly instead of a bare literal that could silently
            // drift from it; host still overrides via `.row_height(..)`.
            row_height: egui_widgets_config::Metrics::default().row_h,
            striped: true,
            show_header: true,
            max_body_height: None,
        }
    }

    /// Minimum width a resizable column may be dragged to (default 40).
    pub fn min_width(mut self, w: f32) -> Self {
        self.min_width = w;
        self
    }

    /// Per-row pixel height (default 18).
    pub fn row_height(mut self, h: f32) -> Self {
        self.row_height = h;
        self
    }

    /// Whether odd rows get a faint background stripe (default true).
    pub fn striped(mut self, v: bool) -> Self {
        self.striped = v;
        self
    }

    /// Whether to draw the header row (default true).
    pub fn show_header(mut self, v: bool) -> Self {
        self.show_header = v;
        self
    }

    /// Bound the table body to this height and render only viewport-visible rows.
    ///
    /// The header remains fixed. The body owns a vertical scroll area; cell callbacks are invoked
    /// only for the visible row range. Without this option [`Self::show`] keeps its original
    /// unbounded layout behavior.
    pub fn max_body_height(mut self, height: f32) -> Self {
        self.max_body_height = Some(height.max(self.row_height));
        self
    }

    /// Draw the table. `cell(ui, row, col)` renders each body cell into a
    /// clipped per-column child `Ui`. Returns `true` when a splitter drag/reset
    /// changed `state.widths` (re-save your config).
    pub fn show(
        mut self,
        ui: &mut Ui,
        state: &mut SplitterTableState,
        rows: usize,
        cell: impl FnMut(&mut Ui, usize, usize),
    ) -> bool {
        if let Some(max_body_height) = self.max_body_height {
            return self.show_bounded(ui, state, rows, max_body_height, cell);
        }
        let columns = std::mem::take(&mut self.columns);
        self.show_with_columns(ui, state, rows, &columns, cell)
    }

    /// Render this same widget using borrowed columns, avoiding per-frame column construction.
    pub fn show_with_columns(
        self,
        ui: &mut Ui,
        state: &mut SplitterTableState,
        rows: usize,
        columns: &[Column],
        mut cell: impl FnMut(&mut Ui, usize, usize),
    ) -> bool {
        if let Some(max_body_height) = self.max_body_height {
            return self.show_bounded_with_columns(ui, state, rows, max_body_height, columns, cell);
        }
        let n = columns.len();
        if n == 0 {
            return false;
        }
        let n_fixed = n - 1; // last column flexes
        let mut changed = state.ensure_len(columns[..n_fixed].iter().map(|c| c.default_width));
        let metrics = egui_widgets_config::WidgetsConfig::get(ui).metrics;

        let header_rows = usize::from(self.show_header);
        let total_h = self.row_height * (header_rows + rows) as f32;
        let (full, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), total_h.max(self.row_height)),
            Sense::hover(),
        );
        let total_w = full.width();

        let splitter_sum = self.splitter_width * n_fixed as f32;
        let fixed_sum: f32 = state.widths.iter().sum();
        let flex_w = (total_w - fixed_sum - splitter_sum).max(self.min_width);

        let painter = ui.painter().clone();
        let base = ui.visuals().panel_fill;
        painter.rect_filled(full, 0.0, base);

        // Striped body-row backgrounds.
        if self.striped {
            let stripe = ui.visuals().faint_bg_color;
            for r in (0..rows).filter(|r| r % 2 == 1) {
                let y = full.top() + (header_rows + r) as f32 * self.row_height;
                let rect = egui::Rect::from_min_size(
                    egui::pos2(full.left(), y),
                    egui::vec2(total_w, self.row_height),
                );
                painter.rect_filled(rect, 0.0, stripe);
            }
        }

        // Header + body cells, each in a clipped child Ui.
        let pad = egui::vec2(4.0, 1.0);
        for (ci, (cx, cw)) in
            column_positions(full.left(), &state.widths, flex_w, self.splitter_width).enumerate()
        {
            if self.show_header {
                let rect = egui::Rect::from_min_size(
                    egui::pos2(cx, full.top()),
                    egui::vec2(cw, self.row_height),
                );
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(rect.shrink2(pad))
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                child.set_clip_rect(rect.intersect(ui.clip_rect()));
                child.strong(&columns[ci].header);
            }
            for r in 0..rows {
                let y = full.top() + (header_rows + r) as f32 * self.row_height;
                let rect =
                    egui::Rect::from_min_size(egui::pos2(cx, y), egui::vec2(cw, self.row_height));
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(rect.shrink2(pad))
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                child.set_clip_rect(rect.intersect(ui.clip_rect()));
                cell(&mut child, r, ci);
            }
        }

        // Interactive splitters after each fixed column. `cx` is the same
        // centre the old inline code painted at (`split.center().x`); passing
        // `hit_half_width = splitter_width/2` reproduces the old hit rect
        // `[sx, sx+splitter_width]` exactly — no hit-zone change.
        let mut column_x = full.left();
        for (i, column) in columns[..n_fixed].iter().enumerate() {
            let original_width = state.widths[i];
            let sx = column_x + original_width; // right edge of column i
            let cx = sx + self.splitter_width / 2.0;
            let id = ui.id().with(("egui_splitter_table", i));
            let max_w = (state.widths[i] + (flex_w - self.min_width).max(0.0)).max(self.min_width);
            let hit = egui_widgets_config::splitter::interact(
                ui,
                full,
                cx,
                id,
                self.splitter_width / 2.0,
                1.0,
                &metrics,
            );
            if let Some(dx) = hit.delta {
                state.widths[i] = (state.widths[i] + dx).clamp(self.min_width, max_w);
                changed = true;
            }
            if hit.double_clicked {
                state.widths[i] = column.default_width;
                changed = true;
            }
            column_x += original_width + self.splitter_width;
        }

        changed
    }

    fn show_bounded(
        mut self,
        ui: &mut Ui,
        state: &mut SplitterTableState,
        rows: usize,
        max_body_height: f32,
        cell: impl FnMut(&mut Ui, usize, usize),
    ) -> bool {
        let columns = std::mem::take(&mut self.columns);
        self.show_bounded_with_columns(ui, state, rows, max_body_height, &columns, cell)
    }

    /// Render a vertically bounded table with borrowed columns.
    pub fn show_bounded_with_columns(
        self,
        ui: &mut Ui,
        state: &mut SplitterTableState,
        rows: usize,
        max_body_height: f32,
        columns: &[Column],
        mut cell: impl FnMut(&mut Ui, usize, usize),
    ) -> bool {
        let n = columns.len();
        if n == 0 {
            return false;
        }
        let n_fixed = n - 1;
        let mut changed = state.ensure_len(columns[..n_fixed].iter().map(|c| c.default_width));
        let metrics = egui_widgets_config::WidgetsConfig::get(ui).metrics;

        let body_content_height = self.row_height * rows as f32;
        // Claim only as much height as the rows need (capped by max_body_height).
        // Claiming the full max always — the previous behaviour — left a dead gap under
        // short attribute lists when a sibling panel (e.g. Effects) sat below the grid.
        let body_outer_height = body_content_height
            .max(self.row_height)
            .min(max_body_height);
        let needs_scroll = body_content_height > max_body_height;
        let scrollbar_width = if needs_scroll {
            ui.style().spacing.scroll.allocated_width()
        } else {
            0.0
        };
        let table_width = (ui.available_width() - scrollbar_width).max(self.min_width);
        let table_left = ui.available_rect_before_wrap().left();

        let splitter_sum = self.splitter_width * n_fixed as f32;
        let fixed_sum: f32 = state.widths.iter().sum();
        let flex_width = (table_width - fixed_sum - splitter_sum).max(self.min_width);

        let padding = Self::CELL_PADDING;
        let header_rect = if self.show_header {
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(table_width, self.row_height), Sense::hover());
            for (column_index, (column_x, column_width)) in
                column_positions(table_left, &state.widths, flex_width, self.splitter_width)
                    .enumerate()
            {
                let cell_rect = egui::Rect::from_min_size(
                    egui::pos2(column_x, rect.top()),
                    egui::vec2(column_width, self.row_height),
                );
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(cell_rect.shrink2(padding))
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                child.set_clip_rect(cell_rect.intersect(ui.clip_rect()));
                child.strong(&columns[column_index].header);
            }
            Some(rect)
        } else {
            None
        };

        let body = egui::ScrollArea::vertical()
            .max_height(body_outer_height)
            .auto_shrink([false, true])
            .show_viewport(ui, |body_ui, viewport| {
                body_ui.set_width(table_width);
                body_ui.set_height(body_content_height.max(self.row_height));
                let content_top = body_ui.max_rect().top();
                let visible =
                    visible_row_range(viewport.min.y, viewport.max.y, self.row_height, rows);

                let base = body_ui.visuals().panel_fill;
                body_ui.painter().rect_filled(body_ui.max_rect(), 0.0, base);

                if self.striped {
                    let stripe = body_ui.visuals().faint_bg_color;
                    for row in visible.clone().filter(|row| row % 2 == 1) {
                        let y = content_top + row as f32 * self.row_height;
                        let rect = egui::Rect::from_min_size(
                            egui::pos2(table_left, y),
                            egui::vec2(table_width, self.row_height),
                        );
                        body_ui.painter().rect_filled(rect, 0.0, stripe);
                    }
                }

                for (column_index, (column_x, column_width)) in
                    column_positions(table_left, &state.widths, flex_width, self.splitter_width)
                        .enumerate()
                {
                    for row in visible.clone() {
                        let y = content_top + row as f32 * self.row_height;
                        let cell_rect = egui::Rect::from_min_size(
                            egui::pos2(column_x, y),
                            egui::vec2(column_width, self.row_height),
                        );
                        let mut child = body_ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(cell_rect.shrink2(padding))
                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        );
                        child.set_clip_rect(cell_rect.intersect(body_ui.clip_rect()));
                        cell(&mut child, row, column_index);
                    }
                }
            });

        let table_rect =
            header_rect.map_or(body.inner_rect, |header| header.union(body.inner_rect));
        let mut column_x = table_left;
        for (index, column) in columns[..n_fixed].iter().enumerate() {
            let original_width = state.widths[index];
            let splitter_x = column_x + original_width;
            let center_x = splitter_x + self.splitter_width / 2.0;
            let id = ui.id().with(("egui_splitter_table", index));
            let max_width =
                (state.widths[index] + (flex_width - self.min_width).max(0.0)).max(self.min_width);
            let hit = egui_widgets_config::splitter::interact(
                ui,
                table_rect,
                center_x,
                id,
                self.splitter_width / 2.0,
                1.0,
                &metrics,
            );
            if let Some(delta) = hit.delta {
                state.widths[index] =
                    (state.widths[index] + delta).clamp(self.min_width, max_width);
                changed = true;
            }
            if hit.double_clicked {
                state.widths[index] = column.default_width;
                changed = true;
            }
            column_x += original_width + self.splitter_width;
        }

        changed
    }
}

fn column_positions(
    left: f32,
    fixed: &[f32],
    flex: f32,
    splitter: f32,
) -> impl Iterator<Item = (f32, f32)> + '_ {
    fixed
        .iter()
        .copied()
        .chain(std::iter::once(flex))
        .scan(left, move |x, width| {
            let position = (*x, width);
            *x += width + splitter;
            Some(position)
        })
}

fn visible_row_range(
    viewport_min_y: f32,
    viewport_max_y: f32,
    row_height: f32,
    total_rows: usize,
) -> std::ops::Range<usize> {
    if total_rows == 0 || row_height <= 0.0 || !row_height.is_finite() {
        return 0..0;
    }
    let first = (viewport_min_y / row_height).floor().max(0.0) as usize;
    let end = (viewport_max_y / row_height).ceil().max(0.0) as usize;
    first.min(total_rows)..end.saturating_add(1).min(total_rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_len_resets_on_mismatch() {
        let mut s = SplitterTableState::new(vec![100.0, 200.0]);
        assert!(!s.ensure_len([10.0, 20.0].into_iter())); // same length → no reset
        assert_eq!(s.widths, vec![100.0, 200.0]);
        assert!(s.ensure_len([10.0, 20.0, 30.0].into_iter())); // length changed → reset
        assert_eq!(s.widths, vec![10.0, 20.0, 30.0]);
    }

    #[test]
    fn borrowed_columns_match_owned_geometry_and_retain_idle_width_storage() {
        for bounded in [false, true] {
            let columns = [
                Column::new("Name", 110.0),
                Column::new("Type", 60.0),
                Column::new("Value", 0.0),
            ];
            let render = |borrowed: bool| {
                let context = egui::Context::default();
                let mut state = SplitterTableState::default();
                let mut first_cells = Vec::new();
                let mut pointer = std::ptr::null();
                let mut capacity = 0;
                for frame in 0..3 {
                    let mut cells = Vec::new();
                    let mut output = context.run_ui(Default::default(), |ui| {
                        ui.set_max_width(500.0);
                        let table = SplitterTable::new(if borrowed {
                            Vec::new()
                        } else {
                            columns
                                .iter()
                                .map(|c| Column::new(&c.header, c.default_width))
                                .collect()
                        })
                        .row_height(24.0);
                        let table = if bounded {
                            table.max_body_height(72.0)
                        } else {
                            table
                        };
                        let draw = |ui: &mut Ui, row, col| {
                            cells.push((row, col, ui.max_rect(), ui.clip_rect()))
                        };
                        let changed = if borrowed {
                            table.show_with_columns(ui, &mut state, 10, &columns, draw)
                        } else {
                            table.show(ui, &mut state, 10, draw)
                        };
                        assert_eq!(changed, frame == 0);
                    });
                    output.textures_delta.clear();
                    if frame == 0 {
                        pointer = state.widths.as_ptr();
                        capacity = state.widths.capacity();
                        first_cells = cells;
                    } else {
                        assert_eq!(state.widths.as_ptr(), pointer);
                        assert_eq!(state.widths.capacity(), capacity);
                        assert_eq!(cells, first_cells);
                    }
                }
                (first_cells, state.widths)
            };
            assert_eq!(render(true), render(false));
        }
        assert_eq!(
            column_positions(10.0, &[80.0, 50.0], 100.0, 4.0).collect::<Vec<_>>(),
            [(10.0, 80.0), (94.0, 50.0), (148.0, 100.0)]
        );
    }

    #[test]
    fn borrowed_splitter_double_click_restores_column_default() {
        let ctx = egui::Context::default();
        let columns = [Column::new("Name", 80.0), Column::new("Value", 0.0)];
        let mut state = SplitterTableState::new(vec![120.0]);
        let mut position = egui::Pos2::ZERO;
        let mut render = |events: Vec<egui::Event>, time| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events,
                    time: Some(time),
                    ..Default::default()
                },
                |ui| {
                    ui.set_max_width(400.0);
                    position = egui::pos2(
                        ui.available_rect_before_wrap().left() + state.widths[0] + 2.0,
                        ui.available_rect_before_wrap().top() + 12.0,
                    );
                    SplitterTable::new(Vec::new())
                        .row_height(24.0)
                        .show_with_columns(ui, &mut state, 1, &columns, |_, _, _| {});
                },
            );
            output.textures_delta.clear();
            position
        };
        let pos = render(Vec::new(), 0.0);
        let button_at = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        render(
            vec![egui::Event::PointerMoved(pos), button_at(pos, true)],
            0.01,
        );
        let moved = pos + egui::vec2(20.0, 0.0);
        render(vec![egui::Event::PointerMoved(moved)], 0.02);
        render(vec![button_at(moved, false)], 0.03);
        let restored_position = render(Vec::new(), 1.0);
        assert!((restored_position.x - pos.x - 20.0).abs() < 0.1);
        let pos = restored_position;
        let button = |pressed| button_at(pos, pressed);
        render(vec![egui::Event::PointerMoved(pos), button(true)], 1.1);
        render(vec![button(false)], 1.15);
        render(vec![button(true)], 1.2);
        render(vec![button(false)], 1.25);
        assert_eq!(state.widths, [80.0]);
    }

    #[test]
    fn default_state_is_empty_until_first_show() {
        let s = SplitterTableState::default();
        assert!(s.widths.is_empty());
    }

    #[test]
    fn visible_rows_are_clamped_to_the_viewport_and_dataset() {
        assert_eq!(visible_row_range(0.0, 60.0, 20.0, 100), 0..4);
        assert_eq!(visible_row_range(200.0, 260.0, 20.0, 100), 10..14);
        assert_eq!(visible_row_range(1_980.0, 2_100.0, 20.0, 100), 99..100);
        assert_eq!(visible_row_range(0.0, 100.0, 0.0, 100), 0..0);
        assert_eq!(visible_row_range(0.0, 100.0, 20.0, 0), 0..0);
    }

    #[test]
    fn bounded_table_invokes_only_visible_cells_and_never_expands_the_panel_clip() {
        use std::cell::{Cell, RefCell};

        let context = egui::Context::default();
        let calls = Cell::new(0_usize);
        let clips = RefCell::new(Vec::new());
        let mut state = SplitterTableState::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 200.0));
        let input = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };

        // egui 0.36 made `TexturesDelta` panic on drop with unapplied deltas ("Deltas need to be
        // handled"). A headless frame has no painter to apply them to, so clear them the way the
        // panic message prescribes rather than discarding the output wholesale.
        let mut output = context.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                SplitterTable::new(vec![Column::new("Name", 140.0), Column::new("Value", 0.0)])
                    .row_height(20.0)
                    .max_body_height(80.0)
                    .show(ui, &mut state, 1_000, |cell_ui, _, _| {
                        calls.set(calls.get() + 1);
                        clips.borrow_mut().push(cell_ui.clip_rect());
                    });
            });
        });
        output.textures_delta.clear();

        assert!(calls.get() > 0);
        assert!(calls.get() < 40, "rendered {} cells", calls.get());
        assert!(
            clips
                .borrow()
                .iter()
                .all(|clip| screen.contains_rect(*clip))
        );
    }
}
