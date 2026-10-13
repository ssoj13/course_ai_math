//! Caller-owned data model for the asset browser.
//!
//! Everything the widget renders lives in a flat [`AssetBrowserModel`]: a list
//! of [`AssetItem`]s plus the current `selection` (a list of item ids). The
//! widget never mutates this model — it only reads it and returns
//! [`crate::AssetAction`]s describing what the user did, so the caller stays the
//! single owner of truth (mirrors playa's event-sink design, but with plain
//! values instead of engine handles).

use egui::{Color32, TextureId};

/// One row in the browser.
///
/// `id` is a caller-defined stable identifier (use whatever maps back to your
/// own asset — a slot index, a hash, etc.; this crate uses `u64` instead of a
/// `Uuid` to stay dependency-free). `kind` is a free-form group label: items
/// are grouped and visually styled by this string, so two items with the same
/// `kind` share a header and a [`KindStyle`].
#[derive(Clone, Debug, PartialEq)]
pub struct AssetItem {
    /// Stable, caller-defined id. Returned verbatim in every action.
    pub id: u64,
    /// Display name (primary label).
    pub name: String,
    /// Group label — drives grouping, the section header text and the per-kind
    /// style lookup. E.g. `"Clip"`, `"Comp"`, `"Camera"`.
    pub kind: String,
    /// Optional thumbnail texture, drawn as a small square on the left of the
    /// row. The caller is responsible for uploading and freeing the texture.
    pub thumbnail: Option<TextureId>,
    /// Optional secondary line / right-aligned badge text (e.g. `"120f 30fps"`
    /// or a filename). Rendered dimmed.
    pub subtitle: Option<String>,
}

impl AssetItem {
    /// Convenience constructor for the common name + kind case.
    pub fn new(id: u64, name: impl Into<String>, kind: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            kind: kind.into(),
            thumbnail: None,
            subtitle: None,
        }
    }

    /// Builder: attach a subtitle / badge.
    pub fn with_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Builder: attach a thumbnail texture.
    pub fn with_thumbnail(mut self, tex: TextureId) -> Self {
        self.thumbnail = Some(tex);
        self
    }
}

/// The complete, flat browser state owned by the caller.
///
/// `selection` lists the ids of the currently-selected items (order is the
/// click order; not necessarily the display order). Ids in `selection` that no
/// longer exist in `items` are simply ignored during rendering.
#[derive(Clone, Debug, Default)]
pub struct AssetBrowserModel {
    /// All items, in display order. Grouping by `kind` is stable: each kind's
    /// header is placed at the position of its first item.
    pub items: Vec<AssetItem>,
    /// Ids of selected items.
    pub selection: Vec<u64>,
}

impl AssetBrowserModel {
    /// Empty model.
    pub fn new() -> Self {
        Self::default()
    }

    /// True if `id` is in the selection.
    pub fn is_selected(&self, id: u64) -> bool {
        self.selection.contains(&id)
    }
}

/// Per-kind visual style, looked up by [`AssetItem::kind`].
///
/// Generalizes playa's hard-coded icon/colour table (File→green `[F]`,
/// Camera→orange `[K]`, …). The caller supplies a small map of these; anything
/// not in the map falls back to [`KindStyle::default`].
#[derive(Clone, Debug)]
pub struct KindStyle {
    /// Short text icon drawn before the name, e.g. `"[F]"`. Empty hides it.
    pub icon: String,
    /// Icon / accent colour.
    pub color: Color32,
}

impl Default for KindStyle {
    fn default() -> Self {
        Self {
            icon: String::new(),
            color: Color32::from_rgb(160, 160, 160),
        }
    }
}

impl KindStyle {
    /// New style with an icon and colour.
    pub fn new(icon: impl Into<String>, color: Color32) -> Self {
        Self {
            icon: icon.into(),
            color,
        }
    }
}
