//! Adapted from pristine Warpbro dock.rs; native App/World ownership is injected by shell.
use egui_dock::{DockState,NodeIndex};
use egui_layout_manager::{LayoutManager,LayoutStore};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Panel {
    Viewport,
    Gallery,
    Bookmarks,
    Materials,
    Inspector,
    Settings,
    Export,
    Timeline,
    Outliner,
    MaterialLibrary,
    Takes,
}
impl Panel {
    pub const ALL: [Self; 11] = [
        Self::Viewport,
        Self::Gallery,
        Self::Bookmarks,
        Self::Materials,
        Self::Inspector,
        Self::Settings,
        Self::Export,
        Self::Timeline,
        Self::Outliner,
        Self::MaterialLibrary,
        Self::Takes,
    ];
    pub fn title(self) -> &'static str {
        match self {
            Self::Viewport => "Viewport",
            Self::Gallery => "Gallery",
            Self::Bookmarks => "Bookmarks",
            Self::Materials => "Materials",
            Self::MaterialLibrary => "Material Library",
            Self::Inspector => "Attribute Editor",
            Self::Settings => "Settings",
            Self::Export => "Render / Encode",
            Self::Timeline => "Timeline",
            Self::Outliner => "Outliner",
            Self::Takes => "Takes",
        }
    }
}

/// The start layout: the Attribute Editor over Settings / Render on the right, the Timeline
/// under the workspace, and Outliner / Gallery / Materials over Takes left of the Viewport.
pub fn default_layout() -> DockState<Panel> {
    let mut state = DockState::new(vec![Panel::Viewport]);
    let surface = state.main_surface_mut();
    let [workspace, right] = surface.split_right(NodeIndex::root(), 0.70, vec![Panel::Inspector]);
    surface.split_below(right, 0.62, vec![Panel::Settings, Panel::Export]);
    let [top, _] = surface.split_below(workspace, 0.82, vec![Panel::Timeline]);
    let [_, panels] = surface.split_left(
        top,
        0.23,
        vec![Panel::Outliner, Panel::Gallery, Panel::Materials],
    );
    surface.split_below(panels, 0.52, vec![Panel::Takes]);
    state
}

pub fn valid_layout(state: &DockState<Panel>) -> bool {
    let mut seen = std::collections::HashSet::new();
    state.iter_all_tabs().all(|(_, tab)| seen.insert(*tab)) && seen.contains(&Panel::Viewport)
}

// egui_dock 0.21 does not update WindowState::screen_rect. Preserve each
// floating window's drawn outer size and position as next-frame requests.
pub fn layout_blob(
    state: &DockState<Panel>,
    ctx: &egui::Context,
) -> Result<String, egui_dock_layout::Error> {
    let mut portable = state.clone();
    for (surface, _) in state.iter_surfaces_indexed() {
        if let Some(window) = portable.get_window_state_mut(surface) {
            // The id is the one used by egui_dock's window_surface.
            if let Some(area) =
                egui::AreaState::load(ctx, egui::Id::new(format!("window {surface:?}")))
                && area.rect().is_finite()
                && area.rect().width() > 0.0
                && area.rect().height() > 0.0
            {
                window.set_size(area.rect().size());
                window.set_position(area.left_top_pos());
            }
        }
    }
    egui_dock_layout::to_blob(&portable)
}

pub fn open_panel(state: &mut DockState<Panel>, panel: Panel) {
    if let Some(path) = state.find_tab(&panel) {
        let _ = state.set_active_tab(path);
        state.set_focused_node_and_surface(path.node_path());
    } else if matches!(
        panel,
        Panel::Settings | Panel::Export | Panel::MaterialLibrary
    ) {
        let surface = state.add_window(vec![panel]);
        if let Some(window) = state.get_window_state_mut(surface) {
            window.set_size(egui::vec2(850.0, 600.0));
        }
    } else {
        state.push_to_focused_leaf(panel);
    }
}

/// A reusable, exact stamp of authored layout state. Pixel rectangles of docked
/// leaves are derived during painting; floating outer rectangles are authored.
#[derive(Default)]
pub struct LayoutCache {
    stamp: Vec<u64>,
    scratch: Vec<u64>,
    window_ids: Vec<egui::Id>,
    pub blob: Option<String>,
    pub revision: u64,
    #[cfg(test)]
    pub serializations: usize,
}
impl LayoutCache {
    pub fn refresh(
        &mut self,
        state: &DockState<Panel>,
        ctx: &egui::Context,
    ) -> Result<(), egui_dock_layout::Error> {
        let stamp = &mut self.scratch;
        stamp.clear();
        let focused = state.focused_leaf();
        stamp.extend([
            focused.map_or(u64::MAX, |p| p.surface.0 as u64),
            focused.map_or(u64::MAX, |p| p.node.0 as u64),
        ]);
        for (index, surface) in state.iter_surfaces_indexed() {
            stamp.extend([
                index.0 as u64,
                match surface {
                    egui_dock::Surface::Empty => 0,
                    egui_dock::Surface::Main(_) => 1,
                    egui_dock::Surface::Window(_, _) => 2,
                },
            ]);
            let Some(tree) = surface.node_tree() else {
                continue;
            };
            stamp.push(tree.focused_leaf().map_or(u64::MAX, |p| p.0 as u64));
            for node in surface.iter_nodes() {
                match node {
                    egui_dock::Node::Empty => stamp.push(0),
                    egui_dock::Node::Leaf(leaf) => {
                        stamp.extend([
                            1,
                            leaf.tabs.len() as u64,
                            leaf.active.0 as u64,
                            leaf.scroll.to_bits() as u64,
                            leaf.collapsed as u64,
                            leaf.tab_bar_hidden as u64,
                        ]);
                        stamp.extend(leaf.tabs.iter().map(|panel| *panel as u64));
                    }
                    egui_dock::Node::Horizontal(split) | egui_dock::Node::Vertical(split) => {
                        stamp.extend([
                            if matches!(node, egui_dock::Node::Horizontal(_)) {
                                2
                            } else {
                                3
                            },
                            split.fraction.to_bits() as u64,
                            split.fully_collapsed as u64,
                            split.collapsed_leaf_count as u64,
                        ]);
                    }
                }
            }
            stamp.push(u64::MAX - 1);
            if let egui_dock::Surface::Window(_, window) = surface {
                while self.window_ids.len() <= index.0 {
                    let surface = egui_dock::SurfaceIndex(self.window_ids.len());
                    self.window_ids
                        .push(egui::Id::new(format!("window {surface:?}")));
                }
                let rect = egui::AreaState::load(ctx, self.window_ids[index.0])
                    .map(|area| area.rect())
                    .filter(|rect| rect.is_finite() && rect.is_positive())
                    .unwrap_or_else(|| window.rect());
                stamp.extend([
                    rect.min.x.to_bits() as u64,
                    rect.min.y.to_bits() as u64,
                    rect.max.x.to_bits() as u64,
                    rect.max.y.to_bits() as u64,
                ]);
            }
        }
        if self.blob.is_some() && self.stamp == self.scratch {
            return Ok(());
        }
        let blob = layout_blob(state, ctx)?;
        #[cfg(test)]
        {
            self.serializations += 1;
        }
        if self.blob.as_ref() != Some(&blob) {
            self.revision = self.revision.wrapping_add(1);
            self.blob = Some(blob);
        }
        std::mem::swap(&mut self.stamp, &mut self.scratch);
        Ok(())
    }
    pub fn invalidate(&mut self) {
        self.stamp.clear();
    }
}

/// Persistent presets are separate from transient widget state.
pub struct Layouts {
    pub store: LayoutStore,
    pub manager: LayoutManager,
    pub cache: LayoutCache,
}
impl Default for Layouts {
    fn default() -> Self {
        Self {
            store: LayoutStore::new(),
            manager: LayoutManager::new().with_combo_width(140.0),
            cache: LayoutCache::default(),
        }
    }
}

pub fn restore_layout(blob: &str) -> Result<DockState<Panel>, String> {
    let state = egui_dock_layout::from_blob(blob).map_err(|e| e.to_string())?;
    if valid_layout(&state) {
        Ok(state)
    } else {
        Err("layout.invalid".into())
    }
}

