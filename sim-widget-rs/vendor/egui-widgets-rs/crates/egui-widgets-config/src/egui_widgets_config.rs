//! Shared design tokens + runtime config for the egui-widgets-rs widget set.
//!
//! Every widget reads its font sizes, spacing/radii/stroke metrics and semantic
//! colours from here instead of hardcoding literals, so the whole set stays
//! visually consistent and follows the host theme.
//!
//! - [`Fonts`] / [`Metrics`] — numeric tokens (sizes in points). `Copy`, defaulted.
//! - [`Palette`] — the few semantic colours egui's [`egui::Visuals`] does NOT
//!   carry (accent / ok / warn / err / source-dim / playhead).
//! - [`Semantic`] — the *resolved* colour set a widget actually paints with:
//!   selection/hover/weak/faint come LIVE from `Visuals` (theme-aware), the rest
//!   from the [`Palette`]. Build it per-frame with [`semantic`] / [`Semantic::resolve`].
//! - [`WidgetsConfig`] — the host-overridable bundle (`fonts` + `metrics` +
//!   `palette`). Install once at startup with [`WidgetsConfig::install`]; widgets
//!   read [`WidgetsConfig::get`] with a [`Default`] fallback.
//!
//! Everything is `Copy` and egui-only — no serde, no allocation. The host derives
//! these from its own (separately persisted) theme; this crate just carries them.
//!
//! ```no_run
//! # use egui_widgets_config as cfg;
//! // host, once at startup (optional — Default is sane):
//! # fn install(ctx: &egui::Context) {
//! cfg::WidgetsConfig::install(ctx, cfg::WidgetsConfig::default());
//! # }
//! // any widget, per frame:
//! # fn paint(ui: &mut egui::Ui) {
//! let c = cfg::WidgetsConfig::get(ui);
//! let sem = cfg::semantic(ui);
//! let font = cfg::prop(c.fonts.md);
//! let _ = (font, sem.selection, c.metrics.row_h);
//! # }
//! ```

pub mod attr_layout;
mod attr_metrics;
mod segmented;
pub use attr_layout::{
    AttrLayout, AttrRowRects, ValueEditorLayout, cell_ui, icon_button, icon_button_tip,
    icon_toggle, square_icon, square_icon_with_paint,
};
pub use attr_metrics::AttrMetrics;
pub use segmented::{segmented, segmented_idx, segmented_pick, segmented_tips};

use egui::{Color32, Context, FontFamily, FontId, Id, Ui, Visuals};

/// egui memory key under which the active [`WidgetsConfig`] is stored.
fn config_id() -> Id {
    Id::new("egui_widgets_config")
}

// ============================================================================
// Fonts
// ============================================================================

/// Font-size scale, in points. Use [`mono`] / [`prop`] to turn a size into a
/// [`FontId`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fonts {
    /// Tiny — dense ruler labels, badges. Default 9.
    pub xs: f32,
    /// Small — secondary text, table cells. Default 11.
    pub sm: f32,
    /// Medium — the default body size. Default 13.
    pub md: f32,
    /// Large — headings. Default 16.
    pub lg: f32,
}

impl Default for Fonts {
    fn default() -> Self {
        Self {
            xs: 9.0,
            sm: 11.0,
            md: 13.0,
            lg: 16.0,
        }
    }
}

impl Fonts {
    /// Derive the token scale from egui's standard [`egui::TextStyle`] sizes, so a
    /// host that drives fonts through the idiomatic `Style::text_styles` (THE egui
    /// standard) gets these painter-text tokens for free instead of maintaining a
    /// second, parallel font system: `xs`←Small, `md`←Body, `lg`←Heading,
    /// `sm`←midpoint(Small, Body). Missing styles fall back to the corresponding
    /// [`Default`] value.
    ///
    /// Laid-out chrome (`ui.label`/`ui.button`) already follows `text_styles`
    /// automatically; these tokens exist only for DIRECT `painter.text()` sites
    /// (rulers, HUDs) where egui does no TextStyle resolution. Feeding them from
    /// the same TextStyles keeps a single host-facing font control.
    pub fn from_text_styles(style: &egui::Style) -> Self {
        let d = Self::default();
        let sz = |ts: egui::TextStyle, fallback: f32| {
            style
                .text_styles
                .get(&ts)
                .map(|f| f.size)
                .unwrap_or(fallback)
        };
        let small = sz(egui::TextStyle::Small, d.xs);
        let body = sz(egui::TextStyle::Body, d.md);
        let heading = sz(egui::TextStyle::Heading, d.lg);
        Self {
            xs: small,
            sm: (small + body) * 0.5,
            md: body,
            lg: heading,
        }
    }
}

/// A proportional [`FontId`] of the given size.
pub fn prop(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

/// A monospace [`FontId`] of the given size.
pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

// ============================================================================
// Metrics
// ============================================================================

/// Spacing / radius / stroke / row-height tokens, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub space_xs: f32,
    pub space_sm: f32,
    pub space_md: f32,
    pub space_lg: f32,
    /// Small corner radius (chips, cells). Default 3.
    pub radius_sm: f32,
    /// Medium corner radius (panels, bars). Default 4.
    pub radius_md: f32,
    /// Thin stroke (separators, idle outlines). Default 1.
    pub stroke_thin: f32,
    /// Thick stroke (focus / active outlines). Default 2.
    pub stroke_thick: f32,
    /// Default list/table row height. Default 18.
    pub row_h: f32,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            space_xs: 2.0,
            space_sm: 4.0,
            space_md: 6.0,
            space_lg: 8.0,
            radius_sm: 3.0,
            radius_md: 4.0,
            stroke_thin: 1.0,
            stroke_thick: 2.0,
            row_h: 18.0,
        }
    }
}

// ============================================================================
// Animation timing
// ============================================================================

/// Animation durations (seconds) shared by every widget that eases a transition
/// (hover/selection fade, disclosure-chevron rotation, playhead/zoom smoothing,
/// gizmo highlight). Host-overridable via [`WidgetsConfig`]; a host that wants a
/// snappy, no-animation feel can zero these and every widget falls back to an
/// instant switch (egui's animate helpers treat `0.0` as "no animation").
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anim {
    /// Quick UI feedback — hover/selection fade, chevron rotation. Default 0.10.
    pub fast: f32,
    /// Standard transition — expand/collapse, zoom-to-fit, playhead seek. Default 0.15.
    pub med: f32,
}

impl Default for Anim {
    fn default() -> Self {
        Self {
            fast: 0.10,
            med: 0.15,
        }
    }
}

// ============================================================================
// Colours
// ============================================================================

/// Semantic colours egui's [`Visuals`] does NOT provide. Defaults are tuned for
/// a dark theme; the host can override per-slot via [`WidgetsConfig`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    /// Primary accent (playhead-less highlights, active toggles).
    pub accent: Color32,
    /// Success / cached / available.
    pub ok: Color32,
    /// Warning / pending.
    pub warn: Color32,
    /// Error / unavailable.
    pub err: Color32,
    /// Tint for source-provenance (read-only / file-derived) attributes.
    pub source_dim: Color32,
    /// Timeline / scrubber playhead.
    pub playhead: Color32,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            accent: Color32::from_rgb(100, 150, 255),
            ok: Color32::from_rgb(100, 200, 100),
            warn: Color32::from_rgb(235, 185, 80),
            err: Color32::from_rgb(230, 100, 100),
            source_dim: Color32::from_rgb(150, 150, 160),
            playhead: Color32::from_rgb(100, 210, 100),
        }
    }
}

/// The resolved colour set a widget paints with: theme-derived slots (selection /
/// hover / weak / faint) merged with the [`Palette`] extras. Build per frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Semantic {
    /// Selected-item fill (from `Visuals::selection`).
    pub selection: Color32,
    /// Hovered-item fill (from `Visuals::widgets.hovered`).
    pub hover: Color32,
    /// Weak/secondary text.
    pub weak: Color32,
    /// Faint background stripe.
    pub faint: Color32,
    /// Deepest recessed surface (rulers, gutters, track backgrounds). Theme-aware
    /// (`Visuals::extreme_bg_color`) — darker than [`Self::panel`].
    pub surface: Color32,
    /// Panel / row base fill (`Visuals::panel_fill`); alternate rows derive from it.
    pub panel: Color32,
    pub accent: Color32,
    pub ok: Color32,
    pub warn: Color32,
    pub err: Color32,
    pub source_dim: Color32,
    pub playhead: Color32,
}

impl Semantic {
    /// Merge the live theme (`visuals`) with a `palette`'s extras.
    pub fn resolve(visuals: &Visuals, palette: &Palette) -> Self {
        Self {
            selection: visuals.selection.bg_fill,
            hover: visuals.widgets.hovered.bg_fill,
            weak: visuals.weak_text_color(),
            faint: visuals.faint_bg_color,
            surface: visuals.extreme_bg_color,
            panel: visuals.panel_fill,
            accent: palette.accent,
            ok: palette.ok,
            warn: palette.warn,
            err: palette.err,
            source_dim: palette.source_dim,
            playhead: palette.playhead,
        }
    }
}

/// Tint a control's fill with a semantic `hue`, just enough to read as a category.
///
/// THE one accent-wash formula for the whole widget set. A button row that colours "creates" /
/// "imports" / "destroys" is only useful if every such row agrees, and the temptation is to
/// re-derive the mix per panel - which is how two bars end up differently saturated and neither
/// looks deliberate.
///
/// Starts from `inactive.weak_bg_fill` rather than a fixed pastel so it lands correctly in BOTH
/// themes: a fixed light green reads as a highlight on a dark theme and as a disabled control on
/// a light one. The wash is small on purpose - a control the eye can still read as a control,
/// with a hint of what it will do.
pub fn accent_wash(visuals: &Visuals, hue: Color32) -> Color32 {
    /// Fraction of the hue mixed into the base fill.
    const WASH: f32 = 20.0 / 255.0;
    let base = visuals.widgets.inactive.weak_bg_fill;
    let mix = |b: u8, h: u8| (f32::from(b) * (1.0 - WASH) + f32::from(h) * WASH).round() as u8;
    Color32::from_rgb(
        mix(base.r(), hue.r()),
        mix(base.g(), hue.g()),
        mix(base.b(), hue.b()),
    )
}

/// Standard X/Y/Z axis colours (R=X, G=Y, B=Z) — the ONE convention every 3D
/// widget that paints *informational* axes shares (the viewport axis indicator
/// today; any future orientation gizmo / ground grid). The stored colour is the
/// base **line** colour; [`AxisColors::label`] derives the slightly-brighter
/// **label** variant so there is a single source of truth per axis.
///
/// Theme-INDEPENDENT by design (like a colour picker's wheel hues): an axis is
/// red because it is X, not because of the host theme. Host-overridable via
/// [`WidgetsConfig`] for a studio that standardises on a different axis palette.
///
/// NOTE — deliberate exception: `egui-gizmo`'s manipulator handles keep their own
/// stylised `GizmoVisuals` axis colours (a distinct neon aesthetic, host-overridable
/// at construction) and are NOT driven from here; the gizmo stays hermetic
/// (egui + glam only) per its fork decision.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisColors {
    /// X axis (red by convention).
    pub x: Color32,
    /// Y axis (green by convention).
    pub y: Color32,
    /// Z axis (blue by convention).
    pub z: Color32,
}

impl Default for AxisColors {
    fn default() -> Self {
        Self {
            x: Color32::from_rgb(220, 50, 50),
            y: Color32::from_rgb(50, 200, 50),
            z: Color32::from_rgb(50, 100, 220),
        }
    }
}

impl AxisColors {
    /// The three base (line) colours in X, Y, Z order.
    pub fn to_array(self) -> [Color32; 3] {
        [self.x, self.y, self.z]
    }

    /// The brighter text-label variant of a base axis colour (lerped ~22% toward
    /// white), so a label reads as the same axis but pops against its line.
    pub fn label(base: Color32) -> Color32 {
        base.lerp_to_gamma(Color32::WHITE, 0.22)
    }
}

// ============================================================================
// Config bundle + memory storage
// ============================================================================

/// The host-overridable token bundle. Read via [`Self::get`]; install once via
/// [`Self::install`]. `Copy` — lives in egui memory at no allocation cost.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct WidgetsConfig {
    pub fonts: Fonts,
    pub metrics: Metrics,
    pub palette: Palette,
    pub anim: Anim,
    pub axis: AxisColors,
}

impl WidgetsConfig {
    /// Store this config in egui memory (call once at startup; optional — the
    /// `Default` is used when nothing is installed).
    pub fn install(ctx: &Context, config: WidgetsConfig) {
        ensure_icon_font(ctx);
        ctx.data_mut(|d| d.insert_temp(config_id(), config));
    }

    /// The active config for this `ui`'s context, or [`Default`] if none installed.
    pub fn get(ui: &Ui) -> WidgetsConfig {
        Self::get_ctx(ui.ctx())
    }

    /// The active config for a raw `ctx`.
    pub fn get_ctx(ctx: &Context) -> WidgetsConfig {
        // Every widget with chrome reads its tokens through here, so this is also where the
        // icon font is guaranteed: a host that never set it up still gets icons, not tofu.
        ensure_icon_font(ctx);
        ctx.data(|d| d.get_temp::<WidgetsConfig>(config_id()))
            .unwrap_or_default()
    }
}

/// Add the phosphor icon font (what [`icons`] draws) to a host's font definitions: the one call
/// for a host that builds its own [`egui::FontDefinitions`], so it needs no `egui-phosphor`
/// dependency of its own.
pub fn add_icon_font(fonts: &mut egui::FontDefinitions) {
    egui_phosphor::add_to_fonts(fonts, egui_phosphor::Variant::Regular);
}

/// Make sure the context has the icon font. Idempotent and cheap: egui ignores a font it already
/// has, and the check runs at most once per pass. Called by [`WidgetsConfig::install`] and
/// [`WidgetsConfig::get_ctx`] (so by every widget that reads its tokens), which makes the widget
/// set self-sufficient; hosts that set the font up themselves are unaffected. The font becomes
/// active at the start of the next pass.
pub fn ensure_icon_font(ctx: &Context) {
    let pass_id = Id::new("egui_widgets_icon_font_pass");
    let pass = ctx.cumulative_pass_nr();
    if ctx.data(|d| d.get_temp::<u64>(pass_id)) == Some(pass) {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(pass_id, pass));
    ctx.add_font(egui::epaint::text::FontInsert::new(
        "phosphor",
        egui_phosphor::Variant::Regular.font_data(),
        vec![egui::epaint::text::InsertFontFamily {
            family: egui::FontFamily::Proportional,
            priority: egui::epaint::text::FontPriority::Lowest,
        }],
    ));
}

/// The resolved [`Semantic`] for this frame: the installed (or default) palette
/// merged with `ui`'s live theme. The common path for widgets.
pub fn semantic(ui: &Ui) -> Semantic {
    let cfg = WidgetsConfig::get(ui);
    Semantic::resolve(ui.visuals(), &cfg.palette)
}

/// The installed (or default) [`Anim`] timing tokens for this `ui`.
pub fn anim(ui: &Ui) -> Anim {
    WidgetsConfig::get(ui).anim
}

// ============================================================================
// Shared iconography
// ============================================================================

/// Named icon glyphs — the widget set's shared vocabulary, re-exported from
/// `egui-phosphor` (regular weight). Reference these (`icons::SEARCH`) instead of
/// hardcoding a phosphor constant per widget, so the same concept shows the same
/// glyph everywhere. The host still registers the phosphor font once via
/// `egui_phosphor::add_to_fonts`.
pub mod icons {
    use egui_phosphor::regular as p;

    pub const SEARCH: &str = p::MAGNIFYING_GLASS;
    pub const CLOSE: &str = p::X;
    pub const ADD: &str = p::PLUS;
    pub const REMOVE: &str = p::MINUS;
    pub const TRASH: &str = p::TRASH;
    pub const RESET: &str = p::ARROW_COUNTER_CLOCKWISE;
    pub const COPY: &str = p::COPY;
    pub const PASTE: &str = p::CLIPBOARD;
    pub const CARET_RIGHT: &str = p::CARET_RIGHT;
    pub const CARET_DOWN: &str = p::CARET_DOWN;
    pub const CARET_UP: &str = p::CARET_UP;
    pub const CARET_LEFT: &str = p::CARET_LEFT;
    // Directional arrows (navigation / key hints; phosphor private-use glyphs
    // that always render, unlike bare U+2190… arrows missing from many fonts).
    pub const ARROW_LEFT: &str = p::ARROW_LEFT;
    pub const ARROW_RIGHT: &str = p::ARROW_RIGHT;
    pub const ARROW_UP: &str = p::ARROW_UP;
    pub const ARROW_DOWN: &str = p::ARROW_DOWN;
    /// Bidirectional ↔ (e.g. "left/right arrow keys").
    pub const ARROWS_LEFT_RIGHT: &str = p::ARROWS_LEFT_RIGHT;
    pub const VISIBLE: &str = p::EYE;
    pub const HIDDEN: &str = p::EYE_SLASH;
    pub const LOCK: &str = p::LOCK_SIMPLE;
    pub const UNLOCK: &str = p::LOCK_SIMPLE_OPEN;
    pub const WARNING: &str = p::WARNING;
    pub const ERROR: &str = p::X_CIRCLE;
    pub const INFO: &str = p::INFO;
    pub const CHECK: &str = p::CHECK;
    /// Circled checkmark — success/complete state (distinct from the bare [`CHECK`] tick).
    pub const SUCCESS: &str = p::CHECK_CIRCLE;
    pub const CIRCLE: &str = p::CIRCLE;
    pub const DOT: &str = p::DOT_OUTLINE;
    /// Animation on / off (stopwatch).
    pub const TIMER: &str = p::TIMER;
    /// A key at the current time.
    pub const KEY: &str = p::DIAMOND;
    pub const FOLDER: &str = p::FOLDER;
    pub const FOLDER_OPEN: &str = p::FOLDER_OPEN;
    pub const FILE: &str = p::FILE;
    pub const GEAR: &str = p::GEAR;
    /// A checked / unchecked box (selection toggles).
    pub const CHECKED: &str = p::CHECK_SQUARE;
    pub const UNCHECKED: &str = p::SQUARE;
    /// Help / "what is this".
    pub const HELP: &str = p::QUESTION;

    // Media / pool item kinds. A host's "create" surfaces (action bar, right-click menu, Add
    // menu) label the SAME kind from here, so one kind cannot pick up two different glyphs
    // depending on which surface happened to draw it.
    /// Footage / media clip.
    pub const MEDIA: &str = p::FILM_STRIP;
    /// Import a whole folder of media.
    pub const FOLDER_ADD: &str = p::FOLDER_PLUS;
    /// Composition — stacked layers.
    pub const COMP: &str = p::STACK;
    /// Camera item.
    pub const CAMERA: &str = p::VIDEO_CAMERA;
    /// Text item.
    pub const TEXT: &str = p::TEXT_T;
    /// AI / generative node.
    pub const AI: &str = p::SPARKLE;
    /// Rename / edit-in-place affordance (pencil-on-paper).
    pub const RENAME: &str = p::NOTE_PENCIL;

    // Editor tools / history (shared toolbar vocabulary).
    /// Selection / pointer tool.
    pub const SELECT: &str = p::CURSOR;
    /// Move / translate tool.
    pub const MOVE: &str = p::ARROWS_OUT_CARDINAL;
    /// Rotate tool.
    pub const ROTATE: &str = p::ARROW_CLOCKWISE;
    /// Scale tool.
    pub const SCALE: &str = p::CORNERS_OUT;
    /// Shear / skew tool — the shape it makes.
    pub const SHEAR: &str = p::PARALLELOGRAM;
    /// Viewer exposure (stops of light), not a light source.
    pub const EXPOSURE: &str = p::SUN;
    /// Cut / razor / split tool.
    pub const CUT: &str = p::SCISSORS;
    /// Undo the last action.
    pub const UNDO: &str = p::ARROW_U_UP_LEFT;
    /// Redo the last undone action.
    pub const REDO: &str = p::ARROW_U_UP_RIGHT;
    /// Reload / refresh a list or a view (not a rotation tool: that is [`ROTATE`]).
    pub const REFRESH: &str = p::ARROWS_CLOCKWISE;
    /// Open in another window or application.
    pub const EXTERNAL: &str = p::ARROW_SQUARE_OUT;
    /// Draw a random value for one object.
    pub const RANDOM: &str = p::DICE_ONE;
    /// Draw random values across every selected object.
    pub const RANDOM_ALL: &str = p::DICE_FIVE;

    // Transport / timeline controls (shared by the timeline toolbar + any player UI).
    pub const PLAY: &str = p::PLAY;
    pub const PAUSE: &str = p::PAUSE;
    pub const STOP: &str = p::STOP;
    pub const SKIP_BACK: &str = p::SKIP_BACK;
    pub const SKIP_FORWARD: &str = p::SKIP_FORWARD;
    pub const LOOP: &str = p::REPEAT;
    pub const FIT: &str = p::ARROWS_OUT;
    pub const SNAP: &str = p::MAGNET;
    pub const ZOOM_IN: &str = p::MAGNIFYING_GLASS_PLUS;
    pub const ZOOM_OUT: &str = p::MAGNIFYING_GLASS_MINUS;
    pub const RECORD: &str = p::RECORD;

    // Application preferences and media tools use this same vocabulary.
    pub const MEMORY: &str = p::MEMORY;
    pub const MONITOR: &str = p::MONITOR;
    pub const PALETTE: &str = p::PALETTE;
    pub const CPU: &str = p::CPU;
    pub const TEXT_AA: &str = p::TEXT_AA;
    pub const HARD_DRIVES: &str = p::HARD_DRIVES;
    pub const CLOUD: &str = p::CLOUD;
    pub const COLUMNS: &str = p::COLUMNS;
    pub const LINK: &str = p::LINK;
    pub const ARROWS_HORIZONTAL: &str = p::ARROWS_HORIZONTAL;
    pub const TRANSITION: &str = p::SPLIT_HORIZONTAL;
    pub const CHAT: &str = p::CHAT_CIRCLE_TEXT;
    pub const ATTACHMENT: &str = p::PAPERCLIP;
    pub const AUDIO: &str = p::MUSIC_NOTES;
    pub const WAVEFORM: &str = p::WAVEFORM;
    pub const LINE_SEGMENT: &str = p::LINE_SEGMENT;
    pub const PAN: &str = p::HAND;
    pub const MARKER: &str = p::MARKER_CIRCLE;
    pub const HEADPHONES: &str = p::HEADPHONES;
    pub const LOGIN: &str = p::SIGN_IN;
    pub const LOGOUT: &str = p::SIGN_OUT;
    pub const CALENDAR: &str = p::CALENDAR;
    pub const USER: &str = p::USER;
    pub const TAG: &str = p::TAG;
    pub const VIEW_3D: &str = p::CUBE;
    pub const KEYPOINTS: &str = p::DOTS_SIX;
    pub const POINT_CLOUD: &str = p::DOTS_NINE;
    pub const MATCHES: &str = p::LINE_SEGMENTS;
    pub const IMAGES: &str = p::IMAGES;
    pub const SPLATS: &str = p::SPARKLE;
    pub const GRID: &str = p::GRID_FOUR;
    pub const FRACTAL: &str = p::CUBE;
    pub const ENVIRONMENT: &str = p::GLOBE;
    pub const LIGHT: &str = p::SUN;
    pub const POWER: &str = p::POWER;
    pub const FLIGHT: &str = p::AIRPLANE;
    pub const PATH_TRACE: &str = p::SPARKLE;
    pub const SAVE: &str = p::FLOPPY_DISK;
    pub const THEME_DARK: &str = p::MOON;
    pub const THEME_LIGHT: &str = p::SUN;
    pub const PAINT_BRUSH: &str = p::PAINT_BRUSH;
    pub const SHORTCUTS: &str = p::KEY;
    pub const GLOBE: &str = p::GLOBE;
    pub const SPEAKER_ON: &str = p::SPEAKER_HIGH;
    pub const SPEAKER_OFF: &str = p::SPEAKER_SLASH;
    pub const PLUGINS: &str = p::PUZZLE_PIECE;
    pub const LIGHTNING: &str = p::LIGHTNING;

    // Viewer backdrop kinds (`egui_display::BackdropKind::icon`): what shows under the
    // image's alpha. The menu and any host surface label a kind with the same glyph.
    /// Transparency checkerboard.
    pub const BACKDROP_CHECKER: &str = p::CHECKERBOARD;
    /// Solid black.
    pub const BACKDROP_BLACK: &str = p::SQUARE;
    /// 50% grey.
    pub const BACKDROP_GRAY: &str = p::SQUARE_HALF;
    /// A colour the user picked.
    pub const BACKDROP_CUSTOM: &str = p::PALETTE;
}

// ============================================================================
// Time axis (shared timeline zoom / pan mapping)
// ============================================================================

/// Zoom clamp range shared by every timeline-like widget (track-timeline,
/// keyframes, …) so their zoom limits agree.
pub const ZOOM_MIN: f32 = 0.1;
pub const ZOOM_MAX: f32 = 20.0;

/// The shared time-axis view: horizontal zoom + pan plus the ONE implementation
/// of the frame ⇄ screen-x mapping every timeline widget in the set maps through,
/// so a clip-bar timeline and a keyframe panel stacked on the same ruler stay
/// pixel-aligned. `Copy`, egui-only, no serde — like the rest of this crate;
/// widgets keep their OWN persisted `zoom`/`pan_offset` fields and build a
/// [`TimeView`] from them per call (or store one directly).
///
/// `pixels_per_frame` is the base scale at `zoom == 1.0`; effective ppf is
/// `pixels_per_frame * zoom`.
pub mod time_axis {
    /// Zoom clamp range (re-exported at crate root as [`crate::ZOOM_MIN`] /
    /// [`crate::ZOOM_MAX`]).
    pub use super::{ZOOM_MAX, ZOOM_MIN};

    /// Shared zoom + pan for a horizontal frame axis. See the module docs.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct TimeView {
        /// Zoom multiplier (`1.0` = default). Clamp via [`Self::apply_zoom`].
        pub zoom: f32,
        /// Horizontal scroll offset, in frames — the frame at the left edge.
        pub pan_offset: f32,
    }

    impl Default for TimeView {
        fn default() -> Self {
            Self {
                zoom: 1.0,
                pan_offset: 0.0,
            }
        }
    }

    impl TimeView {
        /// Construct from persisted zoom/pan fields.
        #[inline]
        pub fn new(zoom: f32, pan_offset: f32) -> Self {
            Self { zoom, pan_offset }
        }

        /// Effective pixels-per-frame at the current zoom.
        #[inline]
        pub fn ppf(&self, pixels_per_frame: f32) -> f32 {
            pixels_per_frame * self.zoom
        }

        /// Map a frame to a screen x, given the axis area's left edge.
        #[inline]
        pub fn frame_to_x(&self, frame: f32, area_min_x: f32, pixels_per_frame: f32) -> f32 {
            area_min_x + (frame - self.pan_offset) * self.ppf(pixels_per_frame)
        }

        /// Map a screen x back to a (fractional) frame.
        #[inline]
        pub fn x_to_frame(&self, x: f32, area_min_x: f32, pixels_per_frame: f32) -> f32 {
            (x - area_min_x) / self.ppf(pixels_per_frame) + self.pan_offset
        }

        /// Convert a horizontal pixel delta to a rounded frame delta.
        #[inline]
        pub fn px_to_frames(&self, delta_px: f32, pixels_per_frame: f32) -> i64 {
            (delta_px / self.ppf(pixels_per_frame)).round() as i64
        }

        /// Apply a multiplicative zoom factor, clamped to `[ZOOM_MIN, ZOOM_MAX]`.
        pub fn apply_zoom(&mut self, factor: f32) {
            self.zoom = (self.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
        }
    }

    /// The 1/2/5 × 10ⁿ "nice number" ladder used for ruler tick spacing, shared
    /// so every timeline ruler lands ticks on the same round numbers.
    ///
    /// [`nice_step_up`] returns the smallest ladder value `>= min` — for "give me
    /// a step at least this many units wide so labels don't collide".
    /// [`nice_step_down`] returns the largest ladder value `<= n` — for bucketing
    /// an already-computed step count down to a round value.
    pub fn nice_step_up(min: f64) -> f64 {
        let min = min.max(1.0);
        let mag = 10f64.powf(min.log10().floor());
        for m in [1.0, 2.0, 5.0, 10.0] {
            let step = m * mag;
            if step >= min {
                return step;
            }
        }
        10.0 * mag
    }

    /// Largest 1/2/5 × 10ⁿ ladder value `<= n` (see [`nice_step_up`]).
    pub fn nice_step_down(n: f64) -> f64 {
        if n <= 1.0 {
            return 1.0;
        }
        let mag = 10f64.powf(n.log10().floor());
        let norm = n / mag;
        let nice = if norm < 2.0 {
            1.0
        } else if norm < 5.0 {
            2.0
        } else {
            5.0
        };
        nice * mag
    }

    #[cfg(test)]
    mod tests {
        use super::{TimeView, nice_step_down, nice_step_up};

        #[test]
        fn nice_up_rounds_up() {
            assert_eq!(nice_step_up(1.0), 1.0);
            assert_eq!(nice_step_up(3.0), 5.0);
            assert_eq!(nice_step_up(6.0), 10.0);
            assert_eq!(nice_step_up(15.0), 20.0);
        }

        #[test]
        fn nice_down_rounds_down() {
            assert_eq!(nice_step_down(1.0), 1.0);
            assert_eq!(nice_step_down(3.0), 2.0);
            assert_eq!(nice_step_down(9.0), 5.0);
            assert_eq!(nice_step_down(15.0), 10.0);
            assert_eq!(nice_step_down(50.0), 50.0);
        }

        #[test]
        fn frame_x_roundtrip() {
            let v = TimeView::new(2.0, 10.0);
            let x = v.frame_to_x(25.0, 100.0, 2.0);
            assert!((v.x_to_frame(x, 100.0, 2.0) - 25.0).abs() < 1e-4);
        }

        #[test]
        fn apply_zoom_clamps() {
            let mut v = TimeView::default();
            v.apply_zoom(1000.0);
            assert!((v.zoom - super::ZOOM_MAX).abs() < 1e-6);
            v.apply_zoom(0.0);
            assert!((v.zoom - super::ZOOM_MIN).abs() < 1e-6);
        }
    }
}

// ============================================================================
// Timeline metrics (generic density tokens for AE/NLE-style timelines)
// ============================================================================

/// Generic sizing tokens for a layer/track timeline (outline rows + graph bars).
///
/// Why: NLE/AE-style timelines hardcode dozens of pixel constants (row height,
/// column gap, handle grab radius, bar height…) scattered across the widget,
/// which makes them coarse ("primer-big") and impossible to tune. This is the
/// ONE place those live, so a host can pick a [`Density`] preset or override any
/// single knob. Domain-specific column *widths* (opacity/blend/matte/…) are NOT
/// here — they belong to the host, which owns the column set; this holds only the
/// generic, reusable geometry. `Copy`, no serde (per crate convention: the app
/// derives these from its own persisted theme).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimelineMetrics {
    /// Outline/track row height. Compact 20, cozy 24, comfortable 30.
    pub row_h: f32,
    /// Column-header band height (labels above the rows).
    pub header_h: f32,
    /// Horizontal gap between outline columns.
    pub col_gap: f32,
    /// Inner horizontal padding inside a single column cell.
    pub cell_pad: f32,
    /// A/V toggle glyph size (eye / speaker / solo / lock).
    pub icon: f32,
    /// Height of an inline control (combo / slider / drag-value) inside a row.
    pub ctrl_h: f32,
    /// Minimum interactive width for an inline slider before it collapses to a
    /// drag-value.
    pub slider_min_w: f32,
    /// Clip bar height on the graph strip (≤ `row_h`).
    pub bar_h: f32,
    /// Clip bar corner radius.
    pub bar_radius: f32,
    /// Ruler (timecode tick) band height.
    pub ruler_h: f32,
    /// Minimum pixels between minor ruler ticks before they thin out.
    pub tick_min_px: f32,
    /// Frame-cache status strip thickness (painted under the ruler).
    pub cache_strip_h: f32,
    /// Pixel grab radius for bar trim/slide handles and work-area ends.
    pub handle_grab: f32,
    /// Extra grab padding (points) added to a keyframe diamond's *drawn* radius,
    /// so the tiny diamonds are comfortable to click (hit-slop; Fitts's law).
    /// The effective keyframe grab half-width is `drawn_radius + key_grab_pad`.
    pub key_grab_pad: f32,
    /// Pick priority of a keyframe vs a clip trim/slide handle where their grab
    /// zones overlap (the thin fringe just below a clip bar). Higher wins; AE-style
    /// the keyframe (`prio_key`) sits on top of the tool handle (`prio_tool`).
    pub prio_key: i32,
    /// Companion to [`Self::prio_key`] — the clip trim/slide handle's pick priority.
    pub prio_tool: i32,
    /// Row hover-fill gamma multiplier (0..1; lower = subtler highlight).
    pub hover_gamma: f32,
    /// Height of ONE expanded property lane (a twirl-down keyframe row) below a
    /// track's base row. Compact by design so a twirled-open layer stays dense;
    /// the timeline widget stacks `lanes.len()` of these under an expanded track.
    pub lane_height: f32,
}

/// Density presets for [`TimelineMetrics`] — one dial from tight to roomy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Density {
    /// Tight, slick (AE/Winamp-compact). The default.
    #[default]
    Compact,
    /// A little breathing room.
    Cozy,
    /// Roomy — large hit targets.
    Comfortable,
}

impl TimelineMetrics {
    /// Build metrics for a [`Density`] preset. Only `row_h`/`header_h`/`bar_h`/
    /// `ctrl_h` scale with density; the small geometry (gaps, radii, grab radius)
    /// stays constant so alignment math is stable across presets.
    pub fn for_density(d: Density) -> Self {
        let (row_h, header_h, ctrl_h, lane_height) = match d {
            Density::Compact => (20.0, 15.0, 16.0, 14.0),
            Density::Cozy => (24.0, 16.0, 18.0, 16.0),
            Density::Comfortable => (30.0, 18.0, 20.0, 18.0),
        };
        Self {
            row_h,
            header_h,
            col_gap: 4.0,
            cell_pad: 4.0,
            icon: 14.0,
            ctrl_h,
            slider_min_w: 40.0,
            bar_h: (row_h - 6.0).max(2.0),
            bar_radius: 3.0,
            ruler_h: 18.0,
            tick_min_px: 6.0,
            cache_strip_h: 2.0,
            handle_grab: 8.0,
            key_grab_pad: 5.0,
            prio_key: 30,
            prio_tool: 25,
            hover_gamma: 0.3,
            lane_height,
        }
    }
}

impl Default for TimelineMetrics {
    /// Compact preset — the slick default.
    fn default() -> Self {
        Self::for_density(Density::Compact)
    }
}

#[cfg(test)]
mod timeline_metrics_tests {
    use super::{Density, TimelineMetrics};

    #[test]
    fn density_scales_row_height() {
        assert!(
            TimelineMetrics::for_density(Density::Compact).row_h
                < TimelineMetrics::for_density(Density::Comfortable).row_h
        );
    }

    #[test]
    fn bar_never_exceeds_row() {
        for d in [Density::Compact, Density::Cozy, Density::Comfortable] {
            let m = TimelineMetrics::for_density(d);
            assert!(m.bar_h <= m.row_h);
        }
    }

    #[test]
    fn default_is_compact() {
        assert_eq!(
            TimelineMetrics::default(),
            TimelineMetrics::for_density(Density::Compact)
        );
    }
}

// ============================================================================
// List / row helpers (shared selection + highlight)
// ============================================================================

/// Helpers shared by the selectable-list widgets (asset-browser, outliner,
/// jobs-table, prefs sidebar, attribute grids, …) so selection and hover LOOK
/// and BEHAVE identically across the whole set.
pub mod list {
    use super::Semantic;
    use egui::{Painter, Rect};

    /// Paint the standard row background: the `selection` colour when selected,
    /// the `hover` colour when hovered, nothing otherwise. Call before drawing
    /// the row's content so text/icons render on top.
    pub fn row_fill(painter: &Painter, rect: Rect, sem: &Semantic, selected: bool, hovered: bool) {
        if selected {
            painter.rect_filled(rect, 0.0, sem.selection);
        } else if hovered {
            painter.rect_filled(rect, 0.0, sem.hover);
        }
    }

    /// Resolve a click into a new selection — the canonical plain / ctrl(cmd) /
    /// shift model shared by every list widget and host:
    /// - `range` (shift): union the inclusive span from the anchor (or, if none,
    ///   the last-selected item) to `clicked_idx`;
    /// - `additive` (ctrl/cmd): toggle the clicked id, keeping the rest;
    /// - neither: replace the selection with just the clicked id.
    ///
    /// Returns `(new_selection, new_anchor)` with the anchor at `clicked_idx`.
    /// `ordered` is the visible items in display order; `current` the prior
    /// selection. Generic over the id type.
    pub fn resolve_selection<Id: Copy + PartialEq>(
        ordered: &[Id],
        current: &[Id],
        anchor: Option<usize>,
        clicked_idx: usize,
        additive: bool,
        range: bool,
    ) -> (Vec<Id>, Option<usize>) {
        let mut sel = current.to_vec();
        if range {
            let a = anchor
                .or_else(|| {
                    current
                        .last()
                        .and_then(|id| ordered.iter().position(|x| x == id))
                })
                .unwrap_or(clicked_idx);
            let (lo, hi) = if a <= clicked_idx {
                (a, clicked_idx)
            } else {
                (clicked_idx, a)
            };
            for id in ordered.iter().skip(lo).take(hi - lo + 1) {
                if !sel.contains(id) {
                    sel.push(*id);
                }
            }
        } else if additive {
            if let Some(id) = ordered.get(clicked_idx) {
                if let Some(pos) = sel.iter().position(|x| x == id) {
                    sel.remove(pos);
                } else {
                    sel.push(*id);
                }
            }
        } else if let Some(id) = ordered.get(clicked_idx) {
            sel.clear();
            sel.push(*id);
        }
        (sel, Some(clicked_idx))
    }

    #[cfg(test)]
    mod tests {
        use super::resolve_selection;

        #[test]
        fn plain_replaces() {
            let ord = [10u64, 20, 30];
            let (s, a) = resolve_selection(&ord, &[10, 20], None, 2, false, false);
            assert_eq!(s, vec![30]);
            assert_eq!(a, Some(2));
        }

        #[test]
        fn ctrl_toggles() {
            let ord = [10u64, 20, 30];
            let (off, _) = resolve_selection(&ord, &[10, 30], None, 2, true, false);
            assert_eq!(off, vec![10]); // 30 toggled off
            let (on, _) = resolve_selection(&ord, &[10], None, 1, true, false);
            assert_eq!(on, vec![10, 20]); // 20 toggled on
        }

        #[test]
        fn shift_extends_from_anchor() {
            let ord = [10u64, 20, 30, 40];
            let (s, _) = resolve_selection(&ord, &[10], Some(0), 2, false, true);
            assert_eq!(s, vec![10, 20, 30]); // 0..=2
        }
    }
}

// ============================================================================
// Reset gesture (one hand for every value control)
// ============================================================================

/// The widget set's ONE "reset to the default" gesture, so every value control answers the
/// same hand: Ctrl / Cmd + left click everywhere (Nuke's), and also a plain right click on
/// controls that have no context menu of their own (a control whose right click opens a menu,
/// like egui-attr-grid's rows, offers Reset in that menu instead). Before this module the
/// attr grid, the slider and each host spelled the gesture themselves, and they disagreed.
pub mod reset {
    use egui::{Response, Ui};

    /// Ctrl / Cmd + left click on `response`. On a hit, what the same click started in the
    /// widget itself is taken back, so the gesture only resets: the keyboard focus a clicked
    /// text field takes (egui's `DragValue` starts typing on any click), and the popup a menu
    /// button toggles (egui's `ComboBox` / `Popup::menu` open on `clicked()` whatever the
    /// modifiers, under `Popup::default_response_id`; closing an id that is not open is a no-op).
    pub fn ctrl_click(ui: &Ui, response: &Response) -> bool {
        let hit = response.clicked() && ui.input(|i| i.modifiers.command);
        if hit {
            let ctx = ui.ctx();
            ctx.memory_mut(|m| m.surrender_focus(response.id));
            egui::Popup::close_id(ctx, egui::Popup::default_response_id(response));
        }
        hit
    }

    /// [`ctrl_click`] or a right click: the gesture of a control WITHOUT a context menu.
    pub fn requested(ui: &Ui, response: &Response) -> bool {
        ctrl_click(ui, response) || response.secondary_clicked()
    }

    /// Apply [`requested`] to a value control: write `default` into `value` and mark
    /// `response` changed. True only when the value actually moved (a reset at the default is
    /// not an edit).
    pub fn apply<T: PartialEq + Clone>(
        ui: &Ui,
        response: &mut Response,
        value: &mut T,
        default: T,
    ) -> bool {
        if !requested(ui, response) || *value == default {
            return false;
        }
        *value = default;
        response.mark_changed();
        true
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// One headless frame with `events` and held `modifiers` (egui's input state takes
        /// them from `Event::ModifiersChanged`, not from the pointer event), a `DragValue` on
        /// `value`; returns what [`apply`] reported and whether the field holds the keyboard
        /// focus afterwards.
        fn frame(
            ctx: &egui::Context,
            mut events: Vec<egui::Event>,
            modifiers: egui::Modifiers,
            value: &mut f32,
        ) -> (bool, bool) {
            let mut seen = (false, false);
            events.insert(0, egui::Event::ModifiersChanged(modifiers));
            let input = egui::RawInput { events, ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                let mut resp = ui.add(egui::DragValue::new(value));
                let reset = apply(ui, &mut resp, value, 0.0);
                seen = (reset, ui.ctx().memory(|m| m.has_focus(resp.id)));
            });
            out.textures_delta.clear();
            seen
        }

        /// Press and release `button` at the field with `modifiers`; the release frame's result.
        fn click(button: egui::PointerButton, modifiers: egui::Modifiers, value: &mut f32) -> (bool, bool) {
            let ctx = egui::Context::default();
            frame(&ctx, vec![], modifiers, value);
            let pos = egui::pos2(10.0, 10.0);
            let press = |pressed| egui::Event::PointerButton { pos, button, pressed, modifiers };
            frame(&ctx, vec![egui::Event::PointerMoved(pos), press(true)], modifiers, value);
            frame(&ctx, vec![press(false)], modifiers, value)
        }

        #[test]
        fn ctrl_click_resets_without_starting_a_text_edit() {
            let mut v = 3.5;
            let (reset, focused) = click(egui::PointerButton::Primary, egui::Modifiers::COMMAND, &mut v);
            assert!(reset);
            assert_eq!(v, 0.0);
            assert!(!focused, "the same click opened the field for typing");
        }

        /// A Ctrl-click on a ComboBox resets without leaving its menu open; a plain click (the
        /// positive control) does open it.
        #[test]
        fn ctrl_click_on_a_combo_box_resets_without_opening_it() {
            // Press and release at the combo with `modifiers`; (value, whether its popup is open).
            let click = |modifiers: egui::Modifiers| {
                let ctx = egui::Context::default();
                let mut v = 2_u8;
                let frame = |mut events: Vec<egui::Event>, v: &mut u8| {
                    events.insert(0, egui::Event::ModifiersChanged(modifiers));
                    let mut seen = (egui::Rect::NOTHING, false);
                    let mut out = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                        let mut resp = egui::ComboBox::from_id_salt("combo")
                            .selected_text(v.to_string())
                            .show_ui(ui, |ui| {
                                for k in 0..3 {
                                    ui.selectable_value(v, k, k.to_string());
                                }
                            })
                            .response;
                        apply(ui, &mut resp, v, 0);
                        let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
                        seen = (resp.rect, open);
                    });
                    out.textures_delta.clear();
                    seen
                };
                let pos = frame(vec![], &mut v).0.center();
                let press = |pressed| egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers,
                };
                frame(vec![egui::Event::PointerMoved(pos), press(true)], &mut v);
                frame(vec![press(false)], &mut v);
                // The next frame shows what stuck.
                let open = frame(vec![], &mut v).1;
                (v, open)
            };
            assert_eq!(click(egui::Modifiers::COMMAND), (0, false), "Ctrl-click resets, menu stays shut");
            assert_eq!(click(egui::Modifiers::NONE), (2, true), "a plain click opens the menu");
        }

        #[test]
        fn right_click_resets() {
            let mut v = 3.5;
            assert!(click(egui::PointerButton::Secondary, egui::Modifiers::NONE, &mut v).0);
            assert_eq!(v, 0.0);
        }

        #[test]
        fn a_plain_click_is_not_a_reset_and_a_reset_at_the_default_is_no_edit() {
            let mut v = 3.5;
            assert!(!click(egui::PointerButton::Primary, egui::Modifiers::NONE, &mut v).0);
            assert_eq!(v, 3.5);
            let mut at_default = 0.0;
            assert!(!click(egui::PointerButton::Secondary, egui::Modifiers::NONE, &mut at_default).0);
        }
    }
}

// ============================================================================
// Splitter interaction (shared drag/double-click/paint mechanics)
// ============================================================================

/// Shared low-level mechanics for a draggable column/section splitter handle,
/// factored out of egui-splitter-table / egui-statusbar / the former egui-attr-table (three
/// independent, near-identical implementations before this module existed).
/// Each caller keeps its OWN width-storage model (per-instance state vs. a
/// globally-shared persisted width) — only the hit-test/drag/double-click/paint
/// plumbing is shared here.
pub mod splitter {
    use super::Metrics;
    use egui::{Id, Rect, Sense, Stroke, Ui};

    /// Result of one frame's interaction with a splitter handle.
    pub struct SplitterHit {
        /// Horizontal drag delta (px) this frame, if the handle was dragged.
        pub delta: Option<f32>,
        /// Whether the handle was double-clicked this frame (caller resets its
        /// own stored width to a default in response).
        pub double_clicked: bool,
    }

    /// One draggable vertical splitter handle centred at `x`, spanning `rect`'s
    /// y-range. `hit_half_width` is the interactive hit-test half-width (px)
    /// around `x` — callers pass their EXISTING hit geometry so this refactor
    /// changes no crate's current hit-testing footprint. `inset` shortens the
    /// painted line by that many px at both `rect.top()`/`rect.bottom()` (each
    /// caller's prior paint code used a different inset; pass it through rather
    /// than silently normalising the look). Paints with `metrics.stroke_thin` in
    /// the theme's active/noninteractive widget-stroke colour and sets a resize
    /// cursor while hovered/dragged.
    ///
    /// Uses [`Sense::click_and_drag`], deliberately NOT `Sense::drag()`: egui
    /// only sets its internal "clicked" flag when a widget's sense includes
    /// `CLICK` (see `Sense::senses_click`), so a drag-only interact can never
    /// report [`egui::Response::double_clicked`] true. All three splitter
    /// implementations this module replaces called `Sense::drag()` while also
    /// checking `double_clicked()` — their double-click-to-reset was dead code.
    pub fn interact(
        ui: &mut Ui,
        rect: Rect,
        x: f32,
        id: Id,
        hit_half_width: f32,
        inset: f32,
        metrics: &Metrics,
    ) -> SplitterHit {
        let hit = Rect::from_min_max(
            egui::pos2(x - hit_half_width, rect.top()),
            egui::pos2(x + hit_half_width, rect.bottom()),
        );
        let resp = ui.interact(hit, id, Sense::click_and_drag());
        let active = resp.hovered() || resp.dragged();
        paint(ui, rect, x, inset, metrics, active);
        if active {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        SplitterHit {
            delta: resp.dragged().then(|| resp.drag_delta().x),
            double_clicked: resp.double_clicked(),
        }
    }

    /// The line of a vertical splitter at `x` (see [`interact`] for `inset`), without any
    /// interaction: what a locked layout draws, so a frozen divider looks exactly like an idle
    /// live one. `active` paints the hovered/dragged colour.
    pub fn paint(ui: &Ui, rect: Rect, x: f32, inset: f32, metrics: &Metrics, active: bool) {
        let color = if active {
            ui.visuals().widgets.active.fg_stroke.color
        } else {
            ui.visuals().widgets.noninteractive.bg_stroke.color
        };
        ui.painter().line_segment(
            [
                egui::pos2(x, rect.top() + inset),
                egui::pos2(x, rect.bottom() - inset),
            ],
            Stroke::new(metrics.stroke_thin, color),
        );
    }

    /// One draggable **horizontal** splitter handle centred at `y`, spanning `rect`'s
    /// x-range. Mirror of [`interact`] for top|bottom pane splits (attrs|effects).
    /// `hit_half_height` is the interactive hit-test half-height (px) around `y`.
    /// `delta` is the vertical drag this frame (`drag_delta().y`).
    pub fn interact_horizontal(
        ui: &mut Ui,
        rect: Rect,
        y: f32,
        id: Id,
        hit_half_height: f32,
        inset: f32,
        metrics: &Metrics,
    ) -> SplitterHit {
        let hit = Rect::from_min_max(
            egui::pos2(rect.left(), y - hit_half_height),
            egui::pos2(rect.right(), y + hit_half_height),
        );
        let resp = ui.interact(hit, id, Sense::click_and_drag());
        let active = resp.hovered() || resp.dragged();
        let color = if active {
            ui.visuals().widgets.active.fg_stroke.color
        } else {
            ui.visuals().widgets.noninteractive.bg_stroke.color
        };
        ui.painter().line_segment(
            [
                egui::pos2(rect.left() + inset, y),
                egui::pos2(rect.right() - inset, y),
            ],
            Stroke::new(metrics.stroke_thin, color),
        );
        if active {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }
        SplitterHit {
            delta: resp.dragged().then(|| resp.drag_delta().y),
            double_clicked: resp.double_clicked(),
        }
    }
}

// ============================================================================
// Animation helpers (shared easing)
// ============================================================================

/// Thin wrappers over egui's built-in animation clock so every widget eases the
/// same way from one named source instead of hand-rolling per-crate timers. All
/// are frame-clock driven (wasm-safe, no `std::time`) and request a repaint
/// while a value is in flight, so a transition animates even when nothing else
/// drives the UI. A `secs` of `0.0` yields an instant switch.
pub mod anim {
    use egui::{Context, Id};

    /// Eased `0.0..=1.0` factor for a boolean state, transitioning over `secs`.
    /// Use it to cross-fade a colour (`a.lerp_to_gamma(b, t)`), fade an alpha, or
    /// drive a rotation. Wraps [`egui::Context::animate_bool_with_time`].
    pub fn bool_factor(ctx: &Context, id: Id, on: bool, secs: f32) -> f32 {
        ctx.animate_bool_with_time(id, on, secs)
    }

    /// Eased scalar easing toward `target` over `secs` — for a continuously
    /// moving value (playhead x, zoom level). Wraps
    /// [`egui::Context::animate_value_with_time`].
    pub fn value(ctx: &Context, id: Id, target: f32, secs: f32) -> f32 {
        ctx.animate_value_with_time(id, target, secs)
    }
}

// ============================================================================
// Transient widget state
// ============================================================================

/// One way to keep per-widget transient UI state (rename buffers, drag anchors,
/// hover indices, press-select flags, …) in egui memory, keyed by a stable
/// `Id`. Replaces the inconsistent mix of `ui.data(..)`, `ui.memory(|m| m.data..)`
/// and `ctx.data_mut(..)` scattered across widgets with one named API. Transient
/// = frame-to-frame, NOT serialized (use egui's `*_persisted` directly for the
/// rare values that must survive a restart, e.g. a colour history).
pub mod state {
    use egui::{Context, Id};

    /// Read a transient value by id (`None` if absent / wrong type).
    pub fn get<T: Clone + 'static>(ctx: &Context, id: Id) -> Option<T> {
        ctx.data(|d| d.get_temp::<T>(id))
    }

    /// Read a transient value, or `T::default()` if absent.
    pub fn get_or_default<T: Clone + Default + 'static>(ctx: &Context, id: Id) -> T {
        get(ctx, id).unwrap_or_default()
    }

    /// Write a transient value by id.
    pub fn set<T: Clone + Send + Sync + 'static>(ctx: &Context, id: Id, value: T) {
        ctx.data_mut(|d| d.insert_temp(id, value));
    }

    /// Remove a transient value (e.g. when a drag gesture ends).
    pub fn clear<T: 'static>(ctx: &Context, id: Id) {
        ctx.data_mut(|d| d.remove::<T>(id));
    }

    /// Read-modify-write a transient value under a single memory lock.
    /// Missing values start from `T::default()`.
    pub fn update<T: Clone + Send + Sync + Default + 'static>(
        ctx: &Context,
        id: Id,
        f: impl FnOnce(&mut T),
    ) {
        ctx.data_mut(|d| {
            let mut v = d.get_temp::<T>(id).unwrap_or_default();
            f(&mut v);
            d.insert_temp(id, v);
        });
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn roundtrip_and_clear() {
            let ctx = Context::default();
            let id = Id::new("t");
            assert_eq!(get::<i32>(&ctx, id), None);
            assert_eq!(get_or_default::<i32>(&ctx, id), 0);
            set(&ctx, id, 7i32);
            assert_eq!(get::<i32>(&ctx, id), Some(7));
            update::<i32>(&ctx, id, |v| *v += 5);
            assert_eq!(get::<i32>(&ctx, id), Some(12));
            clear::<i32>(&ctx, id);
            assert_eq!(get::<i32>(&ctx, id), None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let c = WidgetsConfig::default();
        assert!(c.fonts.xs < c.fonts.sm && c.fonts.sm < c.fonts.md && c.fonts.md < c.fonts.lg);
        assert!(c.metrics.stroke_thin < c.metrics.stroke_thick);
        assert!(c.metrics.radius_sm <= c.metrics.radius_md);
    }

    #[test]
    fn semantic_takes_theme_then_palette() {
        let dark = Visuals::dark();
        let pal = Palette::default();
        let s = Semantic::resolve(&dark, &pal);
        assert_eq!(s.selection, dark.selection.bg_fill); // from theme
        assert_eq!(s.accent, pal.accent); // from palette
    }
}
