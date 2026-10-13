//! A generic, app-agnostic media / asset browser panel for egui 0.34.
//!
//! Renders a flat, caller-owned [`AssetBrowserModel`] as a scrollable list of
//! rows **grouped by kind**, with:
//! * a top action-button row (caller-defined "Create" entries + "Add media"),
//! * selection (plain / ctrl-toggle / shift-range) and hover highlight,
//! * a per-row thumbnail, name, dimmed subtitle badge and a delete control,
//! * double-click → [`AssetAction::Open`],
//! * a right-click "Create" context menu (per-item and on empty space),
//! * optional in-place rename, and
//! * drag-out start reporting via [`AssetAction::BeginDrag`].
//!
//! The widget holds NO persistent state of its own beyond small bits of egui
//! ephemeral data (the shift-range anchor and the in-progress rename buffer);
//! all asset state lives in the caller's model. It runs NO file dialogs — the
//! "Add media" button and item "Open" are reported as actions for the caller to
//! handle (with `rfd` or whatever it likes).
//!
//! ## Usage
//! ```no_run
//! # use egui_asset_browser::{AssetBrowserModel, AssetBrowserConfig, AssetItem, AssetAction, show};
//! # fn ui_demo(ui: &mut egui::Ui, model: &mut AssetBrowserModel) {
//! let config = AssetBrowserConfig::default()
//!     .with_create_kinds(["Comp", "Camera", "Text"]);
//! for action in show(ui, model, &config) {
//!     match action {
//!         AssetAction::Select { id, additive, range } => { /* update model.selection */ }
//!         AssetAction::Open { id } => { /* activate */ }
//!         AssetAction::Create { kind } => { /* push a new AssetItem */ }
//!         AssetAction::Delete { id } => { /* remove from model.items */ }
//!         AssetAction::AddMedia => { /* open your own file dialog */ }
//!         _ => {}
//!     }
//! }
//! # }
//! ```
//!
//! Gathered from playa's `project_ui.rs`; decoupled from
//! `playa_engine::{Project, Player, Node}`, `ProjectActions`, `uuid` and `rfd`.

mod action;
mod model;

pub use action::AssetAction;
pub use model::{AssetBrowserModel, AssetItem, KindStyle};

use egui::{Align2, Color32, Id, Rect, Sense, Stroke, TextEdit, Ui, Vec2, epaint::StrokeKind};
use std::collections::HashMap;

/// Static, caller-supplied configuration for one browser instance.
///
/// Cheap to build each frame (or stash once). `create_kinds` populates both the
/// top action-button row and the right-click "Create" menu; `kind_styles` maps
/// an [`AssetItem::kind`] to its icon + colour.
#[derive(Clone, Debug, Default)]
pub struct AssetBrowserConfig {
    /// Labels offered in the "Create" menu / buttons. Each click emits
    /// [`AssetAction::Create`] with the label as `kind`.
    pub create_kinds: Vec<String>,
    /// Per-kind icon/colour styling, keyed by [`AssetItem::kind`].
    pub kind_styles: HashMap<String, KindStyle>,
    /// Show the "Add media" button (emits [`AssetAction::AddMedia`]).
    pub show_add_media: bool,
    /// Allow double-slow-path in-place rename: a long-press / context "Rename".
    /// When enabled, the context menu gains a "Rename" entry that turns the row
    /// into a text field; committing emits [`AssetAction::Rename`].
    pub allow_rename: bool,
    /// Group rows under a header per kind. When false, rows render in `items`
    /// order with no section headers.
    pub group_by_kind: bool,
}

impl AssetBrowserConfig {
    /// Builder: set the "Create" entry labels.
    pub fn with_create_kinds<I, S>(mut self, kinds: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.create_kinds = kinds.into_iter().map(Into::into).collect();
        self
    }

    /// Builder: register a per-kind style.
    pub fn with_kind_style(mut self, kind: impl Into<String>, style: KindStyle) -> Self {
        self.kind_styles.insert(kind.into(), style);
        self
    }

    /// Builder: enable the "Add media" button.
    pub fn with_add_media(mut self, show: bool) -> Self {
        self.show_add_media = show;
        self
    }

    /// Builder: enable in-place rename.
    pub fn with_rename(mut self, allow: bool) -> Self {
        self.allow_rename = allow;
        self
    }

    /// Builder: enable kind grouping with section headers.
    pub fn with_grouping(mut self, group: bool) -> Self {
        self.group_by_kind = group;
        self
    }

    /// Style for `kind`, falling back to a neutral default.
    fn style_for(&self, kind: &str) -> KindStyle {
        self.kind_styles.get(kind).cloned().unwrap_or_default()
    }
}

/// egui-data key for the id of the row currently being renamed (if any).
fn rename_id_key(base: Id) -> Id {
    base.with("__rename_id")
}

/// egui-data key for the in-progress rename text buffer.
fn rename_buf_key(base: Id) -> Id {
    base.with("__rename_buf")
}

/// Render the browser into `ui` and return this frame's user actions.
///
/// `model` is read-only here (the widget never mutates it); apply the returned
/// [`AssetAction`]s to your own copy. The panel fills the available rectangle.
pub fn show(
    ui: &mut Ui,
    model: &AssetBrowserModel,
    config: &AssetBrowserConfig,
) -> Vec<AssetAction> {
    let mut actions = Vec::new();
    let base_id = ui.id().with("asset_browser");

    // Full-rect interaction for empty-area click (clear selection), empty-area
    // right-click (Create menu) and hover tracking. Reserved BEFORE the rows so
    // per-row interactions (allocated later, on top) take click precedence.
    let panel_rect = ui.available_rect_before_wrap();
    let panel_response = ui.interact(panel_rect, base_id.with("panel"), Sense::click());

    // --- Action button row: Create entries + Add media -------------------
    ui.horizontal(|ui| {
        for kind in &config.create_kinds {
            if ui.button(format!("+ {kind}")).clicked() {
                actions.push(AssetAction::Create { kind: kind.clone() });
            }
        }
        if config.show_add_media {
            if !config.create_kinds.is_empty() {
                ui.separator();
            }
            if ui.button("Add media").clicked() {
                actions.push(AssetAction::AddMedia);
            }
        }
    });
    ui.separator();

    // --- Scrollable, grouped row list ------------------------------------
    let scroll_height = ui.available_height();
    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            ui.set_min_height(scroll_height);

            if model.items.is_empty() {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    let weak = ui.visuals().weak_text_color();
                    ui.colored_label(weak, "No assets");
                    if config.show_add_media {
                        ui.colored_label(weak, "Click 'Add media' to load files");
                    }
                });
                return;
            }

            let mut last_kind: Option<&str> = None;
            for (idx, item) in model.items.iter().enumerate() {
                // Section header at the first item of each kind.
                if config.group_by_kind && last_kind != Some(item.kind.as_str()) {
                    if last_kind.is_some() {
                        ui.add_space(4.0);
                    }
                    ui.label(
                        egui::RichText::new(&item.kind)
                            .small()
                            .color(ui.visuals().weak_text_color()),
                    );
                    last_kind = Some(item.kind.as_str());
                }
                row(ui, base_id, model, config, item, idx, &mut actions);
                ui.add_space(1.0);
            }
        });

    // Empty-area right-click → Create-only menu. Per-item menus take precedence
    // because their (smaller) rects are interacted on top of the panel rect.
    if !config.create_kinds.is_empty() {
        panel_response.context_menu(|ui| create_menu(ui, config, &mut actions));
    }

    // Plain click on empty space clears the selection.
    if panel_response.clicked() {
        actions.push(AssetAction::ClearSelection);
    }

    actions
}

/// Render a single asset row and push any resulting actions.
///
/// Hand-painted (not a `Button`) to faithfully keep playa's look: a filled
/// background that lights up on selection/hover, an optional thumbnail, an icon,
/// the name (clipped), a dimmed right-aligned subtitle and a small delete box.
#[allow(clippy::too_many_arguments)]
fn row(
    ui: &mut Ui,
    base_id: Id,
    model: &AssetBrowserModel,
    config: &AssetBrowserConfig,
    item: &AssetItem,
    _idx: usize,
    actions: &mut Vec<AssetAction>,
) {
    // In-place rename path: replace the row with a focused text field.
    let renaming = ui.data(|d| d.get_temp::<u64>(rename_id_key(base_id))) == Some(item.id);
    if renaming {
        rename_row(ui, base_id, item, actions);
        return;
    }

    // Resolve shared design tokens once for this row's hand-painted chrome.
    let tok = egui_widgets_config::WidgetsConfig::get(ui);

    let style = config.style_for(&item.kind);
    let is_selected = model.is_selected(item.id);

    let avail_w = ui.available_width();
    let row_h = ui.spacing().interact_size.y * 1.2;
    let (row_rect, response) =
        ui.allocate_exact_size(Vec2::new(avail_w, row_h), Sense::click_and_drag());

    // Background: faint card base, then the shared (uniform) selection/hover
    // highlight from `list::row_fill` so the lit colour matches every list widget.
    let sem = egui_widgets_config::semantic(ui);
    let painter = ui.painter();
    painter.rect_filled(row_rect, 2.0, ui.style().visuals.faint_bg_color);
    egui_widgets_config::list::row_fill(painter, row_rect, &sem, is_selected, response.hovered());
    painter.rect_stroke(
        row_rect,
        2.0,
        Stroke::new(
            tok.metrics.stroke_thin,
            ui.style().visuals.window_stroke.color,
        ),
        StrokeKind::Inside,
    );

    let center_y = row_rect.center().y;
    let mut cursor_x = row_rect.min.x + 8.0;

    // Thumbnail (square, row-height-fit).
    if let Some(tex) = item.thumbnail {
        let s = row_h - 4.0;
        let thumb_rect =
            Rect::from_min_size(egui::pos2(cursor_x, center_y - s * 0.5), Vec2::splat(s));
        ui.painter().image(
            tex,
            thumb_rect,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        cursor_x += s + 6.0;
    }

    // Icon.
    if !style.icon.is_empty() {
        let galley = ui.painter().layout_no_wrap(
            style.icon.clone(),
            egui_widgets_config::prop(tok.fonts.md),
            style.color,
        );
        let pos = egui::pos2(cursor_x, center_y - galley.size().y * 0.5);
        let w = galley.size().x;
        ui.painter().galley(pos, galley, style.color);
        cursor_x += w + 8.0;
    }

    // Right-aligned subtitle + delete box positions.
    let weak = ui.visuals().weak_text_color();
    let delete_size = Vec2::splat(16.0);
    let delete_pos = egui::pos2(
        row_rect.max.x - delete_size.x - 6.0,
        center_y - delete_size.y * 0.5,
    );

    let mut name_right = delete_pos.x - 8.0;
    if let Some(sub) = &item.subtitle {
        let galley =
            ui.painter()
                .layout_no_wrap(sub.clone(), egui_widgets_config::prop(tok.fonts.md), weak);
        let pos = egui::pos2(
            delete_pos.x - 8.0 - galley.size().x,
            center_y - galley.size().y * 0.5,
        );
        name_right = pos.x - 8.0;
        ui.painter().galley(pos, galley, weak);
    }

    // Name, clipped to the space before the subtitle.
    let name_max_w = name_right - cursor_x;
    if name_max_w > 0.0 {
        let text_color = ui.visuals().text_color();
        let galley = ui.painter().layout_no_wrap(
            item.name.clone(),
            egui_widgets_config::prop(tok.fonts.md),
            text_color,
        );
        let pos = egui::pos2(cursor_x, center_y - galley.size().y * 0.5);
        let clip = Rect::from_min_size(pos, Vec2::new(name_max_w, row_h));
        ui.painter()
            .with_clip_rect(clip)
            .galley(pos, galley, text_color);
    }

    // Delete box (own click target so it doesn't trigger selection).
    let delete_rect = Rect::from_min_size(delete_pos, delete_size);
    let delete_resp = ui.interact(delete_rect, base_id.with(("del", item.id)), Sense::click());
    if ui.is_rect_visible(delete_rect) {
        let dbg = if delete_resp.hovered() {
            ui.visuals().widgets.hovered.weak_bg_fill
        } else {
            ui.visuals().extreme_bg_color
        };
        ui.painter().rect_filled(delete_rect, 2.0, dbg);
        ui.painter().rect_stroke(
            delete_rect,
            2.0,
            Stroke::new(tok.metrics.stroke_thin, weak),
            StrokeKind::Inside,
        );
        ui.painter().text(
            delete_rect.center(),
            Align2::CENTER_CENTER,
            "X",
            egui_widgets_config::prop(tok.fonts.sm),
            weak,
        );
    }
    if delete_resp.clicked() {
        actions.push(AssetAction::Delete { id: item.id });
    }

    // Selection (single click) and open (double click).
    let mods = ui.input(|i| i.modifiers);
    if response.clicked() {
        actions.push(AssetAction::Select {
            id: item.id,
            additive: mods.command || mods.ctrl,
            range: mods.shift,
        });
    }
    if response.double_clicked() {
        actions.push(AssetAction::Select {
            id: item.id,
            additive: mods.command || mods.ctrl,
            range: mods.shift,
        });
        actions.push(AssetAction::Open { id: item.id });
    }

    // Drag-out start: report once when the gesture begins. Cursor feedback
    // mirrors playa (grab / grabbing).
    if response.drag_started() {
        actions.push(AssetAction::BeginDrag { id: item.id });
    }
    if response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }

    // Right-click menu: item actions on top, Create menu below.
    response.context_menu(|ui| {
        if config.allow_rename && ui.button("Rename").clicked() {
            ui.data_mut(|d| {
                d.insert_temp(rename_id_key(base_id), item.id);
                d.insert_temp(rename_buf_key(base_id), item.name.clone());
            });
            ui.close();
        }
        if ui.button("Delete").clicked() {
            actions.push(AssetAction::Delete { id: item.id });
            ui.close();
        }
        if !config.create_kinds.is_empty() {
            ui.separator();
            create_menu(ui, config, actions);
        }
    });
}

/// Render the in-place rename field for `item`. Commits on Enter / focus loss,
/// cancels on Escape; clears the rename egui-data either way.
fn rename_row(ui: &mut Ui, base_id: Id, item: &AssetItem, actions: &mut Vec<AssetAction>) {
    let mut buf = ui
        .data(|d| d.get_temp::<String>(rename_buf_key(base_id)))
        .unwrap_or_else(|| item.name.clone());

    let resp = ui.add(
        TextEdit::singleline(&mut buf)
            .id(base_id.with(("rename_edit", item.id)))
            .desired_width(f32::INFINITY),
    );
    resp.request_focus();

    let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let committed = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

    if committed {
        let new_name = buf.trim().to_string();
        if !new_name.is_empty() && new_name != item.name {
            actions.push(AssetAction::Rename {
                id: item.id,
                new_name,
            });
        }
        clear_rename(ui, base_id);
    } else if escaped || (resp.lost_focus() && !committed) {
        clear_rename(ui, base_id);
    } else {
        // Persist the edited buffer for the next frame.
        ui.data_mut(|d| d.insert_temp(rename_buf_key(base_id), buf));
    }
}

/// Drop the in-progress rename state from egui data.
fn clear_rename(ui: &mut Ui, base_id: Id) {
    ui.data_mut(|d| {
        d.remove::<u64>(rename_id_key(base_id));
        d.remove::<String>(rename_buf_key(base_id));
    });
}

/// The shared "Create" submenu used by the per-item and empty-area menus.
/// Each entry emits [`AssetAction::Create`] and closes the popup.
fn create_menu(ui: &mut Ui, config: &AssetBrowserConfig, actions: &mut Vec<AssetAction>) {
    ui.menu_button("Create", |ui| {
        for kind in &config.create_kinds {
            if ui.button(format!("+ {kind}")).clicked() {
                actions.push(AssetAction::Create { kind: kind.clone() });
                ui.close();
            }
        }
    });
}
