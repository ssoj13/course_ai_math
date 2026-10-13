//! Actions the browser reports back to the caller.
//!
//! [`crate::show`] returns a `Vec<AssetAction>` for the frame; the caller
//! pattern-matches them and updates its own model. Nothing here mutates state —
//! this is a pure description of user intent (the decoupled replacement for
//! playa's `ProjectActions` event sink).

/// A single user action emitted by the browser this frame.
///
/// All ids are the caller-defined [`crate::AssetItem::id`] values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssetAction {
    /// Double-click (or Enter-style "open") on an item. The caller decides what
    /// "open" means (activate in a viewport/timeline, etc.). Note a double-click
    /// also emits a preceding [`AssetAction::Select`].
    Open { id: u64 },

    /// Selection of `id` changed via a click. `additive` is set for a
    /// ctrl/cmd-toggle (toggle this item, keep the rest); `range` is set for a
    /// shift-click (extend from the anchor to `id`). When both are false it is a
    /// plain click → caller should select only `id`. The caller is free to
    /// implement its own selection semantics; this just reports the modifiers.
    Select {
        id: u64,
        /// Ctrl/Cmd was held (toggle, keep existing selection).
        additive: bool,
        /// Shift was held (range-extend from the anchor).
        range: bool,
    },

    /// Click on empty browser space — caller should clear the selection.
    ClearSelection,

    /// A "Create" menu entry was chosen. `kind` is the caller-supplied label of
    /// the entry (from [`crate::AssetBrowserConfig::create_kinds`]), e.g.
    /// `"Comp"` or `"Camera"`.
    Create { kind: String },

    /// The per-item delete control (or context-menu "Delete") was clicked.
    Delete { id: u64 },

    /// An in-place rename committed a new name for `id`. (Only emitted when
    /// renaming is enabled via [`crate::AssetBrowserConfig::allow_rename`].)
    Rename { id: u64, new_name: String },

    /// The "Add media" button was clicked. The widget runs NO file dialog — the
    /// caller opens its own (`rfd`, native, …) in response.
    AddMedia,

    /// A drag-out gesture started on `id`. The caller can pair this with the
    /// sibling `egui-drag-overlay` crate to paint a cross-panel ghost and detect
    /// the drop (e.g. onto a timeline). This crate only reports the start.
    BeginDrag { id: u64 },
}
