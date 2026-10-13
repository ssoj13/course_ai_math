//! Core model + renderer for the jobs table.
//!
//! Split out from the crate root so the public re-exports stay in one place.
//! Everything here is generic: a [`JobRow`] is just an id, a status, an
//! optional progress fraction, and a vector of pre-formatted column strings.
//! The host formats its own data (timestamps, sizes, names) into those strings
//! — the widget never interprets them except for substring filtering and
//! lexicographic sorting.

use std::borrow::Cow;

use egui::{Color32, RichText, Ui};
use egui_extras::{Column, TableBuilder};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Lifecycle state of a job, rendered as a coloured status badge.
///
/// Generic across job engines: map your queue's richer states down to one of
/// these five buckets (e.g. "submitting"/"downloading" → [`Running`]).
///
/// [`Running`]: JobStatus::Running
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum JobStatus {
    /// Accepted but not yet started.
    Queued,
    /// Actively executing (this is the state that shows a progress bar).
    Running,
    /// Finished successfully.
    Done,
    /// Finished with an error.
    Failed,
    /// Stopped by the user before completion.
    Cancelled,
}

impl JobStatus {
    /// Terminal = the job will not change state on its own again. Used to gate
    /// which bulk/row actions are offered (only terminal jobs can be removed;
    /// only failed/cancelled jobs can be retried).
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Cancelled)
    }

    /// Human label for the status pill.
    fn label(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Running => "Running",
            Self::Done => "Done",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }

    /// Badge colour for the status pill, resolved from the shared semantic palette
    /// so it tracks the host theme: queued→weak, running→accent, done→ok,
    /// failed→err, cancelled→warn.
    fn badge_color(self, sem: &egui_widgets_config::Semantic) -> Color32 {
        match self {
            Self::Queued => sem.weak,
            Self::Running => sem.accent,
            Self::Done => sem.ok,
            Self::Failed => sem.err,
            Self::Cancelled => sem.warn,
        }
    }
}

/// Which per-row action buttons to offer for a given row.
///
/// The host decides this per row (often derived from the row's
/// [`JobStatus`]): e.g. enable `cancel` only while running, `retry` only when
/// failed. A default ([`RowActions::default`]) offers nothing — opt in
/// explicitly. Convenience constructors derive a sensible set from a status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RowActions {
    /// Show a "Cancel" button (host should stop the running job).
    pub cancel: bool,
    /// Show a "Retry" button (host should re-enqueue the job).
    pub retry: bool,
    /// Show an "Open" button (host should reveal the job's output / result).
    pub open: bool,
    /// Show a "Remove" button (host should drop the job from its queue).
    pub remove: bool,
    /// Show an "Edit" button (host should reopen what the job was submitted with).
    ///
    /// Offered for EVERY status by [`Self::for_status`], including done: what "edit" means is the
    /// host's to decide, and the useful answer for a finished job is usually "load its parameters
    /// so I can change one and run it again" rather than rewriting the record of what was sent.
    pub edit: bool,
}

impl RowActions {
    /// A sensible default action set derived from a status:
    /// - running/queued → cancel + edit
    /// - failed/cancelled → retry + remove + edit
    /// - done → open + remove + edit
    ///
    /// `edit` is offered throughout because it reads the submission, and every job has one.
    pub fn for_status(status: JobStatus) -> Self {
        match status {
            JobStatus::Queued | JobStatus::Running => Self {
                cancel: true,
                edit: true,
                ..Self::default()
            },
            JobStatus::Failed | JobStatus::Cancelled => Self {
                retry: true,
                remove: true,
                edit: true,
                ..Self::default()
            },
            JobStatus::Done => Self {
                open: true,
                remove: true,
                edit: true,
                ..Self::default()
            },
        }
    }
}

/// One row in the table — a single job, fully described by the caller.
///
/// `columns` must line up with the headers passed to [`JobsTable::new`]: the
/// widget renders `columns[i]` under header `i`. Extra/short vectors are
/// tolerated (missing cells render empty), but matched lengths look best.
#[derive(Debug, Clone, PartialEq)]
pub struct JobRow {
    /// Stable identifier echoed back in every [`JobAction`]. The host's key
    /// into its own queue.
    pub id: u64,
    /// Pre-formatted cell text, one entry per header column.
    pub columns: Vec<String>,
    /// Lifecycle state → status badge (and progress-bar gating).
    pub status: JobStatus,
    /// Progress in `0.0..=1.0` for a [`JobStatus::Running`] row. `None` renders
    /// a spinner-less plain "Running…" — providers that don't report a fraction
    /// still get a clean cell.
    pub progress: Option<f32>,
    /// Which of the built-in action buttons to show for this row.
    pub actions: RowActions,
    /// EXTRA buttons this row offers, declared by the host.
    ///
    /// The built-in five are the verbs any job queue has - cancel, retry, open, remove, edit. A
    /// host has others that mean nothing here ("add this result to my project"), and growing the
    /// enum for each would make this widget carry one application's vocabulary. Instead the host
    /// names them and gets the name back in [`JobAction::Custom`].
    ///
    /// Rendered after the built-ins, in the order given, in both the button strip and the
    /// context menu - the same single list, so the two surfaces cannot disagree.
    pub extra: Vec<RowExtra>,
}

/// One host-defined row action. See [`JobRow::extra`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowExtra {
    /// Echoed back in [`JobAction::Custom`]. The host's own key - this widget never reads it.
    pub key: String,
    /// Glyph for the button strip. An empty string falls back to the label's first character, so
    /// a host that has no icon set still gets a button rather than a blank.
    pub icon: String,
    /// Shown in the context menu, and as the button's tooltip.
    pub label: String,
    /// A disabled button still SHOWS, with its tooltip, so the row does not change shape as jobs
    /// move between states - and so the tooltip can say why it is unavailable.
    pub enabled: bool,
    /// Category wash, so a host verb reads the same way as the built-ins beside it. `None` is a
    /// neutral button. See [`Accent`].
    pub hue: Option<Accent>,
}

impl RowExtra {
    pub fn new(key: impl Into<String>, icon: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            icon: icon.into(),
            label: label.into(),
            enabled: true,
            hue: None,
        }
    }

    /// Colour it by what it does, like the built-in verbs. See [`Accent`].
    pub fn accent(mut self, accent: Accent) -> Self {
        self.hue = Some(accent);
        self
    }

    /// Offer it greyed out, with `label` explaining why (it is the tooltip).
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }
}

impl JobRow {
    /// Build a row with action buttons auto-derived from `status`
    /// (see [`RowActions::for_status`]).
    pub fn new(id: u64, columns: Vec<String>, status: JobStatus) -> Self {
        Self {
            id,
            columns,
            status,
            progress: None,
            actions: RowActions::for_status(status),
            extra: Vec::new(),
        }
    }

    /// Add the host's own buttons to this row. See [`JobRow::extra`].
    pub fn with_extra(mut self, extra: Vec<RowExtra>) -> Self {
        self.extra = extra;
        self
    }

    /// Set the progress fraction (builder style).
    pub fn with_progress(mut self, progress: f32) -> Self {
        self.progress = Some(progress);
        self
    }

    /// Override the action set (builder style).
    pub fn with_actions(mut self, actions: RowActions) -> Self {
        self.actions = actions;
        self
    }
}

/// An action the user requested this frame. The host owns the jobs and applies
/// these to its own queue; the widget never mutates job state itself.
///
/// Not `Copy` since [`JobAction::Custom`] carries the host's own key, which is a `String` because
/// the host names its actions and this widget must not have a list of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobAction {
    /// "Cancel" clicked — host should stop the running job `id`.
    Cancel { id: u64 },
    /// "Retry" clicked — host should re-enqueue job `id`.
    Retry { id: u64 },
    /// "Open" clicked — host should reveal job `id`'s output / result.
    Open { id: u64 },
    /// "Remove" clicked — host should drop job `id` from its queue.
    Remove { id: u64 },
    /// "Edit" clicked — host should reopen what job `id` was submitted with.
    Edit { id: u64 },
    /// A host-declared button ([`RowExtra`]) was clicked on job `id`. `key` is the host's own,
    /// echoed back untouched.
    Custom { id: u64, key: String },
    /// The row checkbox toggled. Selection lives in [`JobsTableState`] already;
    /// this is emitted as well so a host that mirrors selection elsewhere can
    /// react. `selected` is the new state.
    Select { id: u64, selected: bool },
}

/// Column the table is sorted by — an index into the caller's headers, plus a
/// special [`SortKey::Status`] for the status badge column. Sorting compares
/// the pre-formatted column strings lexicographically (case-insensitive), which
/// matches how the host already formatted them; for numeric columns format with
/// zero-padding if you need numeric order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum SortKey {
    /// Sort by the data column at this header index.
    Column(usize),
    /// Sort by status (badge) order.
    Status,
}

/// Persistable view state: sort, filter, and selection. Owned by the host so it
/// survives across frames (and, with the `serde` feature, across runs).
///
/// Selection is kept as a sorted `Vec<u64>` rather than a `HashSet` so the
/// whole struct is trivially `serde`-friendly without extra bounds.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct JobsTableState {
    /// Active sort key, or `None` for "as supplied" order.
    pub sort: Option<SortKey>,
    /// `true` = descending. Re-clicking the active header flips this.
    pub sort_descending: bool,
    /// Case-insensitive substring matched against every column cell + the id.
    pub filter_text: String,
    /// When set, only rows with this exact status are shown.
    pub status_filter: Option<JobStatus>,
    /// Ids of selected rows (kept sorted + deduped).
    pub selected: Vec<u64>,
}

impl JobsTableState {
    /// Fresh state sorted by the first column, descending (newest-first when
    /// column 0 is a timestamp — the common case).
    pub fn new() -> Self {
        Self {
            sort: Some(SortKey::Column(0)),
            sort_descending: true,
            ..Self::default()
        }
    }

    /// Is `id` currently selected?
    pub fn is_selected(&self, id: u64) -> bool {
        self.selected.binary_search(&id).is_ok()
    }

    /// Toggle selection for `id`, returning the new state.
    fn toggle_selected(&mut self, id: u64) -> bool {
        match self.selected.binary_search(&id) {
            Ok(pos) => {
                self.selected.remove(pos);
                false
            }
            Err(pos) => {
                self.selected.insert(pos, id);
                true
            }
        }
    }

    /// Apply a header click: flip direction if already sorting by `key`,
    /// otherwise switch to `key` descending.
    fn toggle_sort(&mut self, key: SortKey) {
        if self.sort == Some(key) {
            self.sort_descending = !self.sort_descending;
        } else {
            self.sort = Some(key);
            self.sort_descending = true;
        }
    }

    /// Phosphor caret appended to the active sort column's header (descending =
    /// down, ascending = up). Empty for inactive columns. Returns an owned
    /// `String` so the leading space can prefix the icon-font glyph.
    fn sort_arrow(&self, key: SortKey) -> String {
        if self.sort == Some(key) {
            let caret = if self.sort_descending {
                egui_widgets_config::icons::CARET_DOWN
            } else {
                egui_widgets_config::icons::CARET_UP
            };
            format!(" {caret}")
        } else {
            String::new()
        }
    }
}

/// The jobs-table widget. Construct per frame with the column headers, then
/// [`show`](JobsTable::show) it against the caller's rows + state.
///
/// Cheap to build — it just holds the headers and a couple of layout knobs —
/// so constructing it inline each frame is fine.
pub struct JobsTable {
    headers: Vec<String>,
    show_toolbar: bool,
    row_height: f32,
}

impl JobsTable {
    /// New table for the given column headers (left-to-right). The status badge
    /// and the per-row action buttons get their own columns automatically — do
    /// not include them in `headers`.
    pub fn new(headers: Vec<String>) -> Self {
        Self {
            headers,
            show_toolbar: true,
            row_height: 20.0,
        }
    }

    /// Hide the filter toolbar (search box + status dropdown). Off by default.
    pub fn without_toolbar(mut self) -> Self {
        self.show_toolbar = false;
        self
    }

    /// Override the per-row pixel height (default 20.0).
    pub fn row_height(mut self, height: f32) -> Self {
        self.row_height = height;
        self
    }

    /// Render the table and return the actions the user requested this frame.
    ///
    /// `rows` is borrowed immutably; the widget mutates only `state` (sort,
    /// filter, selection). The returned `Vec` is empty on a quiet frame.
    pub fn show(self, ui: &mut Ui, rows: &[JobRow], state: &mut JobsTableState) -> Vec<JobAction> {
        let mut actions = Vec::new();

        if self.show_toolbar {
            self.toolbar(ui, state);
            ui.separator();
        }

        // Filter, then sort, into a vector of references — we never clone rows.
        let mut view: Vec<&JobRow> = rows
            .iter()
            .filter(|r| row_matches(r, &state.filter_text, state.status_filter))
            .collect();
        sort_view(&mut view, state);

        if view.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.weak(if rows.is_empty() {
                    "No jobs."
                } else {
                    "No jobs match the current filter."
                });
            });
            return actions;
        }

        self.render_table(ui, &view, state, &mut actions);
        actions
    }

    /// Filter toolbar: free-text search + status dropdown.
    fn toolbar(&self, ui: &mut Ui, state: &mut JobsTableState) {
        ui.horizontal(|ui| {
            ui.label(egui_widgets_config::icons::SEARCH);
            ui.text_edit_singleline(&mut state.filter_text);

            egui::ComboBox::from_id_salt("jobs_status_filter")
                .selected_text(match state.status_filter {
                    None => "All".to_string(),
                    Some(s) => s.label().to_string(),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut state.status_filter, None, "All");
                    for s in [
                        JobStatus::Queued,
                        JobStatus::Running,
                        JobStatus::Done,
                        JobStatus::Failed,
                        JobStatus::Cancelled,
                    ] {
                        ui.selectable_value(&mut state.status_filter, Some(s), s.label());
                    }
                });

            let has_filter = !state.filter_text.is_empty() || state.status_filter.is_some();
            if has_filter && ui.button("Clear").clicked() {
                state.filter_text.clear();
                state.status_filter = None;
            }
        });
    }

    /// Build + populate the `egui_extras` table.
    fn render_table(
        &self,
        ui: &mut Ui,
        view: &[&JobRow],
        state: &mut JobsTableState,
        actions: &mut Vec<JobAction>,
    ) {
        let sem = egui_widgets_config::semantic(ui);
        let mut builder = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .column(Column::exact(22.0)); // selection checkbox

        // One auto column per caller header…
        for _ in &self.headers {
            builder = builder.column(Column::auto().at_least(60.0));
        }
        // …then status badge, then a remainder column for progress + actions.
        builder = builder
            .column(Column::auto().at_least(90.0)) // status
            .column(Column::remainder().at_least(160.0)); // progress + actions

        builder
            .header(self.row_height, |mut header| {
                header.col(|ui| {
                    ui.label("");
                });
                for (i, title) in self.headers.iter().enumerate() {
                    header.col(|ui| {
                        let key = SortKey::Column(i);
                        let label = format!("{title}{}", state.sort_arrow(key));
                        if ui.button(label).clicked() {
                            state.toggle_sort(key);
                        }
                    });
                }
                header.col(|ui| {
                    let label = format!("Status{}", state.sort_arrow(SortKey::Status));
                    if ui.button(label).clicked() {
                        state.toggle_sort(SortKey::Status);
                    }
                });
                header.col(|ui| {
                    // Progress bar + action icons — no fixed title (same as the
                    // checkbox column above). "Detail" here collided with the
                    // caller's own "Detail" data column (error/progress message).
                    ui.label("");
                });
            })
            .body(|mut body| {
                for row in view {
                    body.row(self.row_height - 2.0, |mut tr| {
                        // Selection checkbox.
                        tr.col(|ui| {
                            let mut sel = state.is_selected(row.id);
                            if ui.checkbox(&mut sel, "").changed() {
                                let now = state.toggle_selected(row.id);
                                actions.push(JobAction::Select {
                                    id: row.id,
                                    selected: now,
                                });
                            }
                        });

                        // Data columns (missing cells render blank). Each cell is a
                        // list-style item: no text I-beam, click toggles row select.
                        for i in 0..self.headers.len() {
                            tr.col(|ui| {
                                let cell = row.columns.get(i).map(|s| s.as_str()).unwrap_or("");
                                let selected = state.is_selected(row.id);
                                let resp = ui
                                    .selectable_label(selected, cell)
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                if resp.clicked() {
                                    let now = state.toggle_selected(row.id);
                                    actions.push(JobAction::Select {
                                        id: row.id,
                                        selected: now,
                                    });
                                }
                            });
                        }

                        // Status badge — same item hit-target as data cells.
                        tr.col(|ui| {
                            let color = row.status.badge_color(&sem);
                            let selected = state.is_selected(row.id);
                            let label = RichText::new(row.status.label()).color(color).strong();
                            let resp = ui
                                .selectable_label(selected, label)
                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                            if resp.clicked() {
                                let now = state.toggle_selected(row.id);
                                actions.push(JobAction::Select {
                                    id: row.id,
                                    selected: now,
                                });
                            }
                        });

                        // Progress bar (when running) + compact icon action buttons.
                        tr.col(|ui| {
                            ui.horizontal(|ui| {
                                row_progress(ui, row);
                                row_buttons(ui, row, actions);
                            });
                        });

                        // Right-click anywhere on the row: the same actions, spelled
                        // out — a single-row shortcut that skips the checkbox. Reads
                        // `row_action_items`, the same list the icon buttons render,
                        // so the two surfaces cannot disagree about what a row can do.
                        row_context_menu(&tr.response(), row, actions);
                    });
                }
            });
    }
}

/// Render the progress cell for a row: a real bar when running with a known
/// fraction, a plain "Running…" otherwise, and nothing for terminal rows.
fn row_progress(ui: &mut Ui, row: &JobRow) {
    if row.status != JobStatus::Running {
        return;
    }
    match row.progress {
        Some(frac) => {
            ui.add(
                egui::ProgressBar::new(frac.clamp(0.0, 1.0))
                    .desired_width(ui.available_width().min(160.0))
                    .show_percentage(),
            );
        }
        None => {
            ui.weak("Running…");
        }
    }
}

/// One row's available actions as `(icon, label, action)` — the single list
/// both [`row_buttons`] (icons) and [`row_context_menu`] (icon + label) render
/// from, so the two surfaces cannot disagree about what a row can do. Order
/// here is the order both render in.
///
/// **Order is by consequence, and Remove is last.** Verbs that act on the JOB come first
/// (Cancel / Retry / Edit), then verbs that act on its RESULT (Open, plus whatever the host
/// added), and the destructive one sits alone at the right-hand end. Remove used to be in the
/// middle, one pixel from Open, which is a bad place for the only button that loses work.
fn row_action_items(row: &JobRow) -> Vec<RowItem<'_>> {
    let a = row.actions;
    let mut items = Vec::with_capacity(6);
    if a.cancel {
        items.push(RowItem::builtin(
            egui_widgets_config::icons::CLOSE,
            "Cancel",
            JobAction::Cancel { id: row.id },
        ));
    }
    if a.retry {
        items.push(
            RowItem::builtin(
                egui_widgets_config::icons::RESET,
                "Retry",
                JobAction::Retry { id: row.id },
            )
            .hue(Accent::Create),
        );
    }
    if a.edit {
        items.push(RowItem::builtin(
            egui_widgets_config::icons::RENAME,
            "Edit",
            JobAction::Edit { id: row.id },
        ));
    }
    if a.open {
        items.push(
            RowItem::builtin(
                egui_widgets_config::icons::FOLDER_OPEN,
                "Open",
                JobAction::Open { id: row.id },
            )
            .hue(Accent::Import),
        );
    }
    // The host's own, after the built-ins and in the order it gave them.
    for e in &row.extra {
        items.push(RowItem {
            icon: if e.icon.is_empty() {
                Cow::Borrowed(&e.label)
            } else {
                Cow::Borrowed(&e.icon)
            },
            label: Cow::Borrowed(&e.label),
            action: JobAction::Custom {
                id: row.id,
                key: e.key.clone(),
            },
            enabled: e.enabled,
            hue: e.hue,
        });
    }
    // Destructive, and therefore LAST — past the host's extras, at the far right.
    if a.remove {
        items.push(
            RowItem::builtin(
                egui_widgets_config::icons::TRASH,
                "Remove",
                JobAction::Remove { id: row.id },
            )
            .hue(Accent::Destroy),
        );
    }
    items
}

/// What a row action DOES, for the wash that colours its button.
///
/// The same three-way split playa's own toolbars use, so a red button means the same thing in
/// the Jobs table as in the Project bar. Resolved against the live palette at paint time rather
/// than stored as a colour, so it follows the theme.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Accent {
    /// Brings something in, or acts on a result.
    Import,
    /// Produces something new.
    Create,
    /// Removes. The only one that can lose work.
    Destroy,
}

impl Accent {
    fn hue(self, sem: &egui_widgets_config::Semantic) -> egui::Color32 {
        match self {
            Self::Import => sem.accent,
            Self::Create => sem.ok,
            Self::Destroy => sem.err,
        }
    }
}

/// One entry in a row's action list — built-in or host-declared, rendered the same.
///
/// Borrows the row's strings, so a table of many jobs does not allocate a pair of `String`s per
/// row per frame just to draw the same buttons again.
struct RowItem<'a> {
    icon: Cow<'a, str>,
    label: Cow<'a, str>,
    action: JobAction,
    enabled: bool,
    /// Category wash, or `None` for a neutral button.
    hue: Option<Accent>,
}

impl RowItem<'static> {
    fn builtin(icon: &'static str, label: &'static str, action: JobAction) -> Self {
        Self {
            icon: Cow::Borrowed(icon),
            label: Cow::Borrowed(label),
            action,
            enabled: true,
            hue: None,
        }
    }

    fn hue(mut self, accent: Accent) -> Self {
        self.hue = Some(accent);
        self
    }
}

/// Compact icon-only action buttons for the row's trailing column. Text
/// labels ("Cancel"/"Retry"/"Open"/"Remove") made this column fight the
/// progress bar for width; the hover text is what names the button now, same
/// convention as playa's own icon-mode toolbars.
fn row_buttons(ui: &mut Ui, row: &JobRow, actions: &mut Vec<JobAction>) {
    let sem = egui_widgets_config::semantic(ui);
    for item in row_action_items(row) {
        let mut btn = egui::Button::new(item.icon.as_ref()).small();
        // A DISABLED button gets no wash. A red trash can that does nothing when clicked is
        // worse than a grey one: the colour is a promise about what will happen.
        if let Some(accent) = item.hue.filter(|_| item.enabled) {
            btn = btn.fill(egui_widgets_config::accent_wash(
                ui.visuals(),
                accent.hue(&sem),
            ));
        }
        // A disabled button still shows, with its tooltip: the row keeps its shape as jobs move
        // between states, and the tooltip is where "why not" gets said.
        let btn = ui.add_enabled(item.enabled, btn);
        if btn.on_hover_text(item.label.as_ref()).clicked() {
            actions.push(item.action);
        }
    }
}

/// Right-click menu with the same actions as [`row_buttons`], spelled out —
/// a single-row shortcut that does not require checking the row first. A
/// no-op popup is not attached when the row has no actions at all.
fn row_context_menu(response: &egui::Response, row: &JobRow, actions: &mut Vec<JobAction>) {
    let items = row_action_items(row);
    if items.is_empty() {
        return;
    }
    response.context_menu(|ui| {
        for item in items {
            let text = format!("{} {}", item.icon, item.label);
            if ui
                .add_enabled(item.enabled, egui::Button::new(text))
                .clicked()
            {
                actions.push(item.action);
                ui.close();
            }
        }
    });
}

// =============================================================================
// Pure helpers (testable without egui)
// =============================================================================

/// Does a row pass the current filter? Case-insensitive substring over the id
/// and every column cell, plus an optional exact status match.
fn row_matches(row: &JobRow, filter: &str, status: Option<JobStatus>) -> bool {
    if let Some(s) = status
        && row.status != s
    {
        return false;
    }
    let q = filter.trim().to_ascii_lowercase();
    if q.is_empty() {
        return true;
    }
    if row.id.to_string().contains(&q) {
        return true;
    }
    row.columns
        .iter()
        .any(|c| c.to_ascii_lowercase().contains(&q))
}

/// Status badge ordering used when sorting by the status column.
fn status_rank(s: JobStatus) -> u8 {
    match s {
        JobStatus::Queued => 0,
        JobStatus::Running => 1,
        JobStatus::Done => 2,
        JobStatus::Failed => 3,
        JobStatus::Cancelled => 4,
    }
}

/// Sort the filtered view in place according to `state.sort` / direction.
fn sort_view(view: &mut [&JobRow], state: &JobsTableState) {
    let Some(key) = state.sort else {
        return;
    };
    view.sort_by(|a, b| {
        let order = match key {
            SortKey::Status => status_rank(a.status).cmp(&status_rank(b.status)),
            SortKey::Column(i) => {
                let ca = a.columns.get(i).map(|s| s.to_ascii_lowercase());
                let cb = b.columns.get(i).map(|s| s.to_ascii_lowercase());
                ca.cmp(&cb)
            }
        };
        if state.sort_descending {
            order.reverse()
        } else {
            order
        }
    });
}

// =============================================================================
// Tests — pure helpers only; egui rendering is not exercised here.
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: u64, cols: &[&str], status: JobStatus) -> JobRow {
        JobRow::new(id, cols.iter().map(|s| s.to_string()).collect(), status)
    }

    /// A host's own buttons appear after the built-ins, in the order given, and carry the host's
    /// key back untouched. The widget must not need to know what "add.comp" means.
    ///
    /// Remove is the ONE built-in that comes after them - see
    /// [`remove_is_the_last_button_in_the_row`].
    #[test]
    fn host_actions_render_after_the_builtins_and_echo_their_key() {
        let r = row(7, &["a"], JobStatus::Done).with_extra(vec![
            RowExtra::new("add.comp", "C", "Add as comp"),
            RowExtra::new("reveal.refs", "R", "Reveal refs").disabled(),
        ]);
        let items = row_action_items(&r);

        let labels: Vec<String> = items.iter().map(|i| i.label.to_string()).collect();
        let first_extra = labels
            .iter()
            .position(|l| l == "Add as comp")
            .expect("the host's button is offered");
        assert!(
            labels[..first_extra].iter().any(|l| l == "Open"),
            "built-ins come first: {labels:?}"
        );
        assert_eq!(
            labels[first_extra + 1],
            "Reveal refs",
            "and the host's own keep the order it gave"
        );

        assert_eq!(
            items[first_extra].action,
            JobAction::Custom {
                id: 7,
                key: "add.comp".into()
            }
        );
        assert!(items[first_extra].enabled);
        assert!(
            !items[first_extra + 1].enabled,
            "a disabled extra is still OFFERED - the row keeps its shape and the tooltip can say why"
        );
    }

    /// An empty icon falls back to the label, so a host with no icon set gets a button rather
    /// than a blank one it cannot find.
    #[test]
    fn a_host_action_without_an_icon_still_renders_something() {
        let r = row(1, &["a"], JobStatus::Done).with_extra(vec![RowExtra::new(
            "k",
            "",
            "Do the thing",
        )]);
        let items = row_action_items(&r);
        // By label, not `last()`: Remove is what sits last in the row now.
        let it = items
            .iter()
            .find(|i| i.label == "Do the thing")
            .expect("the extra");
        assert_eq!(it.icon.as_ref(), "Do the thing");
    }

    /// The destructive button is last, past the host's own.
    ///
    /// Remove used to sit in the middle of the built-ins, one pixel from Open, which is a bad
    /// place for the only button that can lose work. Asserted rather than left to the reading
    /// order of `row_action_items`, because "where is the trash can" is a UI promise, and adding
    /// the next built-in verb below it would quietly break that promise.
    #[test]
    fn remove_is_the_last_button_in_the_row() {
        let r = row(3, &["a"], JobStatus::Done).with_extra(vec![
            RowExtra::new("play", "P", "Play"),
            RowExtra::new("file", "F", "File"),
        ]);
        let items = row_action_items(&r);
        assert!(
            items.iter().any(|i| i.label == "Remove"),
            "a Done row can be removed"
        );
        assert_eq!(
            items.last().map(|i| i.label.as_ref()),
            Some("Remove"),
            "the destructive verb sits at the far right, after the host's own buttons"
        );
    }

    #[test]
    fn filter_substring_case_insensitive_over_columns() {
        let r = row(1, &["cyberpunk wolf", "done"], JobStatus::Done);
        assert!(row_matches(&r, "WOLF", None));
        assert!(row_matches(&r, "", None));
        assert!(!row_matches(&r, "kitten", None));
    }

    #[test]
    fn filter_matches_id() {
        let r = row(4242, &["x"], JobStatus::Queued);
        assert!(row_matches(&r, "4242", None));
        assert!(row_matches(&r, "424", None));
    }

    #[test]
    fn filter_status_is_exact() {
        let r = row(1, &["x"], JobStatus::Failed);
        assert!(row_matches(&r, "", Some(JobStatus::Failed)));
        assert!(!row_matches(&r, "", Some(JobStatus::Done)));
    }

    #[test]
    fn sort_by_column_descending() {
        let a = row(1, &["alpha"], JobStatus::Queued);
        let b = row(2, &["beta"], JobStatus::Queued);
        let mut view: Vec<&JobRow> = vec![&a, &b];
        let state = JobsTableState {
            sort: Some(SortKey::Column(0)),
            sort_descending: true,
            ..JobsTableState::default()
        };
        sort_view(&mut view, &state);
        assert_eq!(view[0].columns[0], "beta");
        assert_eq!(view[1].columns[0], "alpha");
    }

    #[test]
    fn sort_by_status_rank() {
        let a = row(1, &["x"], JobStatus::Done);
        let b = row(2, &["x"], JobStatus::Queued);
        let mut view: Vec<&JobRow> = vec![&a, &b];
        let state = JobsTableState {
            sort: Some(SortKey::Status),
            sort_descending: false,
            ..JobsTableState::default()
        };
        sort_view(&mut view, &state);
        assert_eq!(view[0].status, JobStatus::Queued);
        assert_eq!(view[1].status, JobStatus::Done);
    }

    #[test]
    fn toggle_sort_flips_then_switches() {
        let mut s = JobsTableState::new();
        assert_eq!(s.sort, Some(SortKey::Column(0)));
        assert!(s.sort_descending);
        s.toggle_sort(SortKey::Column(0)); // same key → flip
        assert!(!s.sort_descending);
        s.toggle_sort(SortKey::Status); // new key → descending
        assert_eq!(s.sort, Some(SortKey::Status));
        assert!(s.sort_descending);
    }

    #[test]
    fn selection_toggles_and_persists_sorted() {
        let mut s = JobsTableState::default();
        assert!(s.toggle_selected(5));
        assert!(s.toggle_selected(2));
        assert!(s.is_selected(5));
        assert!(s.is_selected(2));
        assert_eq!(s.selected, vec![2, 5]); // kept sorted
        assert!(!s.toggle_selected(5)); // toggle off
        assert!(!s.is_selected(5));
        assert_eq!(s.selected, vec![2]);
    }

    #[test]
    fn row_actions_for_status_defaults() {
        assert!(RowActions::for_status(JobStatus::Running).cancel);
        let failed = RowActions::for_status(JobStatus::Failed);
        assert!(failed.retry && failed.remove);
        let done = RowActions::for_status(JobStatus::Done);
        assert!(done.open && done.remove && !done.retry);
    }

    #[test]
    fn terminal_states() {
        assert!(JobStatus::Done.is_terminal());
        assert!(JobStatus::Failed.is_terminal());
        assert!(JobStatus::Cancelled.is_terminal());
        assert!(!JobStatus::Queued.is_terminal());
        assert!(!JobStatus::Running.is_terminal());
    }

    #[test]
    fn builder_sets_progress_and_actions() {
        let r = row(1, &["x"], JobStatus::Running)
            .with_progress(0.5)
            .with_actions(RowActions {
                open: true,
                ..RowActions::default()
            });
        assert_eq!(r.progress, Some(0.5));
        assert!(r.actions.open && !r.actions.cancel);
    }
}
