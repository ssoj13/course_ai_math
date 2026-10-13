//! A **Tab menu** for egui: Houdini's layout with Nuke's search ranking, nested submenus.
//!
//! # Model (one source of truth)
//!
//! The host passes a flat list of [`MenuItem`]s every frame: an opaque key, a label, an icon and
//! a group path (`"Settings/Render"`). [`MenuTree::build`] turns it into one tree, shown two ways:
//!
//! - [`TabMenu::show`]: the Tab menu (below).
//! - [`TabMenu::menu_ui`]: the same tree as nested egui menus (an "Add" button, a context menu).
//!
//! # The Tab menu
//!
//! Houdini's layout: a title, then a search field under the cursor, then the first column: the
//! groups (each opens its column to the right when hovered, to any depth) and top-level entries;
//! after a separator `All` (every entry, flat) and the `Favorites` and `History` sections listed
//! in place. Typing goes straight to the search field; the first column stays and the matches
//! appear as a column to its right, the best one lit, Enter takes it.
//!
//! The search ([`score`]) takes the best of Houdini and Nuke: every query word must match the
//! entry, as the start of one of its words ("e p" finds "Edit Poly"), as initials ("dm" finds
//! "DeepMerge": CamelCase starts words), as a substring, or in its group path and keywords
//! (Nuke's "[Color]"). Ranking: the label itself, then word starts in order, initials,
//! substrings, the path; ties go to favorites (Nuke's star), then to the entries taken most
//! often (Nuke's weights), then by name.
//!
//! Keyboard: Up / Down move within a column (a group opens as it is reached), Right / Enter
//! enter a group, Left goes back, Enter takes an entry, Escape closes. The menu is anchored
//! once (moved inside the screen) and never moves while open, so rows never reflow under the
//! cursor; a side column stays unlit until the cursor or the keyboard enters it. A click on an
//! entry's star toggles it as a favorite. History, favorites and usage counts are the host's to
//! save ([`TabMenu::history`], [`TabMenu::favorites`], [`TabMenu::usage`] and their setters).

#![forbid(unsafe_code)]

use std::collections::VecDeque;

use egui::{Align2, Context, Id, Key, Modifiers, Pos2, Sense, Ui, Vec2, vec2};
use egui_phosphor::regular as ph;

/// One entry the host offers.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuItem<K> {
    /// What [`TabMenu::show`] / [`TabMenu::menu_ui`] return when this entry is taken.
    pub key: K,
    pub label: String,
    /// An icon glyph (an `egui_phosphor` constant), or empty.
    pub icon: String,
    /// The groups from the top (`["Settings", "Render"]`); empty: a top-level entry.
    pub path: Vec<String>,
    /// Shown dimmed at the row's right (a shortcut, a type name).
    pub hint: String,
    /// More words the search matches besides the label and the group path.
    pub keywords: String,
}

impl<K> MenuItem<K> {
    pub fn new(key: K, label: impl Into<String>) -> Self {
        Self {
            key,
            label: label.into(),
            icon: String::new(),
            path: Vec::new(),
            hint: String::new(),
            keywords: String::new(),
        }
    }
    /// Place the entry in the group `path`, its levels separated by `/` ("Settings/Render").
    pub fn group(mut self, path: &str) -> Self {
        self.path = path
            .split('/')
            .map(str::trim)
            .filter(|level| !level.is_empty())
            .map(str::to_owned)
            .collect();
        self
    }
    pub fn icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = icon.into();
        self
    }
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = hint.into();
        self
    }
    pub fn keywords(mut self, keywords: impl Into<String>) -> Self {
        self.keywords = keywords.into();
        self
    }
}

/// The groups and entries of one level of the item tree.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuTree {
    /// Named sub-groups.
    pub groups: Vec<(String, MenuTree)>,
    /// This level's own entries: indices into the items.
    pub leaves: Vec<usize>,
}

impl MenuTree {
    /// The tree of `items`: groups and entries by name when `sorted` (Houdini), else in the order
    /// the items first name them.
    pub fn build<K>(items: &[MenuItem<K>], sorted: bool) -> Self {
        let mut root = Self::default();
        for (index, item) in items.iter().enumerate() {
            let mut level = &mut root;
            for name in &item.path {
                level = level.child_or_insert(name);
            }
            level.leaves.push(index);
        }
        if sorted {
            root.sort(items);
        }
        root
    }
    fn child_or_insert(&mut self, name: &str) -> &mut Self {
        let at = match self.groups.iter().position(|(n, _)| n == name) {
            Some(at) => at,
            None => {
                self.groups.push((name.to_owned(), Self::default()));
                self.groups.len() - 1
            }
        };
        &mut self.groups[at].1
    }
    fn sort<K>(&mut self, items: &[MenuItem<K>]) {
        self.groups.sort_by(|a, b| a.0.cmp(&b.0));
        self.leaves
            .sort_by(|&a, &b| items[a].label.cmp(&items[b].label));
        for (_, group) in &mut self.groups {
            group.sort(items);
        }
    }
    /// Every entry below this level, sorted by label (the `All` group).
    fn all<K>(&self, items: &[MenuItem<K>]) -> Vec<usize> {
        fn collect(tree: &MenuTree, out: &mut Vec<usize>) {
            out.extend(&tree.leaves);
            for (_, group) in &tree.groups {
                collect(group, out);
            }
        }
        let mut out = Vec::new();
        collect(self, &mut out);
        out.sort_by(|&a, &b| items[a].label.cmp(&items[b].label));
        out
    }
}

/// The words of `text`, lowercase: split at anything not alphanumeric and where CamelCase
/// starts a word ("DeepMerge" -> "deep", "merge").
fn words(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut prev: Option<char> = None;
    for c in text.chars() {
        if !c.is_alphanumeric() {
            prev = None;
            continue;
        }
        let starts = match prev {
            None => true,
            Some(p) => c.is_uppercase() && p.is_lowercase() || c.is_numeric() != p.is_numeric(),
        };
        if starts {
            out.push(String::new());
        }
        if let Some(word) = out.last_mut() {
            word.extend(c.to_lowercase());
        }
        prev = Some(c);
    }
    out
}

/// How well `query` matches `item` (higher is better), or None. Every query word must match:
/// as the start of a label word (30, and 10 more when in the label's word order), as the
/// initials of consecutive label words (25, "dm" in "Deep Merge"), as a label substring (10),
/// or in a group or keyword word (5). A single letter is only ever a word start (Houdini: "e p"
/// is "Edit Poly", not every name with an e and a p). The whole label scores 1000.
pub fn score<K>(item: &MenuItem<K>, query: &str) -> Option<u32> {
    let query = query.trim().to_lowercase();
    let label = item.label.to_lowercase();
    if query.is_empty() {
        return None;
    }
    if label == query {
        return Some(1000);
    }
    let label_words = words(&item.label);
    let other: Vec<String> = item
        .path
        .iter()
        .flat_map(|level| words(level))
        .chain(words(&item.keywords))
        .collect();
    let initials = |q: &str| {
        let q: Vec<char> = q.chars().collect();
        (0..label_words.len()).any(|start| {
            q.len() <= label_words.len() - start
                && q.iter()
                    .zip(&label_words[start..])
                    .all(|(c, word)| word.starts_with(*c))
        })
    };
    let mut total = 0;
    let mut last: Option<usize> = None;
    for q in query.split_whitespace() {
        let start = label_words
            .iter()
            .enumerate()
            .skip(last.map_or(0, |l| l + 1))
            .find(|(_, word)| word.starts_with(q))
            .or_else(|| {
                label_words
                    .iter()
                    .enumerate()
                    .find(|(_, w)| w.starts_with(q))
            });
        total += if let Some((at, _)) = start {
            let ordered = last.is_none_or(|l| at > l);
            last = Some(at);
            if ordered { 40 } else { 30 }
        } else if q.chars().count() > 1 && initials(q) {
            25
        } else if q.chars().count() > 1 && label.contains(q) {
            10
        } else if other
            .iter()
            .any(|w| w.starts_with(q) || q.chars().count() > 1 && w.contains(q))
        {
            5
        } else {
            return None;
        };
    }
    Some(total)
}

/// One row of a column.
#[derive(Clone, Debug, PartialEq)]
enum Row {
    /// A group of the column's tree (index): it opens a column.
    Group(usize),
    Leaf(usize),
    Separator,
    /// A section title (History, Favorites).
    Header(String),
    /// A dimmed line ("(no entries)").
    Note(String),
}

impl Row {
    fn selectable(&self) -> bool {
        matches!(self, Row::Group(_) | Row::Leaf(_))
    }
}

/// A column of the cascade: the tree its groups open, and its rows.
struct Column<'a> {
    tree: &'a MenuTree,
    rows: Vec<Row>,
}

impl<'a> Column<'a> {
    fn of(tree: &'a MenuTree) -> Self {
        let mut rows: Vec<Row> = (0..tree.groups.len()).map(Row::Group).collect();
        rows.extend(tree.leaves.iter().map(|&i| Row::Leaf(i)));
        Self { tree, rows }
    }
    fn group(&self, row: usize) -> Option<&'a MenuTree> {
        match self.rows.get(row)? {
            Row::Group(g) => Some(&self.tree.groups[*g].1),
            _ => None,
        }
    }
    /// The next selectable row from `row` in direction `step` (+1 / -1), staying put at an end.
    fn step(&self, row: Option<usize>, down: bool) -> Option<usize> {
        let selectable = |r: &usize| self.rows[*r].selectable();
        match (row, down) {
            (None, _) => (0..self.rows.len()).find(selectable),
            (Some(r), true) => (r + 1..self.rows.len()).find(selectable).or(Some(r)),
            (Some(r), false) => (0..r).rev().find(selectable).or(Some(r)),
        }
    }
}

/// The open menu's state.
#[derive(Clone, Debug)]
struct Open {
    /// The top-left corner, fixed once the first frame measured the menu.
    at: Pos2,
    anchored: bool,
    query: String,
    /// The open group's row in each column: column `c + 1` shows `path[c]`'s group.
    path: Vec<usize>,
    /// The lit row: `(column, row)`. While searching, column 1 is the result list.
    lit: Option<(usize, usize)>,
}

/// The Tab menu: its open state, History, favorites, usage counts and options. One per host
/// menu, kept across frames: [`Self::open`] at the cursor, then [`Self::show`] every frame.
#[derive(Clone, Debug)]
pub struct TabMenu<K> {
    id: Id,
    open: Option<Open>,
    history: VecDeque<K>,
    favorites: Vec<K>,
    usage: Vec<(K, u32)>,
    history_len: usize,
    all: bool,
    sorted: bool,
    rows: usize,
    column_width: f32,
    title: String,
}

impl<K: Clone + PartialEq> TabMenu<K> {
    /// A closed menu with Houdini's options: sorted, `All`, a History of 10, "TAB Menu".
    pub fn new(id: impl Into<Id>) -> Self {
        Self {
            id: id.into(),
            open: None,
            history: VecDeque::new(),
            favorites: Vec::new(),
            usage: Vec::new(),
            history_len: 10,
            all: true,
            sorted: true,
            rows: 24,
            column_width: 180.0,
            title: "TAB Menu".to_owned(),
        }
    }
    /// Entries kept in `History` (0: no History section).
    pub fn history_len(mut self, len: usize) -> Self {
        self.history_len = len;
        self.history.truncate(len);
        self
    }
    /// Whether the `All` group is shown.
    pub fn all_group(mut self, all: bool) -> Self {
        self.all = all;
        self
    }
    /// Groups and entries by name (true, Houdini) or in the items' order.
    pub fn sorted(mut self, sorted: bool) -> Self {
        self.sorted = sorted;
        self
    }
    /// Rows a column shows before it scrolls.
    pub fn rows(mut self, rows: usize) -> Self {
        self.rows = rows.max(1);
        self
    }
    pub fn column_width(mut self, width: f32) -> Self {
        self.column_width = width;
        self
    }
    /// The title over the search field (empty: none).
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Open the menu with its search field at `at` (the cursor), empty and nothing lit.
    pub fn open(&mut self, at: Pos2) {
        self.open = Some(Open {
            at,
            anchored: false,
            query: String::new(),
            path: Vec::new(),
            lit: None,
        });
    }
    pub fn close(&mut self) {
        self.open = None;
    }
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }
    /// The History, newest first.
    pub fn history(&self) -> impl Iterator<Item = &K> {
        self.history.iter()
    }
    /// Restore a saved History (newest first).
    pub fn set_history(&mut self, keys: impl IntoIterator<Item = K>) {
        self.history = keys.into_iter().take(self.history_len).collect();
    }
    /// The favorites, in the order they were starred.
    pub fn favorites(&self) -> &[K] {
        &self.favorites
    }
    pub fn set_favorites(&mut self, keys: impl IntoIterator<Item = K>) {
        self.favorites = keys.into_iter().collect();
    }
    /// How often each entry was taken (the search's weights).
    pub fn usage(&self) -> &[(K, u32)] {
        &self.usage
    }
    pub fn set_usage(&mut self, counts: impl IntoIterator<Item = (K, u32)>) {
        self.usage = counts.into_iter().collect();
    }

    /// The item tree the menus show (for a host building its own menu type from it).
    pub fn tree(&self, items: &[MenuItem<K>]) -> MenuTree {
        MenuTree::build(items, self.sorted)
    }

    /// The matches of `query` among `items`, best first ([`score`], then favorites, usage and
    /// name).
    pub fn search(&self, items: &[MenuItem<K>], query: &str) -> Vec<usize> {
        let mut found: Vec<(usize, u32)> = items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| Some((i, score(item, query)?)))
            .collect();
        found.sort_by(|&(a, sa), &(b, sb)| {
            sb.cmp(&sa)
                .then_with(|| {
                    self.favorite(&items[b].key)
                        .cmp(&self.favorite(&items[a].key))
                })
                .then_with(|| self.used(&items[b].key).cmp(&self.used(&items[a].key)))
                .then_with(|| items[a].label.cmp(&items[b].label))
        });
        found.into_iter().map(|(i, _)| i).collect()
    }
    fn favorite(&self, key: &K) -> bool {
        self.favorites.contains(key)
    }
    fn used(&self, key: &K) -> u32 {
        self.usage
            .iter()
            .find(|(k, _)| k == key)
            .map_or(0, |(_, n)| *n)
    }
    fn toggle_favorite(&mut self, key: &K) {
        if let Some(at) = self.favorites.iter().position(|k| k == key) {
            self.favorites.remove(at);
        } else {
            self.favorites.push(key.clone());
        }
    }

    /// The same tree as nested egui menus inside an open menu (an Add button, a context menu):
    /// the key of the entry clicked. Taking it counts as in the Tab menu (History, usage).
    pub fn menu_ui(&mut self, ui: &mut Ui, items: &[MenuItem<K>]) -> Option<K> {
        fn level<K>(ui: &mut Ui, tree: &MenuTree, items: &[MenuItem<K>]) -> Option<usize> {
            let mut chosen = None;
            for (name, group) in &tree.groups {
                ui.menu_button(name, |ui| {
                    if let Some(i) = level(ui, group, items) {
                        chosen = Some(i);
                    }
                });
            }
            for &i in &tree.leaves {
                let item = &items[i];
                if ui
                    .add(egui::Button::new(label(item)).shortcut_text(&item.hint))
                    .clicked()
                {
                    ui.close();
                    chosen = Some(i);
                }
            }
            chosen
        }
        let tree = self.tree(items);
        let chosen = level(ui, &tree, items)?;
        Some(self.take(items, chosen))
    }

    /// Draw the open menu: the key of the entry taken this frame, if any (the menu closes).
    pub fn show(&mut self, ctx: &Context, items: &[MenuItem<K>]) -> Option<K> {
        let mut open = self.open.take()?;
        let mut tree = self.tree(items);
        let real = tree.groups.len();
        if self.all {
            let leaves = tree.all(items);
            tree.groups.push((
                "All".to_owned(),
                MenuTree {
                    groups: Vec::new(),
                    leaves,
                },
            ));
        }
        let index = |key: &K| items.iter().position(|item| item.key == *key);
        let favorites: Vec<usize> = self.favorites.iter().filter_map(index).collect();
        let history: Vec<usize> = self.history.iter().filter_map(index).collect();
        // The first column: the real groups and top-level entries, a separator, the synthetic
        // groups and the Favorites / History sections (Houdini's layout).
        let mut first = Column::of(&tree);
        first
            .rows
            .retain(|row| !matches!(row, Row::Group(g) if *g >= real));
        if tree.groups.len() > real || !favorites.is_empty() || self.history_len > 0 {
            first.rows.push(Row::Separator);
        }
        first.rows.extend((real..tree.groups.len()).map(Row::Group));
        if !favorites.is_empty() {
            first.rows.push(Row::Header("Favorites".to_owned()));
            first.rows.extend(favorites.iter().map(|&i| Row::Leaf(i)));
        }
        if self.history_len > 0 {
            first.rows.push(Row::Header("History".to_owned()));
            if history.is_empty() {
                first.rows.push(Row::Note("(no entries)".to_owned()));
            }
            first.rows.extend(history.iter().map(|&i| Row::Leaf(i)));
        }
        let searching = !open.query.trim().is_empty();
        let mut columns = vec![first];
        if searching {
            open.path.clear();
            columns.push(Column {
                tree: &tree,
                rows: self
                    .search(items, &open.query)
                    .into_iter()
                    .map(Row::Leaf)
                    .collect(),
            });
        } else {
            for &row in &open.path {
                let Some(group) = columns.last().and_then(|c| c.group(row)) else {
                    break;
                };
                columns.push(Column::of(group));
            }
        }

        // Keys drive the menu, not the search field: taken before the field sees them.
        let key = |key: Key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key));
        if key(Key::Escape) {
            return None;
        }
        let (down, up, right, left, enter) = (
            key(Key::ArrowDown),
            key(Key::ArrowUp),
            key(Key::ArrowRight),
            key(Key::ArrowLeft),
            key(Key::Enter),
        );
        let mut chosen = None;
        if searching {
            // The results column: the best match lit until the keyboard moves.
            let results = &columns[1];
            let row = open.lit.filter(|(c, _)| *c == 1).map(|(_, r)| r);
            let row = if down || up {
                results.step(row, down)
            } else {
                row.or_else(|| results.step(None, true))
            };
            open.lit = row.map(|r| (1, r));
            if enter && let Some(Row::Leaf(i)) = row.and_then(|r| results.rows.get(r)) {
                chosen = Some(*i);
            }
        } else if down || up {
            let col = open.lit.map_or(0, |(c, _)| c.min(columns.len() - 1));
            let row = columns[col].step(open.lit.map(|(_, r)| r), down);
            if let Some(row) = row {
                open.lit = Some((col, row));
                open.path.truncate(col);
                if columns[col].group(row).is_some() {
                    open.path.push(row);
                }
            }
        } else if let Some((col, row)) = open.lit {
            if (right || enter) && columns[col].group(row).is_some() {
                let entered = col + 1;
                let next = columns[col]
                    .group(row)
                    .map(Column::of)
                    .and_then(|c| c.step(None, true).map(|r| (r, c.group(r).is_some())));
                if let Some((r, group)) = next {
                    open.lit = Some((entered, r));
                    open.path.truncate(entered);
                    if group {
                        open.path.push(r);
                    }
                }
            } else if left && col > 0 {
                open.lit = Some((col - 1, open.path[col - 1]));
                open.path.truncate(col);
            } else if enter && let Some(Row::Leaf(i)) = columns[col].rows.get(row) {
                chosen = Some(*i);
            }
        }
        if let Some(i) = chosen {
            return Some(self.take(items, i));
        }

        let style = ctx.global_style();
        let row_height = style.spacing.interact_size.y;
        let width = self.column_width;
        let max_height = row_height * self.rows as f32;
        let mut star = None;
        let shown = egui::Area::new(self.id)
            .order(egui::Order::Foreground)
            .fixed_pos(open.at)
            .constrain(false)
            .show(ctx, |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(2.0, 0.0);
                    ui.horizontal_top(|ui| {
                        for (c, column) in columns.iter().enumerate() {
                            ui.vertical(|ui| {
                                ui.set_width(width);
                                if c == 0 {
                                    header(ui, &self.title, &mut open.query, width, row_height);
                                } else {
                                    // Side columns start level with the first column's rows.
                                    ui.add_space(header_height(&self.title, row_height));
                                }
                                let opened = open.path.get(c).copied();
                                let (lit, active) = match open.lit {
                                    Some((lc, row)) if lc == c => (Some(row), true),
                                    _ => (opened, false),
                                };
                                let view = View {
                                    items,
                                    tree: column.tree,
                                    favorites: &self.favorites,
                                    width,
                                    row_height,
                                    max_height,
                                };
                                match column_ui(ui, c, &column.rows, lit, active, &view) {
                                    Some(Pick::Hover(row)) => {
                                        open.lit = Some((c, row));
                                        if !searching {
                                            open.path.truncate(c);
                                            if column.group(row).is_some() {
                                                open.path.push(row);
                                            }
                                        }
                                    }
                                    Some(Pick::Take(i)) => chosen = Some(i),
                                    Some(Pick::Star(i)) => star = Some(i),
                                    None => {}
                                }
                            });
                        }
                    });
                });
            });
        let rect = shown.response.rect;
        if !open.anchored {
            // Anchored once, inside the screen by the first frame's size, then fixed.
            let screen = ctx.content_rect();
            let shift = Vec2::new(
                (screen.right() - rect.right()).min(0.0),
                (screen.bottom() - rect.bottom()).min(0.0),
            );
            open.at = (open.at + shift).max(screen.min);
            open.anchored = true;
        }
        if let Some(i) = star {
            self.toggle_favorite(&items[i].key);
        }
        if let Some(i) = chosen {
            return Some(self.take(items, i));
        }
        let pressed_outside = ctx.input(|i| {
            i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p))
        });
        if !pressed_outside {
            self.open = Some(open);
        }
        None
    }

    /// Take entry `i`: to the front of the History, one more use, the menu closes.
    fn take(&mut self, items: &[MenuItem<K>], i: usize) -> K {
        let key = items[i].key.clone();
        if self.history_len > 0 {
            self.history.retain(|k| *k != key);
            self.history.push_front(key.clone());
            self.history.truncate(self.history_len);
        }
        match self.usage.iter_mut().find(|(k, _)| *k == key) {
            Some((_, n)) => *n = n.saturating_add(1),
            None => self.usage.push((key.clone(), 1)),
        }
        self.open = None;
        key
    }
}

/// The title and search field's height (the first column's head).
fn header_height(title: &str, row_height: f32) -> f32 {
    if title.is_empty() {
        row_height + 4.0
    } else {
        2.0 * row_height + 4.0
    }
}

/// The first column's head: the title, then the search field (always focused, so typing goes
/// straight to it) behind a magnifier.
fn header(ui: &mut Ui, title: &str, query: &mut String, width: f32, row_height: f32) {
    if !title.is_empty() {
        let (rect, _) = ui.allocate_exact_size(vec2(width, row_height), Sense::hover());
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            title,
            egui::TextStyle::Button.resolve(ui.style()),
            ui.visuals().strong_text_color(),
        );
    }
    ui.horizontal(|ui| {
        ui.label(ph::MAGNIFYING_GLASS);
        let edit = ui.add(egui::TextEdit::singleline(query).desired_width(f32::INFINITY));
        edit.request_focus();
    });
    ui.add_space(4.0);
}

/// What the pointer did in a column.
enum Pick {
    Hover(usize),
    Take(usize),
    Star(usize),
}

/// What every column draws with.
struct View<'a, K> {
    items: &'a [MenuItem<K>],
    tree: &'a MenuTree,
    favorites: &'a [K],
    width: f32,
    row_height: f32,
    max_height: f32,
}

/// An entry's label with its icon.
fn label<K>(item: &MenuItem<K>) -> String {
    if item.icon.is_empty() {
        item.label.clone()
    } else {
        format!("{}  {}", item.icon, item.label)
    }
}

/// One column's rows, `lit` highlighted (strongly in the `active` column, the one the cursor or
/// keyboard is in; an open group elsewhere shows weaker), scrolled to keep it in view. An entry
/// shows its hint and, when a favorite or hovered, its star.
fn column_ui<K: PartialEq>(
    ui: &mut Ui,
    column: usize,
    rows: &[Row],
    lit: Option<usize>,
    active: bool,
    view: &View<'_, K>,
) -> Option<Pick> {
    let mut picked = None;
    egui::ScrollArea::vertical()
        .id_salt(("tab_menu_column", column))
        .max_height(view.max_height)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (r, row) in rows.iter().enumerate() {
                let font = egui::TextStyle::Button.resolve(ui.style());
                match row {
                    Row::Separator => {
                        ui.separator();
                        continue;
                    }
                    Row::Header(text) | Row::Note(text) => {
                        let (rect, _) = ui
                            .allocate_exact_size(vec2(view.width, view.row_height), Sense::hover());
                        let header = matches!(row, Row::Header(_));
                        if header {
                            ui.painter()
                                .rect_filled(rect, 0.0, ui.visuals().faint_bg_color);
                        }
                        let (anchor, align) = if header {
                            (rect.center(), Align2::CENTER_CENTER)
                        } else {
                            (rect.left_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER)
                        };
                        ui.painter().text(
                            anchor,
                            align,
                            text,
                            font,
                            ui.visuals().weak_text_color(),
                        );
                        continue;
                    }
                    Row::Group(_) | Row::Leaf(_) => {}
                }
                let (rect, response) =
                    ui.allocate_exact_size(vec2(view.width, view.row_height), Sense::click());
                let visuals = ui.visuals();
                let on = lit == Some(r);
                if on {
                    let fill = if active {
                        visuals.selection.bg_fill
                    } else {
                        visuals.widgets.hovered.weak_bg_fill
                    };
                    ui.painter().rect_filled(rect, 2.0, fill);
                    if active {
                        response.scroll_to_me(None);
                    }
                }
                let color = if on && active {
                    visuals.selection.stroke.color
                } else {
                    visuals.text_color()
                };
                let inner = rect.shrink2(vec2(6.0, 0.0));
                let painter = ui.painter();
                match row {
                    Row::Group(g) => {
                        painter.text(
                            inner.left_center(),
                            Align2::LEFT_CENTER,
                            &view.tree.groups[*g].0,
                            font.clone(),
                            color,
                        );
                        painter.text(
                            inner.right_center(),
                            Align2::RIGHT_CENTER,
                            ph::CARET_RIGHT,
                            font,
                            color,
                        );
                    }
                    Row::Leaf(i) => {
                        let item = &view.items[*i];
                        painter.text(
                            inner.left_center(),
                            Align2::LEFT_CENTER,
                            label(item),
                            font.clone(),
                            color,
                        );
                        // The star (Nuke's favorites): lit when starred, shown on hover.
                        let favorite = view.favorites.contains(&item.key);
                        let star = egui::Rect::from_center_size(
                            inner.right_center() - vec2(view.row_height * 0.5, 0.0),
                            Vec2::splat(view.row_height),
                        );
                        if favorite || response.hovered() {
                            let star_color = if favorite {
                                visuals.warn_fg_color
                            } else {
                                visuals.weak_text_color()
                            };
                            painter.text(
                                star.center(),
                                Align2::CENTER_CENTER,
                                ph::STAR,
                                font.clone(),
                                star_color,
                            );
                        }
                        painter.text(
                            star.left_center() - vec2(4.0, 0.0),
                            Align2::RIGHT_CENTER,
                            &item.hint,
                            font,
                            visuals.weak_text_color(),
                        );
                        if response.clicked() {
                            let on_star = response
                                .interact_pointer_pos()
                                .is_some_and(|p| star.contains(p));
                            picked = Some(if on_star {
                                Pick::Star(*i)
                            } else {
                                Pick::Take(*i)
                            });
                            continue;
                        }
                    }
                    _ => {}
                }
                // Houdini: the row under a moving cursor is lit (a group opens); a still cursor
                // leaves the keyboard's choice alone.
                if response.hovered()
                    && !(active && on)
                    && ui.input(|i| i.pointer.delta() != Vec2::ZERO)
                {
                    picked = Some(Pick::Hover(r));
                }
            }
        });
    picked
}

#[cfg(test)]
mod tests;
