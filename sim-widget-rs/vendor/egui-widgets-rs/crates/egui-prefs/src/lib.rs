//! Pluggable preferences for egui applications.
//!
//! # Why this crate exists
//!
//! Mature creative tools (After Effects, Houdini, Maya) let each module own
//! its preferences UI; a central Preferences window aggregates them via a
//! tree-view sidebar. Hardcoding every panel into one place is the
//! anti-pattern.
//!
//! `playa-prefs` provides the infrastructure: a generic
//! [`PrefsRegistry`]`<S>` of [`PrefsEntry`]`<S>` items (each carrying its own
//! render closure) plus a modal [`PrefsWindow`] that draws the sidebar /
//! content / action-buttons.
//!
//! `S` is the application's settings type (any `Clone + PartialEq`). Each
//! entry's render closure receives `&mut S` (or, idiomatically, a slice of
//! `S` extracted by the caller) and mutates the working copy.
//!
//! # State machine
//!
//! 1. [`PrefsWindow::open_with`] takes a clone of the current settings as the
//!    working copy and the last-applied baseline.
//! 2. The user edits the working copy via render callbacks.
//! 3. [`PrefsWindow::apply`] commits the working copy back to the host's
//!    state and updates the baseline (window stays open).
//! 4. [`PrefsWindow::ok`] does an apply then closes.
//! 5. [`PrefsWindow::cancel`] discards the working copy (host state
//!    untouched) and closes.
//!
//! [`PrefsWindow::show`] is the egui-driven entry point that wires the above
//! to button clicks, returning a [`PrefsResult`].

#![forbid(unsafe_code)]

use egui::{Context, Layout, ScrollArea, Ui, Window};

/// Visible-entry indices grouped by category label (categorized sidebar).
type CategoryIndex = std::collections::HashMap<&'static str, Vec<usize>>;

/// A panel's render callback: draws into `ui`, mutating the working settings `S`.
/// `Send + Sync` so the registry can live in thread-safe app state.
pub type RenderFn<S> = Box<dyn FnMut(&mut Ui, &mut S) + Send + Sync>;

// =============================================================================
// Entry + Registry
// =============================================================================

/// A single preferences panel registered for inclusion in the tree-view.
pub struct PrefsEntry<S> {
    /// Stable id used for selection state and tests (e.g. `"jobs"`).
    pub id: &'static str,
    /// Human-readable label shown in the tree-view (e.g. `"Jobs & Rendering"`).
    pub label: &'static str,
    /// Tree-view category (e.g. `"App"`, `"Integrations"`).
    pub category: &'static str,
    /// Free-text keywords matched by the search bar in addition to `label`.
    pub search_keywords: Vec<&'static str>,
    /// Render the panel into `ui`, mutating the working copy of settings.
    /// `Send + Sync` so the registry can be shared across threads (for
    /// thread-safe app state, anyway). See [`RenderFn`].
    pub render: RenderFn<S>,
}

impl<S> std::fmt::Debug for PrefsEntry<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrefsEntry")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("category", &self.category)
            .field("search_keywords", &self.search_keywords)
            .finish_non_exhaustive()
    }
}

/// Collection of [`PrefsEntry`] handed to a [`PrefsWindow::show`] call.
pub struct PrefsRegistry<S> {
    entries: Vec<PrefsEntry<S>>,
}

impl<S> PrefsRegistry<S> {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn add(&mut self, entry: PrefsEntry<S>) {
        self.entries.push(entry);
    }

    pub fn iter(&self) -> std::slice::Iter<'_, PrefsEntry<S>> {
        self.entries.iter()
    }

    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, PrefsEntry<S>> {
        self.entries.iter_mut()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn find_by_id(&self, id: &str) -> Option<&PrefsEntry<S>> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Visible entries given a search query (case-insensitive substring match
    /// against label and search_keywords). Empty query → all entries.
    pub fn filtered_indices(&self, query: &str) -> Vec<usize> {
        let q = query.trim().to_ascii_lowercase();
        if q.is_empty() {
            return (0..self.entries.len()).collect();
        }
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                e.label.to_ascii_lowercase().contains(&q)
                    || e.category.to_ascii_lowercase().contains(&q)
                    || e.search_keywords
                        .iter()
                        .any(|k| k.to_ascii_lowercase().contains(&q))
            })
            .map(|(i, _)| i)
            .collect()
    }
}

impl<S> Default for PrefsRegistry<S> {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Result + Window state machine
// =============================================================================

/// Outcome of one [`PrefsWindow::show`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefsResult {
    /// Window not visible this frame.
    Closed,
    /// Window visible, no action this frame.
    Open,
    /// User clicked **Apply**: working copy was committed to host state, the
    /// baseline updated, window stays open.
    Applied,
    /// User clicked **OK**: working copy committed AND window closed.
    OkClosed,
    /// User clicked **Cancel**: working copy discarded, host state
    /// unchanged, window closed.
    Cancelled,
}

/// How the left sidebar lists the registered panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SidebarMode {
    /// Group panels under collapsible category headers (the tree-view look).
    #[default]
    Categorized,
    /// A flat list of panels (registry order), no category grouping — the
    /// "set of tabs down the left" look. Categories are ignored for layout but
    /// still searchable.
    Flat,
}

/// Modal preferences window with sidebar tree-view, search bar, and
/// Apply/OK/Cancel buttons.
///
/// Generic over `S` — the application's settings type. `S` must be
/// `Clone + PartialEq` so the window can keep a working copy and detect a
/// dirty state for the Apply button.
pub struct PrefsWindow<S> {
    open: bool,
    selected_entry_id: Option<String>,
    search_text: String,
    /// Edits in progress; `None` when the window is closed.
    working_copy: Option<S>,
    /// Snapshot of the host state at last open / last apply; used to detect
    /// "dirty" for the Apply button and to know if Cancel needs to do
    /// anything (it does nothing host-side because we never mutate host
    /// state until Apply).
    last_applied: Option<S>,
    /// Sidebar layout (categorized tree vs flat tabs).
    sidebar_mode: SidebarMode,
}

impl<S> PrefsWindow<S>
where
    S: Clone + PartialEq,
{
    pub fn new() -> Self {
        Self {
            open: false,
            selected_entry_id: None,
            search_text: String::new(),
            working_copy: None,
            last_applied: None,
            sidebar_mode: SidebarMode::default(),
        }
    }

    /// Builder: choose the sidebar layout (default [`SidebarMode::Categorized`]).
    pub fn with_sidebar_mode(mut self, mode: SidebarMode) -> Self {
        self.sidebar_mode = mode;
        self
    }

    /// Set the sidebar layout at runtime.
    pub fn set_sidebar_mode(&mut self, mode: SidebarMode) {
        self.sidebar_mode = mode;
    }

    /// The current sidebar layout.
    pub fn sidebar_mode(&self) -> SidebarMode {
        self.sidebar_mode
    }

    /// Open the window with `state` cloned into the working copy.
    pub fn open_with(&mut self, state: &S) {
        self.open = true;
        self.working_copy = Some(state.clone());
        self.last_applied = Some(state.clone());
    }

    /// Is the modal currently visible?
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Working copy differs from the baseline?
    pub fn is_dirty(&self) -> bool {
        match (&self.working_copy, &self.last_applied) {
            (Some(w), Some(la)) => w != la,
            _ => false,
        }
    }

    /// Mutable access to the working copy. Returns `None` when the window is
    /// closed (open the window first).
    pub fn working_mut(&mut self) -> Option<&mut S> {
        self.working_copy.as_mut()
    }

    /// Read-only access to the working copy.
    pub fn working(&self) -> Option<&S> {
        self.working_copy.as_ref()
    }

    /// Apply: commit `working_copy → state` and update the baseline.
    /// Window stays open.
    pub fn apply(&mut self, state: &mut S) {
        if let Some(w) = &self.working_copy {
            *state = w.clone();
            self.last_applied = Some(w.clone());
        }
    }

    /// OK: apply + close.
    pub fn ok(&mut self, state: &mut S) {
        self.apply(state);
        self.close();
    }

    /// Cancel: discard working copy, close. Host state untouched.
    pub fn cancel(&mut self) {
        self.close();
    }

    /// Close + reset internal state.
    pub fn close(&mut self) {
        self.open = false;
        self.working_copy = None;
        self.last_applied = None;
        self.selected_entry_id = None;
        self.search_text.clear();
    }

    /// Render and drive the window. Returns the action taken this frame.
    ///
    /// Caller is responsible for opening the window via [`Self::open_with`]
    /// before this is called; if `is_open()` is false this is a no-op
    /// returning [`PrefsResult::Closed`].
    pub fn show(
        &mut self,
        ctx: &Context,
        registry: &mut PrefsRegistry<S>,
        state: &mut S,
    ) -> PrefsResult {
        if !self.open {
            return PrefsResult::Closed;
        }

        // Pre-select first entry on first frame after open.
        if self.selected_entry_id.is_none()
            && let Some(first) = registry.entries.first()
        {
            self.selected_entry_id = Some(first.id.to_string());
        }

        // Snapshot dirty before the body so the Apply button reflects the
        // state at the START of this frame (mutations during render still
        // count toward the next frame's display).
        let mut result = PrefsResult::Open;
        let mut requested_close = false;

        // We need to keep `self.open` toggleable by the egui window's `[x]`
        // close button without holding two mutable borrows of `self` inside
        // the closure. Separate the bool, restore at the end.
        let mut window_open = self.open;

        Window::new("Preferences")
            .open(&mut window_open)
            .resizable(true)
            .default_size([720.0, 480.0])
            .collapsible(false)
            .show(ctx, |ui| {
                // Top bar — search.
                ui.horizontal(|ui| {
                    ui.label(egui_widgets_config::icons::SEARCH);
                    ui.text_edit_singleline(&mut self.search_text);
                    if !self.search_text.is_empty()
                        && ui.small_button(egui_widgets_config::icons::CLOSE).clicked()
                    {
                        self.search_text.clear();
                    }
                });
                ui.separator();

                let visible = registry.filtered_indices(&self.search_text);

                // Bottom bar — Apply / Cancel / OK.
                egui::Panel::bottom("prefs_actions")
                    .resizable(false)
                    .min_size(32.0)
                    .show(ui, |ui| {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("OK").clicked() {
                                    if let Some(w) = &self.working_copy {
                                        *state = w.clone();
                                        self.last_applied = Some(w.clone());
                                    }
                                    result = PrefsResult::OkClosed;
                                    requested_close = true;
                                }
                                if ui.button("Cancel").clicked() {
                                    result = PrefsResult::Cancelled;
                                    requested_close = true;
                                }
                                let dirty = match (&self.working_copy, &self.last_applied) {
                                    (Some(w), Some(la)) => w != la,
                                    _ => false,
                                };
                                if ui.add_enabled(dirty, egui::Button::new("Apply")).clicked() {
                                    if let Some(w) = &self.working_copy {
                                        *state = w.clone();
                                        self.last_applied = Some(w.clone());
                                    }
                                    result = PrefsResult::Applied;
                                }
                            });
                        });
                    });

                // Sidebar — tree-view grouped by category.
                egui::Panel::left("prefs_tree")
                    .resizable(true)
                    .default_size(200.0)
                    .show(ui, |ui| {
                        ScrollArea::vertical().show(ui, |ui| {
                            if visible.is_empty() {
                                ui.weak("(no matches)");
                                return;
                            }
                            match self.sidebar_mode {
                                SidebarMode::Flat => {
                                    // Flat list of panels (registry order), no grouping.
                                    for &i in &visible {
                                        let e = &registry.entries[i];
                                        let selected =
                                            self.selected_entry_id.as_deref() == Some(e.id);
                                        if egui_prefs2::sidebar_item(
                                            ui, selected, "", e.label, 11.0,
                                        )
                                        .clicked()
                                        {
                                            self.selected_entry_id = Some(e.id.to_string());
                                        }
                                    }
                                }
                                SidebarMode::Categorized => {
                                    // Group visible entries by category, preserving
                                    // first-seen order for both categories and items.
                                    let mut order: Vec<&'static str> = Vec::new();
                                    let mut by_category: CategoryIndex = CategoryIndex::new();
                                    for &i in &visible {
                                        let cat = registry.entries[i].category;
                                        if !by_category.contains_key(cat) {
                                            order.push(cat);
                                        }
                                        by_category.entry(cat).or_default().push(i);
                                    }
                                    for cat in order {
                                        ui.collapsing(cat, |ui| {
                                            for i in &by_category[cat] {
                                                let e = &registry.entries[*i];
                                                let selected =
                                                    self.selected_entry_id.as_deref() == Some(e.id);
                                                if egui_prefs2::sidebar_item(
                                                    ui, selected, "", e.label, 11.0,
                                                )
                                                .clicked()
                                                {
                                                    self.selected_entry_id = Some(e.id.to_string());
                                                }
                                            }
                                        });
                                    }
                                }
                            }
                        });
                    });

                // Central — render selected entry.
                egui::CentralPanel::default().show(ui, |ui| {
                    let selected_idx = self
                        .selected_entry_id
                        .as_deref()
                        .and_then(|id| registry.entries.iter().position(|e| e.id == id));
                    match (selected_idx, self.working_copy.as_mut()) {
                        (Some(idx), Some(working)) => {
                            let entry = &mut registry.entries[idx];
                            egui_prefs2::category_header(ui, "", entry.label, 13.5);
                            ScrollArea::vertical().show(ui, |ui| {
                                // Match egui-prefs2's compact body sizing so the modal and the
                                // egui-prefs2 panel read as one design.
                                if let Some(f) =
                                    ui.style_mut().text_styles.get_mut(&egui::TextStyle::Body)
                                {
                                    f.size = 11.0;
                                }
                                if let Some(f) =
                                    ui.style_mut().text_styles.get_mut(&egui::TextStyle::Button)
                                {
                                    f.size = 11.0;
                                }
                                (entry.render)(ui, working);
                            });
                        }
                        _ => {
                            ui.centered_and_justified(|ui| {
                                ui.weak("Select a panel from the sidebar.");
                            });
                        }
                    }
                });
            });

        // Sync window-open state back. The egui window's [x] close button
        // toggles `window_open` to false; we treat that as Cancel.
        if !window_open && self.open {
            // User clicked the [x]. Treat as Cancel.
            result = PrefsResult::Cancelled;
            requested_close = true;
        }

        if requested_close {
            self.close();
        }

        result
    }
}

impl<S> Default for PrefsWindow<S>
where
    S: Clone + PartialEq,
{
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Stand-in for an application settings type.
    #[derive(Debug, Clone, PartialEq, Default)]
    struct TestSettings {
        volume: u8,
        verbose: bool,
        name: String,
    }

    fn build_registry() -> PrefsRegistry<TestSettings> {
        let mut r = PrefsRegistry::<TestSettings>::new();
        r.add(PrefsEntry {
            id: "audio",
            label: "Audio",
            category: "Output",
            search_keywords: vec!["volume", "loudness"],
            render: Box::new(|_, _| {}),
        });
        r.add(PrefsEntry {
            id: "logging",
            label: "Logging",
            category: "Diagnostics",
            search_keywords: vec!["verbose", "debug"],
            render: Box::new(|_, _| {}),
        });
        r.add(PrefsEntry {
            id: "identity",
            label: "Identity",
            category: "Output",
            search_keywords: vec!["name", "branding"],
            render: Box::new(|_, _| {}),
        });
        r
    }

    #[test]
    fn registry_add_and_iter_preserve_order() {
        let r = build_registry();
        assert_eq!(r.len(), 3);
        let ids: Vec<&str> = r.iter().map(|e| e.id).collect();
        assert_eq!(ids, ["audio", "logging", "identity"]);
    }

    #[test]
    fn registry_find_by_id() {
        let r = build_registry();
        assert_eq!(r.find_by_id("logging").map(|e| e.label), Some("Logging"));
        assert!(r.find_by_id("nonexistent").is_none());
    }

    #[test]
    fn filter_empty_query_returns_all() {
        let r = build_registry();
        assert_eq!(r.filtered_indices("").len(), 3);
        assert_eq!(r.filtered_indices("   ").len(), 3);
    }

    #[test]
    fn filter_by_label() {
        let r = build_registry();
        let v = r.filtered_indices("audio");
        assert_eq!(v, vec![0]);
    }

    #[test]
    fn filter_by_keyword_case_insensitive() {
        let r = build_registry();
        let v = r.filtered_indices("VERBOSE");
        assert_eq!(v, vec![1]);
    }

    #[test]
    fn filter_by_category() {
        let r = build_registry();
        let v = r.filtered_indices("Output");
        // "audio" + "identity" both in Output category.
        assert_eq!(v, vec![0, 2]);
    }

    #[test]
    fn filter_no_matches() {
        let r = build_registry();
        assert!(r.filtered_indices("zzzzzz").is_empty());
    }

    #[test]
    fn window_starts_closed() {
        let w = PrefsWindow::<TestSettings>::new();
        assert!(!w.is_open());
        assert!(w.working().is_none());
    }

    #[test]
    fn open_with_clones_state_into_working_copy() {
        let mut w = PrefsWindow::<TestSettings>::new();
        let state = TestSettings {
            volume: 70,
            verbose: true,
            name: "joss".into(),
        };
        w.open_with(&state);
        assert!(w.is_open());
        assert_eq!(w.working(), Some(&state));
        assert!(!w.is_dirty(), "freshly-opened window must be clean");
    }

    #[test]
    fn dirty_flips_when_working_copy_diverges() {
        let mut w = PrefsWindow::<TestSettings>::new();
        let state = TestSettings::default();
        w.open_with(&state);
        assert!(!w.is_dirty());
        w.working_mut().unwrap().volume = 99;
        assert!(w.is_dirty());
    }

    #[test]
    fn apply_commits_working_into_host_and_clears_dirty() {
        let mut w = PrefsWindow::<TestSettings>::new();
        let mut state = TestSettings::default();
        w.open_with(&state);
        w.working_mut().unwrap().volume = 50;
        assert!(w.is_dirty());

        w.apply(&mut state);
        assert_eq!(state.volume, 50, "host state received the change");
        assert!(
            !w.is_dirty(),
            "Apply re-baselines so the window is no longer dirty"
        );
        assert!(w.is_open(), "Apply does not close the window");
    }

    #[test]
    fn ok_applies_then_closes() {
        let mut w = PrefsWindow::<TestSettings>::new();
        let mut state = TestSettings::default();
        w.open_with(&state);
        w.working_mut().unwrap().verbose = true;

        w.ok(&mut state);
        assert!(state.verbose, "OK committed the change");
        assert!(!w.is_open(), "OK closed the window");
        assert!(w.working().is_none(), "OK cleared the working copy");
    }

    #[test]
    fn cancel_discards_working_and_closes_without_touching_host() {
        let mut w = PrefsWindow::<TestSettings>::new();
        let state = TestSettings {
            volume: 10,
            ..Default::default()
        };
        w.open_with(&state);
        w.working_mut().unwrap().volume = 99;
        assert!(w.is_dirty());

        w.cancel();
        assert_eq!(state.volume, 10, "Cancel must not mutate host state");
        assert!(!w.is_open());
        assert!(w.working().is_none());
        // After close + re-open, dirty is false again (working == last_applied).
        w.open_with(&state);
        assert!(!w.is_dirty());
    }

    #[test]
    fn working_when_closed_returns_none() {
        let mut w = PrefsWindow::<TestSettings>::new();
        assert!(w.working_mut().is_none());
    }

    #[test]
    fn sidebar_mode_defaults_categorized_and_is_settable() {
        let w = PrefsWindow::<TestSettings>::new();
        assert_eq!(w.sidebar_mode(), SidebarMode::Categorized);
        let w = w.with_sidebar_mode(SidebarMode::Flat);
        assert_eq!(w.sidebar_mode(), SidebarMode::Flat);
        let mut w = w;
        w.set_sidebar_mode(SidebarMode::Categorized);
        assert_eq!(w.sidebar_mode(), SidebarMode::Categorized);
    }

    #[test]
    fn search_text_clears_on_close() {
        let mut w = PrefsWindow::<TestSettings>::new();
        let s = TestSettings::default();
        w.open_with(&s);
        w.search_text.push_str("audio");
        w.close();
        assert!(w.search_text.is_empty());
    }
}

// ============================================================================
// In-dock preferences shell (the non-modal, live-editing paradigm)
// ============================================================================

/// An alternative to the modal [`PrefsWindow`]: an **in-dock, live-editing**
/// preferences shell — a left resizable category sidebar + a right scrolling
/// content pane, the playa / vfx-view style.
///
/// Where [`PrefsWindow`] is a modal `Window` with an Apply/OK/Cancel working copy
/// over ONE unified `S: Clone + PartialEq`, this shell edits settings **in place**
/// and suits the opposite case: settings scattered across live subsystems that
/// can't be cloned into one type, shown as a docked tab with immediate feedback.
/// It is pure chrome — the caller renders each category's rows itself (via any
/// widget: `egui-attr-grid`, plain controls, …), so this module has NO opinion on
/// the settings types. Harvested + decoupled from colmap-rs's `panels/preferences.rs`.
pub mod dock {
    // The category sidebar entry, dim section header, and accent-bar row are SHARED with
    // egui-prefs2 (ONE visual SSOT). Re-exported so `egui_prefs::dock::{Category, section_header,
    // sidebar_item}` keeps working and stays pixel-identical to the egui-prefs2 panel.
    use egui::Ui;
    pub use egui_prefs2::{Category, section_header, sidebar_item};

    /// Draw the in-dock preferences shell: a resizable category sidebar on the left
    /// (its width persists by `id_salt`) and a scrolling content pane on the right.
    /// The caller owns `selected` (the active category index — persist it) and
    /// renders that category's rows in `content(content_ui, selected_index)`.
    /// `sidebar_font` sizes the sidebar rows (wire it to the host's "settings tabs"
    /// font pref). When `reset_label` is `Some`, a right-aligned footer button with
    /// that label is shown; this returns `true` on the frame it is clicked so the
    /// caller can reset the current category.
    pub fn show(
        ui: &mut Ui,
        id_salt: &str,
        categories: &[Category<'_>],
        selected: &mut usize,
        sidebar_font: f32,
        reset_label: Option<&str>,
        content: impl FnOnce(&mut Ui, usize),
    ) -> bool {
        if *selected >= categories.len() {
            *selected = 0;
        }
        egui::Panel::left(egui::Id::new(id_salt).with("egui_prefs_dock_sidebar"))
            .resizable(true)
            .default_size(160.0)
            .size_range(90.0..=400.0)
            .show(ui, |ui| {
                ui.add_space(4.0);
                for (i, cat) in categories.iter().enumerate() {
                    if sidebar_item(ui, *selected == i, cat.icon, cat.label, sidebar_font).clicked()
                    {
                        *selected = i;
                    }
                }
            });

        let mut reset_clicked = false;
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(2.0);
                    content(ui, *selected);
                    if let Some(label) = reset_label {
                        ui.add_space(8.0);
                        ui.separator();
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            reset_clicked = ui.button(label).clicked();
                        });
                    }
                });
        });
        reset_clicked
    }
}
