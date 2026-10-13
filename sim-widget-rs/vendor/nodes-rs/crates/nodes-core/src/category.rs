//! Per-category colours for node header bands (Houdini-style).
//!
//! The colour table now lives in the app-wide SSOT crate `ui-theme` (so widgets, node bodies,
//! grid, wires and category headers all share ONE palette). These are thin re-exports that keep
//! `nodes-core`'s historical public API (`category_accent_rgba` / `category_header_bg_rgba`) — used
//! by `nodes-view::shape_scene` for title-bar fills — pointing at that single source.

/// Header accent RGBA (0–1). Delegates to [`ui_theme::category_accent`] (the SSOT).
pub fn category_accent_rgba(category: &str) -> [f32; 4] {
    ui_theme::category_accent(category)
}

/// Darkened header background (~40% luminance). Delegates to [`ui_theme::category_header_bg`].
pub fn category_header_bg_rgba(category: &str) -> [f32; 4] {
    ui_theme::category_header_bg(category)
}
