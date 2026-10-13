//! `ui-theme` — the single source of truth (SSOT) for every color / style token used across
//! the node editor stack.
//!
//! ## Why this crate exists
//! Colors used to live in three disconnected places: `wgpu-widgets-core::Theme` (widgets + HUD),
//! hard-coded constants in `nodes-view` (node bodies, pins, grid, wires), and `nodes-core`
//! category tables. There was no one place to restyle the app, and the widget theme was even
//! duplicated per HUD panel. This crate collapses all of that into ONE [`Theme`] that every layer
//! reads.
//!
//! ## Layering
//! Pure crate (only `glam` + `flex-geom` + `serde`, NO wgpu), exactly like `flex-geom`, so it can
//! be a shared leaf dependency of BOTH `nodes-core` (which must stay wgpu-free) and
//! `wgpu-widgets-core` (which re-exports [`Theme`] so widget code is unchanged).
//!
//! ## Runtime theming
//! [`Theme`] is `Serialize`/`Deserialize` (persisted in the editor's settings) and ships named
//! presets ([`Theme::dark`] / [`Theme::light`]). Host components that cache a theme implement
//! [`Themed`] so a single `apply_theme` broadcast re-styles the whole app live.

#![warn(missing_docs)]

use serde::{Deserialize, Serialize};

/// RGBA color, linear-ish 0..1 per channel — the one color type every token uses.
pub type Rgba = [f32; 4];

/// The whole app palette + metrics: widget/HUD tokens AND graph (node/pin/grid/wire) tokens in
/// one struct. Change a color here and it changes everywhere that reads it.
///
/// UI-token field NAMES are kept identical to the historical `wgpu-widgets-core::Theme` so widget
/// and HUD code compiles unchanged after that type becomes a re-export of this one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    // ---- Widget / HUD tokens (migrated verbatim from wgpu-widgets-core::Theme) ----
    /// Window / panel background.
    pub bg: Rgba,
    /// Raised surface (buttons, rows).
    pub bg_raised: Rgba,
    /// Sunken surface (input wells, tracks).
    pub bg_sunken: Rgba,
    /// Hairline borders.
    pub border: Rgba,
    /// Primary accent (selection, focus).
    pub accent: Rgba,
    /// Accent on hover.
    pub accent_hover: Rgba,
    /// Primary text.
    pub text: Rgba,
    /// Disabled / dimmed text.
    pub text_disabled: Rgba,
    /// Warning / caution accent (non-fatal notices, e.g. a transient warn toast).
    pub warn: Rgba,
    /// Danger / error accent (failed actions, e.g. a rejected wire connect toast).
    pub danger: Rgba,
    /// Corner radius for small widgets.
    pub radius_sm: f32,
    /// Corner radius for panels / large widgets.
    pub radius_md: f32,
    /// Small font size.
    pub font_size_sm: f32,
    /// Medium (default) font size.
    pub font_size_md: f32,
    /// Compact spacing unit.
    pub spacing_sm: f32,
    /// Standard spacing unit.
    pub spacing_md: f32,

    // ---- Graph tokens (were hard-coded constants in nodes-view / nodes-editor) ----
    /// Node body fill (normal node).
    pub node_body: Rgba,
    /// Node body fill for a Frame (backdrop) node — translucent.
    pub node_body_frame: Rgba,
    /// Node outline stroke (idle).
    pub node_outline: Rgba,
    /// Node outline stroke when hovered or selected (width, not color, distinguishes them).
    pub node_outline_hot: Rgba,
    /// Pin socket stroke (idle).
    pub pin_stroke: Rgba,
    /// Pin socket stroke when hovered.
    pub pin_stroke_hot: Rgba,
    /// Grid minor line.
    pub grid_minor: Rgba,
    /// Grid major (every Nth) line.
    pub grid_major: Rgba,
    /// Grid world-axis line.
    pub grid_axis: Rgba,
    /// Wire drag-preview fill (translucent).
    pub wire_preview_fill: Rgba,
    /// Wire drag-preview stroke.
    pub wire_preview_stroke: Rgba,
    /// Node title label text.
    pub title_text: Rgba,
    /// Display-flag checkbox fill when ON.
    pub flag_display_on: Rgba,
    /// Bypass-flag checkbox fill when ON.
    pub flag_bypass_on: Rgba,
    /// Bypass-flag checkbox fill when OFF.
    pub flag_bypass_off: Rgba,
}

impl Theme {
    /// The dark preset — the app's historical palette (also [`Default`]).
    pub fn dark() -> Self {
        Self {
            // Widget / HUD (verbatim from the old wgpu-widgets-core::Theme dark defaults).
            bg: [0.10, 0.11, 0.13, 1.0],
            bg_raised: [0.18, 0.19, 0.22, 1.0],
            bg_sunken: [0.07, 0.08, 0.09, 1.0],
            border: [0.30, 0.33, 0.40, 1.0],
            accent: [0.27, 0.52, 0.90, 1.0],
            accent_hover: [0.37, 0.62, 1.00, 1.0],
            text: [0.87, 0.87, 0.87, 1.0],
            text_disabled: [0.45, 0.45, 0.45, 1.0],
            warn: [0.95, 0.75, 0.25, 1.0],
            danger: [0.94, 0.38, 0.34, 1.0],
            radius_sm: 4.0,
            radius_md: 8.0,
            font_size_sm: 11.0,
            font_size_md: 13.0,
            spacing_sm: 4.0,
            spacing_md: 8.0,
            // Graph (verbatim from the old nodes-view / nodes-editor constants).
            node_body: [0.14, 0.15, 0.17, 1.0],
            node_body_frame: [0.10, 0.12, 0.16, 0.12],
            node_outline: [0.38, 0.44, 0.52, 1.0],
            node_outline_hot: [1.0, 0.82, 0.25, 1.0],
            pin_stroke: [0.10, 0.12, 0.16, 0.95],
            pin_stroke_hot: [0.02, 0.04, 0.30, 1.0],
            grid_minor: [0.20, 0.20, 0.24, 1.0],
            grid_major: [0.30, 0.30, 0.36, 1.0],
            grid_axis: [0.42, 0.42, 0.50, 1.0],
            wire_preview_fill: [0.39, 0.71, 1.0, 0.12],
            wire_preview_stroke: [0.39, 0.71, 1.0, 0.85],
            title_text: [0.92, 0.94, 0.98, 1.0],
            flag_display_on: [0.28, 0.55, 0.95, 1.0],
            flag_bypass_on: [0.20, 0.22, 0.26, 1.0],
            flag_bypass_off: [0.50, 0.53, 0.58, 1.0],
        }
    }

    /// A light preset — a calm neutral-gray UI (Figma/Blender-light feel), NOT washed-out white.
    /// Surfaces are soft grays (never pure white), borders/pins are dark enough to read, and the
    /// node title label stays LIGHT because it sits on the darkened category header band.
    pub fn light() -> Self {
        Self {
            // Surfaces: soft neutral grays, low glare (no near-white).
            bg: [0.84, 0.85, 0.87, 1.0],
            bg_raised: [0.90, 0.91, 0.93, 1.0],
            bg_sunken: [0.77, 0.78, 0.81, 1.0],
            border: [0.62, 0.64, 0.69, 1.0],
            accent: [0.16, 0.42, 0.82, 1.0],
            accent_hover: [0.24, 0.52, 0.92, 1.0],
            text: [0.14, 0.15, 0.18, 1.0],
            text_disabled: [0.48, 0.50, 0.55, 1.0],
            // Deeper, less glaring warn/danger so they read on the light neutral-gray surfaces.
            warn: [0.80, 0.52, 0.08, 1.0],
            danger: [0.80, 0.20, 0.18, 1.0],
            // Node card: a touch lighter than the window so nodes read as raised panels, with a
            // defined mid-gray outline (pure-white bodies were the "overbright" glare).
            node_body: [0.91, 0.92, 0.94, 1.0],
            node_body_frame: [0.55, 0.62, 0.74, 0.16],
            node_outline: [0.56, 0.59, 0.65, 1.0],
            node_outline_hot: [0.88, 0.58, 0.10, 1.0],
            // Pins/grid must be DARKER than the light bg to be visible.
            pin_stroke: [0.42, 0.45, 0.52, 0.95],
            pin_stroke_hot: [0.12, 0.30, 0.72, 1.0],
            grid_minor: [0.79, 0.80, 0.83, 1.0],
            grid_major: [0.70, 0.71, 0.75, 1.0],
            grid_axis: [0.56, 0.58, 0.64, 1.0],
            wire_preview_fill: [0.16, 0.42, 0.82, 0.16],
            wire_preview_stroke: [0.16, 0.42, 0.82, 0.92],
            // Title label sits on the DARK category header band → keep it light in both themes.
            title_text: [0.95, 0.96, 0.98, 1.0],
            flag_display_on: [0.16, 0.42, 0.82, 1.0],
            flag_bypass_on: [0.66, 0.68, 0.72, 1.0],
            flag_bypass_off: [0.50, 0.53, 0.58, 1.0],
            // Metrics identical to dark.
            ..Theme::dark()
        }
    }

    /// Compact container padding (`spacing_sm` on every side).
    pub fn padding_sm(&self) -> flex_geom::Padding {
        flex_geom::Padding::all(self.spacing_sm)
    }

    /// Standard container padding (`spacing_md` on every side).
    pub fn padding_md(&self) -> flex_geom::Padding {
        flex_geom::Padding::all(self.spacing_md)
    }

    /// Per-category node header accent — the Houdini-style category identity color. Theme-
    /// independent for now (categories are identity colors, not surface tints); lives here so the
    /// SSOT owns it. `nodes-core` re-exports [`category_accent`] / [`category_header_bg`].
    pub fn category_accent(&self, category: &str) -> Rgba {
        category_accent(category)
    }

    /// Darkened category header background (~40% luminance of [`Self::category_accent`]).
    pub fn category_header_bg(&self, category: &str) -> Rgba {
        category_header_bg(category)
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

/// Per-category node header accent RGBA. Free function (theme-independent identity colors) so
/// `nodes-core` can re-export it without a `Theme` instance.
pub fn category_accent(category: &str) -> Rgba {
    let (r, g, b) = match category {
        "Math" => (90, 170, 90),
        "Constants" => (140, 160, 200),
        "Vector" => (160, 110, 200),
        "Color" => (200, 180, 80),
        "Logic" => (200, 100, 90),
        "String" => (210, 160, 80),
        "Layout" => (120, 140, 160),
        "IO" => (90, 150, 190),
        "Subnet" => (210, 130, 50),
        _ => (130, 130, 130),
    };
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
}

/// Darkened header background (~40% luminance) derived from [`category_accent`].
pub fn category_header_bg(category: &str) -> Rgba {
    let [r, g, b, _] = category_accent(category);
    [r * 0.4, g * 0.4, b * 0.4, 1.0]
}

/// Implemented by any host component that caches a [`Theme`] (HUD panels, node-widget layer, the
/// renderer). A single `apply_theme` broadcast then re-styles the whole app live — the "themed
/// everywhere" mechanism.
pub trait Themed {
    /// Replace the component's cached theme. Implementations should store a clone and mark
    /// themselves dirty / relayout as needed on the next frame.
    fn set_theme(&mut self, theme: &Theme);
}
