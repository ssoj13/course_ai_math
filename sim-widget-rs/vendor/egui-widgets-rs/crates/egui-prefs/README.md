# egui-prefs

> Gathered into `egui-widgets-rs` from playa (originally `playa-prefs`); ported
> egui 0.33 → 0.34. Depends on `egui` + `egui-prefs2` (the two share one visual
> language — see "Look shared with egui-prefs2").

Pluggable preferences system for egui applications. Each subsystem
(jobs, viewport, cache, …) owns its own preferences UI and registers
a single render closure; a central **Preferences window** aggregates
them via a tree-view sidebar with search bar + Apply/OK/Cancel
buttons.

## Why this crate exists

Mature creative tools (After Effects, Houdini, Maya) let every module
expose its own preferences UI; one big monolithic settings panel is
the anti-pattern. `playa-prefs` provides the infrastructure: a
generic `PrefsRegistry<S>` over the host's settings type `S`, plus a
modal `PrefsWindow<S>` that drives the layout.

No host engine — just `egui` (+ `egui-prefs2` for the shared sidebar/section look).
Any egui app can use it.

## Module map

| File | Purpose |
|---|---|
| `lib.rs` | The modal surface: `PrefsEntry<S>`, `PrefsRegistry<S>`, `PrefsWindow<S>`, `PrefsResult` enum, render impl |
| `dock` (module in `lib.rs`) | In-DOCK live-editing prefs shell (complements the modal window) — see below |

## Public surface (canonical)

```rust
use egui_prefs::{PrefsEntry, PrefsRegistry, PrefsWindow, PrefsResult};

// Host AppSettings is any `Clone + PartialEq`:
let mut registry: PrefsRegistry<AppSettings> = PrefsRegistry::new();

registry.add(PrefsEntry {
    id: "jobs",
    label: "Jobs & Rendering",
    category: "Integrations",
    search_keywords: vec!["budget", "queue", "fal", "seedance"],
    render: Box::new(|ui, settings: &mut AppSettings| {
        ui.checkbox(&mut settings.jobs.enabled, "Enable jobs");
    }),
});

let mut window = PrefsWindow::<AppSettings>::new();
// Open via menu / hotkey:
window.open_with(&app_settings);

// Each frame (egui 0.34 is ui-first: get the ctx via `ui.ctx()`):
match window.show(ui.ctx(), &mut registry, &mut app_settings) {
    PrefsResult::Applied  => log::info!("Preferences applied"),
    PrefsResult::OkClosed => log::info!("Preferences applied + closed"),
    PrefsResult::Cancelled => log::info!("Cancelled"),
    PrefsResult::Open | PrefsResult::Closed => {}
}
```

## `dock` module — in-dock live-editing shell

An alternative to the modal `PrefsWindow`: a preferences shell that lives INSIDE a
dock panel and edits **live** (no Apply/OK/Cancel), harvested from colmap-rs's
preferences panel and decoupled. `dock::show(ui, id_salt, categories, &mut selected,
sidebar_font, reset_label, content)` draws a category sidebar (accent-bar
`sidebar_item`) + a content pane the caller fills for the selected category; helpers
`Category { icon, label }` and `section_header`. Use it when settings should apply as
the user changes them (a docked panel), vs the modal `PrefsWindow` for an
Apply/OK/Cancel dialog. The two paradigms are complementary — same crate, pick per host.

## Look shared with egui-prefs2

The sidebar/section primitives are a SINGLE visual SSOT: `dock`'s `Category`, `section_header`,
and `sidebar_item` are **re-exported from `egui-prefs2`**, and the modal `PrefsWindow` renders its
tree rows via `egui_prefs2::sidebar_item` (accent bar) + its category header via
`egui_prefs2::category_header`, at the same compact fonts. So this crate (tree-view modal + dock)
and the flat two-pane `egui-prefs2` panel read as one design and can never drift.

## State machine (modal `PrefsWindow`)

```
                          open_with(state)
                          ──────────────→
                  ┌──── working_copy = state.clone() ────┐
                  │     last_applied = state.clone()     │
                  │                                       │
       Closed ────┴─→  Open ──Apply──→  Open (dirty=false, working=state)
                       │ │
                       │ └──OK──→  Closed (working_copy committed back)
                       │
                       └─Cancel─→  Closed (working_copy discarded)
```

The Apply button is disabled until `working_copy != last_applied` —
dirty-tracking for free if `S: PartialEq`.

## Search

Search filters entries by case-insensitive substring across `label`,
`category`, and `search_keywords`. Empty query → all entries visible.

## Tests

15 unit tests covering registry CRUD, search filter cases, and the
PrefsWindow state machine transitions.

## License

MIT.
