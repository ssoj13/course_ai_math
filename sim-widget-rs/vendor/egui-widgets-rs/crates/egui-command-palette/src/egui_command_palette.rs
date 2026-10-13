//! A searchable **command palette** (Ctrl-P style) for egui 0.34, plus a reusable
//! fuzzy subsequence matcher and a fuzzy-search combo box.
//!
//! # Why this crate exists
//!
//! Almost every mature tool (VS Code, Sublime, the DCCs) has a "press a key, type
//! a few characters, run any command" palette. It is a generic, app-agnostic
//! affordance: a search box over a flat list of named actions, ranked by a fuzzy
//! match. This crate provides exactly that as a hermetic egui widget — no app
//! types, no fuzzy-matcher dependency (the matcher is implemented inline so the
//! crate stays `egui`-only and the scoring is auditable/tweakable).
//!
//! # Contract (immediate-mode)
//!
//! The host owns the list of [`Command`]s and passes them **by reference** every
//! frame. The widget owns only its transient view-state — the query string and the
//! highlighted row index — exactly like `egui-encode-dialog` owns its wizard state.
//! [`CommandPalette::show`] returns a [`CommandPaletteResult`]:
//!
//! - [`Open`](CommandPaletteResult::Open) — keep calling `show()` next frame.
//! - [`Cancelled`](CommandPaletteResult::Cancelled) — user pressed Esc / clicked
//!   the backdrop; the host stops calling `show()` (and may [`reset`](CommandPalette::reset)).
//! - [`Run(id)`](CommandPaletteResult::Run) — user picked a command; the host runs
//!   the command keyed by `id` and stops calling `show()`.
//!
//! ```ignore
//! // host state: `palette: CommandPalette`, `palette_open: bool`.
//! if ctx.input(|i| i.key_pressed(egui::Key::P) && i.modifiers.command) {
//!     palette_open = true;
//!     palette.reset(); // fresh query + selection
//! }
//! if palette_open {
//!     match palette.show(ctx, &commands) {
//!         CommandPaletteResult::Open => {}
//!         CommandPaletteResult::Cancelled => palette_open = false,
//!         CommandPaletteResult::Run(id) => { palette_open = false; run_command(id); }
//!     }
//! }
//! ```
//!
//! # egui 0.34 notes (the bits that bite)
//! - Uses the native [`egui::Modal`] (centered, backdrop-dimmed, Esc/backdrop
//!   close via [`egui::ModalResponse::should_close`]) — same as `egui-encode-dialog`.
//! - Auto-focus: the search [`egui::TextEdit`] returns a [`egui::Response`]; on the
//!   first frame we call `response.request_focus()` so typing starts immediately.
//! - Arrow keys would otherwise move the text cursor; we read Up/Down (and
//!   Enter/Esc) from `ctx.input` and *consume* them so they drive the list instead.

#![forbid(unsafe_code)]

// =============================================================================
// Data model (caller-supplied, read-only to the widget)
// =============================================================================

/// One selectable command shown in the palette.
///
/// Caller-supplied and read-only: the widget never mutates a `Command`. The `id`
/// is the host's opaque key — it is what comes back in
/// [`CommandPaletteResult::Run`], so the host can map it to an action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    /// Stable machine key for this command. Returned on selection; the host
    /// resolves it back to the actual action.
    pub id: u64,
    /// Human-readable label shown in the list and matched against the query.
    pub label: String,
    /// Optional secondary text (e.g. a keyboard shortcut or category) shown
    /// dimmed on the right of the row. Not matched against the query.
    pub hint: Option<String>,
}

impl Command {
    /// Convenience constructor for a command without a hint.
    pub fn new(id: u64, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            hint: None,
        }
    }

    /// Builder: attach a hint (shortcut / category) shown dimmed on the right.
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

// =============================================================================
// Result enum
// =============================================================================

/// Outcome of one [`CommandPalette::show`] call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandPaletteResult {
    /// The palette is still open; keep calling `show()` next frame.
    Open,
    /// The user cancelled (Esc, backdrop click). The host should stop showing it.
    Cancelled,
    /// The user chose a command. Carries the chosen [`Command::id`]. The host
    /// should stop showing the palette and run the command.
    Run(u64),
}

// =============================================================================
// The palette widget
// =============================================================================

/// A searchable, fuzzy-ranked command palette modal.
///
/// Owns only transient view-state (the query and the highlighted index). Default
/// is an empty query with the first row highlighted. Re-use one instance across
/// frames; call [`reset`](Self::reset) when (re)opening to clear stale state.
#[derive(Clone, Debug, Default)]
pub struct CommandPalette {
    /// Current search query (edited by the user via the search field).
    query: String,
    /// Highlighted row index **into the filtered list** (not the source list).
    /// Clamped to the filtered length each frame.
    selected: usize,
    /// `true` until the first `show()` after a `reset()`/construction has run, so
    /// we can request keyboard focus on the search field exactly once on open.
    needs_focus: bool,
}

impl CommandPalette {
    /// Create an empty palette (empty query, first row highlighted).
    pub fn new() -> Self {
        Self {
            query: String::new(),
            selected: 0,
            needs_focus: true,
        }
    }

    /// Clear the query + selection and arm auto-focus. Call this when (re)opening
    /// the palette so it starts fresh and grabs the keyboard.
    pub fn reset(&mut self) {
        self.query.clear();
        self.selected = 0;
        self.needs_focus = true;
    }

    /// Read the current query (e.g. to persist it between opens).
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Show the palette modal for one frame and return the [`CommandPaletteResult`].
    ///
    /// `commands` is borrowed read-only. Filtering + ranking is done here every
    /// frame via [`fuzzy_score`]; an empty query lists all commands in their given
    /// order. Keyboard: Up/Down move the highlight (clamped), Enter runs the
    /// highlighted command, Esc cancels. Clicking a row runs that command.
    pub fn show(&mut self, ctx: &egui::Context, commands: &[Command]) -> CommandPaletteResult {
        // ---- 1. Compute the filtered + ranked view of the commands. ----
        // `ranked` holds indices into `commands`, best match first. An empty query
        // means "show everything in the caller's order" (score is irrelevant).
        let ranked: Vec<usize> = self.ranked_indices(commands);

        // Keep the highlight inside the (possibly shrunken) filtered list.
        if self.selected >= ranked.len() {
            self.selected = ranked.len().saturating_sub(1);
        }

        // ---- 2. Read + consume navigation keys BEFORE drawing the TextEdit. ----
        // Reading them via `input_mut` and `consume_key` stops the arrow keys from
        // also moving the text cursor and stops Esc from being swallowed elsewhere.
        let (go_up, go_down, accept, cancel_key) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });
        if go_up && self.selected > 0 {
            self.selected -= 1;
        }
        if go_down && self.selected + 1 < ranked.len() {
            self.selected += 1;
        }

        let mut result = CommandPaletteResult::Open;

        // Enter on a non-empty list runs the highlighted command.
        if accept && let Some(&idx) = ranked.get(self.selected) {
            result = CommandPaletteResult::Run(commands[idx].id);
        }
        // Esc cancels (in addition to the Modal's own backdrop/Escape handling).
        if cancel_key {
            result = CommandPaletteResult::Cancelled;
        }

        // ---- 3. Draw the modal. ----
        let modal = egui::Modal::new(egui::Id::new("egui_command_palette")).show(ctx, |ui| {
            ui.set_width(420.0);

            // Search field — auto-focused on the first frame after open.
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("Type a command…")
                    .desired_width(f32::INFINITY),
            );
            if self.needs_focus {
                edit.request_focus();
                self.needs_focus = false;
            }

            ui.separator();

            if ranked.is_empty() {
                ui.weak("(no matching commands)");
                return;
            }

            // Scrollable result list. Each row is a full-width selectable label;
            // the highlighted row reflects keyboard navigation.
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for (row, &idx) in ranked.iter().enumerate() {
                        let cmd = &commands[idx];
                        let selected = row == self.selected;

                        // One horizontal row: a full-width selectable label on the
                        // left and (optionally) a dimmed hint pushed to the right.
                        // Wrapping both in `ui.horizontal` keeps them on one line.
                        let resp = ui
                            .horizontal(|ui| {
                                let resp = ui.selectable_label(selected, &cmd.label);
                                if let Some(hint) = &cmd.hint {
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| ui.weak(hint),
                                    );
                                }
                                resp
                            })
                            .inner;

                        if resp.clicked() {
                            result = CommandPaletteResult::Run(cmd.id);
                        }
                        // A hover updates the highlight so click + keyboard agree.
                        if resp.hovered() {
                            self.selected = row;
                        }
                    }
                });
        });

        // Backdrop click / Modal's own Escape → cancel (unless a button already won).
        if result == CommandPaletteResult::Open && modal.should_close() {
            result = CommandPaletteResult::Cancelled;
        }
        result
    }

    /// Build the filtered + ranked list of indices into `commands`.
    ///
    /// Empty query ⇒ every command in caller order. Otherwise keep only commands
    /// whose label fuzzily matches the query, sorted by descending [`fuzzy_score`]
    /// (ties broken by original order so the result is stable).
    fn ranked_indices(&self, commands: &[Command]) -> Vec<usize> {
        let q = self.query.trim();
        if q.is_empty() {
            return (0..commands.len()).collect();
        }
        let mut scored: Vec<(usize, i32)> = commands
            .iter()
            .enumerate()
            .filter_map(|(i, c)| fuzzy_score(q, &c.label).map(|s| (i, s)))
            .collect();
        // Higher score first; equal scores keep their original relative order
        // (sort_by is stable), so the ranking is deterministic.
        scored.sort_by_key(|&(_, s)| std::cmp::Reverse(s));
        scored.into_iter().map(|(i, _)| i).collect()
    }
}

// =============================================================================
// Fuzzy matcher (the reusable core)
// =============================================================================

/// Case-insensitive fuzzy **subsequence** score of `needle` against `haystack`.
///
/// Returns `None` when `needle` is not a subsequence of `haystack` (i.e. not all
/// of its characters appear, in order). Otherwise returns a score where **higher
/// is a better match**. An empty needle matches anything with score `0`.
///
/// # Scoring (simple, documented, tweakable)
/// We walk `haystack` once, consuming `needle` characters in order, and award:
/// - **+16 consecutive bonus** when this match immediately follows the previous
///   match (rewards contiguous runs, so "cmpl" ranks "Compile" over "Copy Model").
/// - **+8 word-start bonus** when the matched char begins a word (start of string
///   or right after a space / `_` / `-` / `/` / `.` separator) — rewards
///   acronym-style hits ("op" → "**O**pen **P**roject").
/// - **−1 per leading gap**: a small penalty for each unmatched haystack char
///   *before the first* matched char, so earlier matches rank higher.
///
/// Every matched char also contributes a flat **+1** so longer (more complete)
/// matches against the same string don't lose to shorter ones for free.
///
/// The absolute numbers don't matter; only their relative size does. They were
/// chosen so "contiguous" beats "word-start" beats "earlier position", which is
/// the conventional palette feel.
pub fn fuzzy_score(needle: &str, haystack: &str) -> Option<i32> {
    if needle.is_empty() {
        return Some(0);
    }

    // Lowercase both sides for case-insensitive matching. `chars` (not bytes) so
    // non-ASCII labels match correctly.
    let needle: Vec<char> = needle.chars().flat_map(char::to_lowercase).collect();
    // Cap the scanned haystack so the i32 score accumulator can't overflow on a
    // pathologically long label (command labels are short; this is pure hardening).
    let hay: Vec<char> = haystack.chars().take(8192).collect();

    let mut score = 0i32;
    let mut n = 0usize; // index into `needle`
    let mut prev_match: Option<usize> = None; // haystack index of the previous match
    let mut first_match: Option<usize> = None;

    for (h, &hc) in hay.iter().enumerate() {
        if n >= needle.len() {
            break;
        }
        // Compare lowercased haystack char to the (already-lowercased) needle char.
        // `to_lowercase` can yield multiple chars; for matching we only need the
        // first, which is correct for all the scripts a command label realistically
        // uses (the rare multi-char foldings still compare their leading char).
        let hc_lower = hc.to_lowercase().next().unwrap_or(hc);
        if hc_lower == needle[n] {
            score += 1; // flat per-char credit

            // Word-start bonus: first char overall, or right after a separator.
            let at_word_start = h == 0
                || hay
                    .get(h - 1)
                    .is_some_and(|p| matches!(p, ' ' | '_' | '-' | '/' | '.'));
            if at_word_start {
                score += 8;
            }

            // Consecutive bonus: this match directly follows the previous one.
            if let Some(prev) = prev_match
                && prev + 1 == h
            {
                score += 16;
            }

            if first_match.is_none() {
                first_match = Some(h);
            }
            prev_match = Some(h);
            n += 1;
        }
    }

    // All needle chars consumed ⇒ it's a subsequence; otherwise no match.
    if n != needle.len() {
        return None;
    }

    // Small penalty for how far in the first match started (earlier is better).
    if let Some(first) = first_match {
        score -= first as i32;
    }
    Some(score)
}

// =============================================================================
// Bonus: fuzzy-search combo box
// =============================================================================

/// A [`egui::ComboBox`] whose popup carries a search field that fuzzy-filters the
/// items via [`fuzzy_score`]. Selecting an item writes its [`Command::id`] into
/// `*selected`.
///
/// This is the "pick one of many, but the list is long" companion to the full
/// palette: a drop-in replacement for a plain combo when the option set is large
/// enough to want typing-to-filter.
///
/// - `selected`: the currently chosen command id (or `None`). Updated in place.
/// - `commands`: the candidate items (read-only).
/// - `id_salt`: disambiguates this combo from others on the same panel (passed to
///   [`egui::ComboBox::from_id_salt`]).
///
/// Returns the [`egui::Response`] of the combo's header (so callers can react to
/// it like any other widget). The per-open search text is stashed in egui's
/// temporary memory keyed by `id_salt`, so it survives across frames while the
/// popup is open and is naturally dropped when egui forgets the popup.
pub fn fuzzy_combo(
    ui: &mut egui::Ui,
    selected: &mut Option<u64>,
    commands: &[Command],
    // egui 0.35: AsIdSalt now requires Debug.
    id_salt: impl std::hash::Hash + std::fmt::Debug,
) -> egui::Response {
    // Stable id for both the ComboBox and the popup's search-text memory slot.
    let combo_id = ui.make_persistent_id(("egui_fuzzy_combo", &id_salt));

    // Header text = the selected command's label, or a placeholder.
    let selected_text = selected
        .and_then(|id| commands.iter().find(|c| c.id == id))
        .map(|c| c.label.as_str())
        .unwrap_or("Select…")
        .to_owned();

    egui::ComboBox::from_id_salt(combo_id)
        .selected_text(selected_text)
        .show_ui(ui, |ui| {
            // Pull the popup-local search text out of egui's temp memory.
            let search_id = combo_id.with("search");
            let mut search: String =
                egui_widgets_config::state::get::<String>(ui.ctx(), search_id).unwrap_or_default();

            let edit = ui.add(
                egui::TextEdit::singleline(&mut search)
                    .hint_text("Filter…")
                    .desired_width(f32::INFINITY),
            );
            // Focus the filter when the popup first opens (nothing focused yet) so
            // typing filters immediately. Guard on "no widget currently focused" so
            // we don't yank focus back every frame while the user interacts.
            if ui.memory(|m| m.focused().is_none()) {
                edit.request_focus();
            }
            ui.separator();

            // Filter + rank exactly like the palette.
            let q = search.trim();
            let mut ranked: Vec<(usize, i32)> = if q.is_empty() {
                commands.iter().enumerate().map(|(i, _)| (i, 0)).collect()
            } else {
                commands
                    .iter()
                    .enumerate()
                    .filter_map(|(i, c)| fuzzy_score(q, &c.label).map(|s| (i, s)))
                    .collect()
            };
            if !q.is_empty() {
                ranked.sort_by_key(|&(_, s)| std::cmp::Reverse(s));
            }

            egui::ScrollArea::vertical()
                .max_height(240.0)
                .show(ui, |ui| {
                    if ranked.is_empty() {
                        ui.weak("(no matches)");
                    }
                    for (i, _) in ranked {
                        let cmd = &commands[i];
                        let is_sel = *selected == Some(cmd.id);
                        if ui.selectable_label(is_sel, &cmd.label).clicked() {
                            *selected = Some(cmd.id);
                        }
                    }
                });

            // Persist the (possibly edited) search text for the next frame.
            egui_widgets_config::state::set(ui.ctx(), search_id, search);
        })
        .response
}

// =============================================================================
// Tests (pure logic — no egui context needed)
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn cmds() -> Vec<Command> {
        vec![
            Command::new(1, "Open Project"),
            Command::new(2, "Open File").hint("Ctrl+O"),
            Command::new(3, "Compile"),
            Command::new(4, "Copy Model"),
            Command::new(5, "Close Window"),
        ]
    }

    #[test]
    fn empty_needle_matches_everything() {
        assert_eq!(fuzzy_score("", "anything"), Some(0));
    }

    #[test]
    fn non_subsequence_returns_none() {
        assert!(fuzzy_score("xyz", "Open Project").is_none());
        // Right chars, wrong order → not a subsequence.
        assert!(fuzzy_score("po", "Open").is_none());
    }

    #[test]
    fn subsequence_matches() {
        assert!(fuzzy_score("opj", "Open Project").is_some());
        assert!(fuzzy_score("cmpl", "Compile").is_some());
    }

    #[test]
    fn contiguous_beats_scattered() {
        // "abc" is a subsequence of BOTH, but contiguous in "abcdef" (consecutive
        // bonuses) and gappy in "axbxc" (no consecutive bonus). The earlier "cmpl"
        // example was invalid: "cmpl" is NOT a subsequence of "Copy Model" (the `p`
        // precedes the `m` there), so the matcher correctly returns `None`.
        let contiguous = fuzzy_score("abc", "abcdef").unwrap();
        let scattered = fuzzy_score("abc", "axbxc").unwrap();
        assert!(
            contiguous > scattered,
            "contiguous {contiguous} should beat scattered {scattered}"
        );
    }

    #[test]
    fn word_start_beats_mid_word() {
        // "op" hits two word-starts in "Open Project"; in "Copy" it's mid-word.
        let word_starts = fuzzy_score("op", "Open Project").unwrap();
        let mid_word = fuzzy_score("op", "Copy").unwrap();
        assert!(word_starts > mid_word);
    }

    #[test]
    fn case_insensitive() {
        assert!(fuzzy_score("OPEN", "open project").is_some());
        assert!(fuzzy_score("open", "OPEN PROJECT").is_some());
    }

    #[test]
    fn ranked_indices_empty_query_keeps_order() {
        let p = CommandPalette::new();
        let c = cmds();
        assert_eq!(p.ranked_indices(&c), vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn ranked_indices_filters_and_ranks() {
        let mut p = CommandPalette::new();
        let c = cmds();
        p.query = "open".to_string();
        let r = p.ranked_indices(&c);
        // Only the two "Open …" commands match.
        assert_eq!(r.len(), 2);
        assert!(r.contains(&0) && r.contains(&1));
    }

    #[test]
    fn ranked_indices_no_match_is_empty() {
        let mut p = CommandPalette::new();
        let c = cmds();
        p.query = "zzzz".to_string();
        assert!(p.ranked_indices(&c).is_empty());
    }

    #[test]
    fn reset_clears_state() {
        let mut p = CommandPalette::new();
        p.query = "foo".into();
        p.selected = 3;
        p.needs_focus = false;
        p.reset();
        assert_eq!(p.query(), "");
        assert_eq!(p.selected, 0);
        assert!(p.needs_focus);
    }
}
