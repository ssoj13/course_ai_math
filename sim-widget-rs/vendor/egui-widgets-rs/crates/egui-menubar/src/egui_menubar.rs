//! A declarative, app-agnostic menu bar and context menu for egui.
//!
//! # The idea: the menu is DATA, not code
//!
//! A menu is a `&'static [MenuNode]` tree whose leaves name actions by an opaque
//! `&'static str` id. This crate never learns what an action *does*: it draws the tree, asks
//! the host how each item should currently look, and returns the id that was clicked.
//!
//! ```no_run
//! use egui_menubar::{MenuBar, MenuNode::*, ItemState};
//!
//! const MENU: &[egui_menubar::MenuNode] = &[
//!     Sub("File", &[Item("project.open"), Sep, Item("app.preferences"), Sep, Item("app.exit")]),
//!     Sub("Add",  &[Item("comp.add"), Item("media.import")]),
//! ];
//!
//! # fn demo(ui: &mut egui::Ui) {
//! if let Some(id) = MenuBar::new(MENU).show(ui, |id| ItemState::from_label(match id {
//!     "project.open" => "Open Project...",
//!     _ => id,
//! })) {
//!     // dispatch `id` through the host's own action registry
//! }
//! # }
//! ```
//!
//! # Why this shape
//!
//! Hard-coding a menu inline is the usual approach and it rots the same way every time: the
//! menu grows its own private list of what the app can do, and that list drifts from the
//! keyboard shortcuts. Naming actions by id forces the host to keep ONE registry that the
//! menu, the hotkey table and any command palette all read - so an action cannot exist in one
//! and be missing from the other, and the shortcut a menu prints is by construction the
//! shortcut that actually fires.
//!
//! # What the host owns
//!
//! Everything that depends on app state, supplied per-item by the `state` callback:
//! label text, shortcut text, whether the item is enabled, and whether it shows a checkmark.
//! Resolving these at draw time (rather than storing them in the tree) is what lets a `&'static`
//! tree describe a menu whose items grey out, toggle and re-label as the app runs.

use egui::{Response, Ui};

/// One node of a menu tree.
///
/// Deliberately tiny: a submenu, an action leaf, a separator, or a slot the host draws itself.
/// Per-item looks (label, shortcut, enabled, checkmark) belong in the host's [`ItemState`];
/// content a static tree cannot hold (a recent-files list, a slider, a radio group) goes in a
/// [`MenuNode::Dyn`] slot - a menu tree that can express everything is a menu tree nobody can
/// read.
#[derive(Clone, Copy, Debug)]
pub enum MenuNode {
    /// A submenu: a label and its children. Nests to any depth.
    Sub(&'static str, &'static [MenuNode]),
    /// An action leaf, named by the host's action id.
    Item(&'static str),
    /// A horizontal rule. Leading, trailing and doubled separators are dropped at draw time,
    /// so a menu built by concatenating optional groups never shows a stray line.
    Sep,
    /// A slot whose content the host draws, named by the host's id ([`MenuBar::show_full`]'s
    /// `dynamic` callback): what changes as the app runs and is not one action - the list of
    /// recent files, a slider, a radio group. It stays IN the tree, so it sits in the right
    /// submenu at the right place instead of in a separate top-level menu.
    Dyn(&'static str),
}

/// How one item should look right now, resolved by the host per draw.
#[derive(Clone, Debug)]
pub struct ItemState {
    /// Text to display. The id is a stable key, not UI copy - this is what the user reads.
    pub label: String,
    /// Shortcut text shown right-aligned (e.g. `"Ctrl+S"`). Empty for none.
    ///
    /// Purely informational: this crate never binds keys. Feed it from the same registry the
    /// hotkey table is built from, or the menu will confidently print a lie.
    pub shortcut: String,
    /// Greyed out and unclickable when false.
    pub enabled: bool,
    /// Shows a checkmark - for toggles (panel visibility, loop on/off).
    pub checked: bool,
}

impl ItemState {
    /// The common case: an enabled, unchecked item with no shortcut.
    pub fn from_label(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            shortcut: String::new(),
            enabled: true,
            checked: false,
        }
    }

    /// Builder: attach shortcut text.
    pub fn shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = shortcut.into();
        self
    }

    /// Builder: set the enabled state.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Builder: set the checkmark.
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }
}

impl Default for ItemState {
    /// An enabled, unchecked, unlabelled item. `enabled` defaults to TRUE so a host that
    /// forgets to fill this in gets a usable menu rather than a dead one.
    fn default() -> Self {
        Self {
            label: String::new(),
            shortcut: String::new(),
            enabled: true,
            checked: false,
        }
    }
}

/// Draws a [`MenuNode`] tree.
///
/// Holds no state between frames - the tree is `&'static` and everything mutable is resolved
/// through the host's `state` callback, so there is nothing to keep.
#[derive(Clone, Copy, Debug)]
pub struct MenuBar {
    nodes: &'static [MenuNode],
}

impl MenuBar {
    /// Wrap a menu tree.
    pub fn new(nodes: &'static [MenuNode]) -> Self {
        Self { nodes }
    }

    /// Draw as a horizontal menu bar; returns the id clicked this frame, if any.
    ///
    /// `state` is called for every VISIBLE item (i.e. only inside an open submenu), so it may
    /// do real work - query the project, check a selection - without costing anything while the
    /// menus are closed.
    pub fn show(
        &self,
        ui: &mut Ui,
        state: impl FnMut(&'static str) -> ItemState,
    ) -> Option<&'static str> {
        self.show_with_extra(ui, state, |_| {})
    }

    /// [`Self::show`], plus host content drawn INSIDE the bar row after the static menus —
    /// a dynamic submenu (e.g. scripts read from disk) can sit beside the const table
    /// without a crate release. `extra` runs inside the same `egui::MenuBar` container,
    /// so its buttons share the row instead of wrapping to a second one.
    pub fn show_with_extra(
        &self,
        ui: &mut Ui,
        state: impl FnMut(&'static str) -> ItemState,
        extra: impl FnOnce(&mut Ui),
    ) -> Option<&'static str> {
        self.show_full(ui, state, missing_dyn, extra)
    }

    /// [`Self::show_with_extra`], plus the host's drawing of every [`MenuNode::Dyn`] slot:
    /// `dynamic(id, ui)` runs inside the open submenu, where the slot sits. What it does with a
    /// click (open a recent file, set a value) is the host's; the returned id covers only
    /// [`MenuNode::Item`]s.
    pub fn show_full(
        &self,
        ui: &mut Ui,
        mut state: impl FnMut(&'static str) -> ItemState,
        mut dynamic: impl FnMut(&'static str, &mut Ui),
        extra: impl FnOnce(&mut Ui),
    ) -> Option<&'static str> {
        let mut clicked = None;
        // Popup ids of the top-level menus, plus which one the cursor is over. Collected while
        // drawing and acted on afterwards - see `follow_hover_across_menus`.
        let mut tops: Vec<egui::Id> = Vec::new();
        let mut hovered_top: Option<egui::Id> = None;

        egui::MenuBar::new().ui(ui, |ui| {
            for node in self.nodes {
                match node {
                    MenuNode::Sub(label, children) => {
                        let (button, _) =
                            egui::containers::menu::MenuButton::new(*label).ui(ui, |ui| {
                                draw_children(ui, children, &mut state, &mut dynamic, &mut clicked);
                            });
                        let popup_id = egui::Popup::default_response_id(&button);
                        tops.push(popup_id);
                        // `contains_pointer`, not `hovered`: egui reports `hovered() == false`
                        // for every widget while another one holds the interaction, which is
                        // exactly the state we are in with a menu open.
                        if button.contains_pointer() {
                            hovered_top = Some(popup_id);
                        }
                    }
                    // A bare Item at bar level is a button, not a menu - handy for a
                    // one-click action sitting beside the menus.
                    MenuNode::Item(id) => {
                        if draw_item(ui, id, &mut state).clicked() {
                            clicked = Some(*id);
                            ui.close();
                        }
                    }
                    MenuNode::Sep => {
                        ui.separator();
                    }
                    MenuNode::Dyn(id) => dynamic(id, ui),
                }
            }
            extra(ui);
        });

        follow_hover_across_menus(ui.ctx(), &tops, hovered_top);
        clicked
    }

    /// Draw the same tree as a CONTEXT menu body (inside `response.context_menu(..)`).
    ///
    /// Same tree, same ids, same host callback - a right-click menu and the bar cannot disagree
    /// about what an action is called or whether it is available. [`MenuNode::Dyn`] slots show
    /// the missing-slot marker ([`Self::show_full`] is the bar's way to fill them).
    pub fn show_context(
        &self,
        ui: &mut Ui,
        mut state: impl FnMut(&'static str) -> ItemState,
    ) -> Option<&'static str> {
        let mut clicked = None;
        draw_children(ui, self.nodes, &mut state, &mut missing_dyn, &mut clicked);
        clicked
    }
}

/// The drawing of a [`MenuNode::Dyn`] slot nobody fills: a disabled `?id` row, so a tree shown
/// through a method without a `dynamic` callback says what is missing instead of silently
/// dropping part of the menu.
fn missing_dyn(id: &'static str, ui: &mut Ui) {
    ui.add_enabled(false, egui::Button::new(format!("?{id}")));
}

/// Once ONE menu in the bar is open, sliding the cursor onto another top-level menu switches to
/// it — the behaviour every desktop menu bar has had for thirty years.
///
/// egui does not do this on its own: `Popup::menu` opens on CLICK only
/// (`open_memory(clicked -> Toggle)`), and `MenuConfig::bar` merely changes how a menu CLOSES.
/// So the bar has to drive it: while any of our own menus is open, an un-open sibling under the
/// pointer gets opened. `Popup::open_id` closes every other popup as it opens this one, so no
/// explicit close of the previous menu is needed.
///
/// Two things this deliberately does NOT do:
/// - it never OPENS the first menu on hover (that would make the bar grab the pointer as you
///   sweep past it) — a click still starts the interaction;
/// - it only considers ids from THIS bar, so an unrelated popup elsewhere in the app being open
///   does not turn the bar into a hover trap.
fn follow_hover_across_menus(ctx: &egui::Context, tops: &[egui::Id], hovered: Option<egui::Id>) {
    let Some(hovered) = hovered else { return };
    if egui::Popup::is_id_open(ctx, hovered) {
        return; // already the open one - nothing to switch to
    }
    if tops.iter().any(|id| egui::Popup::is_id_open(ctx, *id)) {
        egui::Popup::open_id(ctx, hovered);
        // The switch is decided after this frame's menus were drawn, so ask for one more frame;
        // without it the new menu appears only when some other input happens to repaint.
        ctx.request_repaint();
    }
}

/// Draw a submenu's children, recursing into nested submenus.
///
/// Separator hygiene happens here: a `Sep` is only emitted once something has been drawn since
/// the last one, and a trailing `Sep` is dropped. That is what lets a host concatenate optional
/// groups into one tree without hand-managing the rules.
fn draw_children(
    ui: &mut Ui,
    nodes: &'static [MenuNode],
    state: &mut impl FnMut(&'static str) -> ItemState,
    dynamic: &mut impl FnMut(&'static str, &mut Ui),
    clicked: &mut Option<&'static str>,
) {
    let mut pending_sep = false;
    let mut drawn_any = false;

    for node in nodes {
        match node {
            MenuNode::Sep => pending_sep = drawn_any,
            MenuNode::Sub(label, children) => {
                if pending_sep {
                    ui.separator();
                    pending_sep = false;
                }
                ui.menu_button(*label, |ui| {
                    draw_children(ui, children, state, dynamic, clicked);
                });
                drawn_any = true;
            }
            MenuNode::Dyn(id) => {
                if pending_sep {
                    ui.separator();
                    pending_sep = false;
                }
                dynamic(id, ui);
                drawn_any = true;
            }
            MenuNode::Item(id) => {
                if pending_sep {
                    ui.separator();
                    pending_sep = false;
                }
                if draw_item(ui, id, state).clicked() {
                    *clicked = Some(*id);
                    ui.close();
                }
                drawn_any = true;
            }
        }
    }
}

/// Draw one action leaf and hand back its `Response`.
///
/// The shortcut is drawn by egui's own right-aligned shortcut slot, so it lines up with the
/// rest of the UI instead of being faked with padding.
fn draw_item(
    ui: &mut Ui,
    id: &'static str,
    state: &mut impl FnMut(&'static str) -> ItemState,
) -> Response {
    let st = state(id);
    let button = egui::Button::new(&st.label)
        .shortcut_text(&st.shortcut)
        .selected(st.checked);
    ui.add_enabled(st.enabled, button)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NESTED: &[MenuNode] = &[
        MenuNode::Sub(
            "File",
            &[
                MenuNode::Item("project.open"),
                MenuNode::Sep,
                MenuNode::Item("app.exit"),
            ],
        ),
        MenuNode::Item("help.about"),
    ];

    /// Walk the tree and collect every action id, in order. This is the operation a host needs
    /// to verify its menu against its action registry (every menu id must name a real action),
    /// so it is exercised here to keep the tree walkable in `const` context.
    fn ids(nodes: &'static [MenuNode], out: &mut Vec<&'static str>) {
        for n in nodes {
            match n {
                MenuNode::Item(id) => out.push(id),
                MenuNode::Sub(_, children) => ids(children, out),
                MenuNode::Sep | MenuNode::Dyn(_) => {}
            }
        }
    }

    #[test]
    fn every_leaf_is_reachable_by_a_walk() {
        let mut found = Vec::new();
        ids(NESTED, &mut found);
        assert_eq!(found, vec!["project.open", "app.exit", "help.about"]);
    }

    #[test]
    fn item_state_defaults_to_enabled() {
        // A host that fills in only the label must still get a CLICKABLE item; defaulting
        // `enabled` to false would silently produce a dead menu.
        assert!(ItemState::default().enabled);
        assert!(ItemState::from_label("Open").enabled);
        assert_eq!(ItemState::from_label("Open").shortcut, "");
    }

    #[test]
    fn builders_compose() {
        let st = ItemState::from_label("Loop")
            .shortcut("`")
            .checked(true)
            .enabled(false);
        assert_eq!(st.label, "Loop");
        assert_eq!(st.shortcut, "`");
        assert!(st.checked);
        assert!(!st.enabled);
    }
}
