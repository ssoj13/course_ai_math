//! # egui-ramp
//!
//! Reusable, **generic** ramp editors for [`egui`] 0.36, modelled on the
//! ubiquitous DCC controls — a Houdini ramp, a Nuke LUT, Maya's `remapValue`:
//!
//! * [`curve_edit`] — an **animation / remap curve**. A framed box over
//!   [`CurveConfig::x_range`] × [`CurveConfig::y_range`] (both `[0,1]` by
//!   default) holds draggable keys (`t` = input, `v` = output) of a [`Track`].
//!   Sample it with [`Track::eval`]. Every key has two tangent [`Side`]s
//!   ([`Tan`]: Smooth, Linear, Constant, Flat, Catmull-Rom, User) whose handles
//!   are draggable. The maths come from the standalone [`curves`] crate,
//!   re-exported here; the editors are generic over its [`Real`] (`f32` or `f64`).
//! * [`curves_edit`] — the same plot for a SET of tracks (an RGB curve, an OpenFX
//!   Parametric parameter): a channel selector, numeric fields for the selected
//!   key, a Reset to the caller's defaults and a hint line listing the gestures
//!   ([`CURVE_HINT`]). It reports whether an edit is in progress or finished
//!   ([`EditPhase`]), so a caller with undo sends one value per gesture.
//! * [`gradient_edit`] — a **colour GRADIENT / ramp**. A horizontal bar painted
//!   from [`Gradient::sample`] (one [`Track`] per colour channel), with draggable
//!   [`GradientStop`] markers beneath it and a colour picker for the selected stop.
//!
//! ## Curve gestures (one set for both curve editors)
//!
//! * hover a key: highlighted, grab cursor; press-drag it: move it (between its
//!   neighbours); **Ctrl**-drag: fine (a tenth of the pointer motion);
//!   **Shift**-drag: only horizontally or only vertically, whichever moved more;
//! * click a key: select it (its tangent handles appear); **Shift**-click: toggle
//!   the segment after it (before it, for the last key) between `Linear` and the
//!   [`CurveConfig::new_tan`] kind (a linear segment's key is drawn as a square);
//! * drag a tangent handle: shape that side (`Track::set_handle`; the opposite
//!   side follows while the key is unbroken); **Shift**-drag breaks the key first,
//!   so the two sides move independently;
//! * **double-click** off the keys: add a key ON the curve (its current value at
//!   that `t`, tangent [`CurveConfig::new_tan`]), selected;
//! * **right-click** a key, or **Delete** / Backspace with the plot focused:
//!   delete it (never below [`CurveConfig::min_knots`]).
//!
//! ## Immediate-mode contract
//!
//! The widgets take a `&mut` to the caller-owned data ([`Track`] / [`Gradient`]),
//! **mutate it in place**, and return an [`egui::Response`]. `response.changed()`
//! is `true` on the frames where the user edited the data. The widgets own **no**
//! data of their own: the selection, a drag in progress and the active channel are
//! transient UI state in [`egui::Memory`], keyed by the widget's [`egui::Id`].
//! A track refuses an invalid edit and stays unchanged, so a refused gesture is
//! simply no edit.
//!
//! ## Dependencies
//!
//! `egui-widgets-config` for shared metrics, `curves` for the track maths, and
//! `egui-colorpicker` for the gradient stop's colour swatch — the same
//! Maya/Nuke-style picker every other colour swatch in this workspace uses,
//! instead of egui's bare [`egui::Ui::color_edit_button_srgba`].
//!
//! ## Example
//!
//! ```no_run
//! use egui_ramp::{CurveConfig, Gradient, GradientConfig, Tan, Track};
//!
//! # fn ui_example(ui: &mut egui::Ui, track: &mut Track, gradient: &mut Gradient) {
//! // Remap curve: the tangent kinds live in the keys.
//! if egui_ramp::curve_edit(ui, track, &CurveConfig::default()).changed() {
//!     // `track` was edited this frame; e.g. re-evaluate a lookup table.
//!     let _y = track.eval(0.5);
//! }
//!
//! // Colour gradient: draws the ramp bar + stop markers, mutates `gradient`.
//! if egui_ramp::gradient_edit(ui, gradient, &GradientConfig::default()).changed() {
//!     let _c = gradient.sample(0.25, Tan::Linear);
//! }
//! # }
//! ```

use std::ops::RangeInclusive;

use std::borrow::Cow;

use egui_widgets_config::{AttrMetrics, icon_button_tip, icons, segmented_idx};

use egui::{
    Color32, CornerRadius, CursorIcon, Id, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind,
    Ui, Vec2,
};

// ===========================================================================
// A. Value-remap CURVE
// ===========================================================================

/// The track maths (keys, tangents, evaluation) live in the standalone [`curves`]
/// crate so they stay UI-agnostic and reusable. We re-export the types here so
/// consumers find them under `egui_ramp::*`.
pub use curves::{Extrap, Key, Real, Side, SideId, Tan, Track};

/// The tangent kinds offered for the colour GRADIENT selector: the gradient has
/// no handles, so only the kinds that need none (Smooth never overshoots,
/// Catmull-Rom may; Constant is a hard step).
pub const GRADIENT_KINDS: [Tan; 4] = [Tan::Linear, Tan::Smooth, Tan::Constant, Tan::CatmullRom];

/// The gesture summary shown as the plot's hover tooltip (the gestures of every
/// curve editor here, see the crate docs).
pub const CURVE_HINT: &str = "drag: move (Ctrl fine, Shift one axis) · double-click: add · \
     right-click / Del: delete · Shift-click: linear segment · handles: drag (Shift: break)";

/// Look & behaviour of [`curve_edit`] / [`curves_edit`].
#[derive(Debug, Clone)]
pub struct CurveConfig {
    /// Horizontal domain the box maps to (left = `*start()`, right = `*end()`).
    /// Key times stay inside it. Default `0.0..=1.0`.
    pub x_range: RangeInclusive<f64>,
    /// Vertical value range the box maps to (top = `*end()`, bottom = `*start()`).
    /// A dragged key's value is clamped to this. Default `0.0..=1.0`.
    pub y_range: RangeInclusive<f64>,
    /// Box width in points; `None` fills the available width.
    pub width: Option<f32>,
    /// Box height in points.
    pub height: f32,
    /// Draw a faint reference grid behind the curve.
    pub grid: bool,
    /// Keep the first and last key at the domain ends (a ramp's endpoints); off,
    /// every key moves freely between its neighbours.
    pub pin_ends: bool,
    /// Deletion never leaves fewer keys than this. Default 2.
    pub min_knots: usize,
    /// The tangent kind of a key added by double-click (the host's "new key
    /// type"); also what a Shift-clicked linear segment returns to. Default
    /// [`Tan::Smooth`].
    pub new_tan: Tan,
    /// The curve colour of [`curve_edit`]; `None` = the widget palette's accent (the
    /// colour of the attribute grid's slider fill and selection).
    pub color: Option<Color32>,
    /// Radius / half-size (points) of a tangent handle marker.
    pub handle_size: f32,
    /// Marker shape for tangent handles (circle or triangle).
    pub handle_shape: HandleShape,
    /// Line style for the tangent connector lines (solid / dashed / dash-dot).
    pub tangent_line: TangentLine,
    /// Colour of the tangent handles + their connector lines. `None` = derive a
    /// lighter tint of the curve colour so the handles read as distinct from it.
    pub handle_color: Option<Color32>,
    /// Control sizes of the attribute grid: the icon buttons, numeric fields and the channel
    /// strip of [`curves_edit`] follow them. Pass the host's grid metrics to match the panel.
    pub metrics: AttrMetrics,
}

impl Default for CurveConfig {
    fn default() -> Self {
        Self {
            x_range: 0.0..=1.0,
            y_range: 0.0..=1.0,
            width: None,
            height: 160.0,
            grid: true,
            pin_ends: true,
            min_knots: 2,
            new_tan: Tan::Smooth,
            color: None,
            handle_size: 3.5,
            handle_shape: HandleShape::Circle,
            tangent_line: TangentLine::Solid,
            handle_color: None,
            metrics: AttrMetrics::default(),
        }
    }
}

/// `rect` inset by `pad` on each side (never past its centre): the area the centres of markers
/// map into. A marker centred on the edge of a framed area (an end key, an end gradient stop)
/// would otherwise be cut in half by the painter's clip and half unclickable, so curve and
/// gradient both map their data into this rect and keep every marker whole inside the
/// allocated one.
fn marker_rect(rect: Rect, pad: Vec2) -> Rect {
    rect.shrink2(pad.min(rect.size() * 0.5))
}

/// How far inside the plot frame a curve key / tangent handle centre stays: the biggest marker,
/// the selected key's enlarged ring with its outline, or the handle shape.
fn marker_pad(cfg: &CurveConfig) -> f32 {
    (HANDLE_R + 1.5 + 1.0).max(cfg.handle_size * 1.5) + 1.0
}

impl CurveConfig {
    /// The rectangle into which the curve domain maps, inside the plot response's frame.
    /// Background painters and host input use it to align with keys and tangent handles.
    pub fn data_rect(&self, frame: Rect) -> Rect {
        marker_rect(frame, Vec2::splat(marker_pad(self)))
    }
}

/// Corner radius of the plot frame: the attribute grid's segmented strip and fields use 4.
const FRAME_R: u8 = 4;
/// Pixel radius of a curve handle's circle.
const HANDLE_R: f32 = 5.0;
/// Pointer pick radius for grabbing a handle (a bit larger than the circle so
/// the handle is easy to hit).
const HANDLE_PICK_R: f32 = HANDLE_R + 4.0;
/// How far (in points) a gradient marker must be dragged below the strip before
/// it is removed — the "drag a key off the track to delete" gesture.
const DRAG_OUT_DIST: f32 = 24.0;
/// The pointer-motion factor of a Ctrl (fine) knot drag.
const FINE: f32 = 0.1;
/// The smallest x gap between neighbouring knots, as a fraction of the domain
/// (keeps the knots strictly increasing, which samplers and hosts rely on).
const GAP: f64 = 1e-4;

/// Marker shape for a tangent handle (configurable via [`CurveConfig`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleShape {
    /// A filled circle.
    Circle,
    /// A small filled triangle.
    Triangle,
}

impl HandleShape {
    /// All shapes, for a selector.
    pub fn all() -> [HandleShape; 2] {
        [HandleShape::Circle, HandleShape::Triangle]
    }
    /// Short label for a selector.
    pub fn label(self) -> &'static str {
        match self {
            HandleShape::Circle => "Circle",
            HandleShape::Triangle => "Triangle",
        }
    }
}

/// Line style for the tangent (handle) connector lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TangentLine {
    /// Continuous line.
    Solid,
    /// Evenly dashed.
    Dashed,
    /// Dash-dot (штрих-пунктир).
    DashDot,
}

impl TangentLine {
    /// All styles, for a selector.
    pub fn all() -> [TangentLine; 3] {
        [
            TangentLine::Solid,
            TangentLine::Dashed,
            TangentLine::DashDot,
        ]
    }
    /// Short label for a selector.
    pub fn label(self) -> &'static str {
        match self {
            TangentLine::Solid => "Solid",
            TangentLine::Dashed => "Dashed",
            TangentLine::DashDot => "Dash-dot",
        }
    }
    /// The repeating on/off dash pattern in points (`[]` = solid).
    fn pattern(self) -> &'static [f32] {
        match self {
            TangentLine::Solid => &[],
            TangentLine::Dashed => &[6.0, 4.0],
            TangentLine::DashDot => &[7.0, 3.5, 1.5, 3.5],
        }
    }
}

/// Lighten an sRGBA colour toward white by `t` (0 = unchanged, 1 = white). Used
/// to tint the tangent handles a bit lighter than the curve so they stand apart.
fn lighten(c: Color32, t: f32) -> Color32 {
    let f = |v: u8| (v as f32 + (255.0 - v as f32) * t).round() as u8;
    Color32::from_rgb(f(c.r()), f(c.g()), f(c.b()))
}

/// Paint a tangent-handle marker at `p` in the configured [`HandleShape`]. For a
/// triangle, `dir` (a unit vector from the handle toward its knot) orients the
/// apex so it points back along the tangent — "into the whisker"; ignored for a
/// circle.
fn paint_handle(
    painter: &egui::Painter,
    p: Pos2,
    size: f32,
    shape: HandleShape,
    dir: Vec2,
    fill: Color32,
    stroke: Stroke,
) {
    match shape {
        HandleShape::Circle => {
            painter.circle(p, size, fill, stroke);
        }
        HandleShape::Triangle => {
            // Isosceles triangle whose apex points along `dir` (toward the knot /
            // along the tangent line); the base sits on the far side.
            let perp = Vec2::new(-dir.y, dir.x);
            let apex = p + dir * size;
            let b1 = p - dir * (size * 0.6) + perp * (size * 0.9);
            let b2 = p - dir * (size * 0.6) - perp * (size * 0.9);
            painter.add(Shape::convex_polygon(vec![apex, b1, b2], fill, stroke));
        }
    }
}

/// Draw `a→b` with a repeating on/off dash `pattern` (lengths in points,
/// `[on, off, on, off, …]`, starting “on”). Empty/zero pattern ⇒ a solid line.
/// Used for the tangent-line styles (dashed / dash-dot).
fn paint_dashes(painter: &egui::Painter, a: Pos2, b: Pos2, stroke: Stroke, pattern: &[f32]) {
    let len = a.distance(b);
    let total: f32 = pattern.iter().sum();
    if pattern.is_empty() || total <= 0.0 || len <= f32::EPSILON {
        painter.line_segment([a, b], stroke);
        return;
    }
    let dir = (b - a) / len;
    let mut t = 0.0_f32;
    let mut idx = 0_usize;
    while t < len {
        let seg = pattern[idx].max(0.01);
        let end = (t + seg).min(len);
        if idx.is_multiple_of(2) {
            // “on” run → draw this slice.
            painter.line_segment([a + dir * t, a + dir * end], stroke);
        }
        t = end;
        idx = (idx + 1) % pattern.len();
    }
}

/// What a curve edit did this frame — so a caller with undo can send ONE value
/// per gesture: keep the edited curves while [`Live`](Self::Live) and commit them
/// on [`Done`](Self::Done).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum EditPhase {
    /// No edit this frame.
    #[default]
    Idle,
    /// The curves changed and the gesture goes on (a knot or a numeric field is
    /// being dragged).
    Live,
    /// A gesture finished this frame (a drag ended; a knot was added, deleted or
    /// toggled; a numeric value was entered; Reset): the curves hold its result.
    Done,
}

/// A caller's drawing under the curves (e.g. an OpenFX plug-in's
/// `kOfxParamPropParametricInteractBackground`): called with the plot's painter
/// and rect after the frame and grid; the rect (the frame inset by the marker margin, where
/// keys map) maps [`CurveConfig::x_range`] ×
/// [`CurveConfig::y_range`] (y up).
pub type Background<'a> = &'a mut dyn FnMut(&egui::Painter, Rect);

/// The plot mapping: `x` over its domain, `y` over its range, into the frame `rect` inset by
/// the marker margin (y up).
#[derive(Clone, Copy, Debug)]
struct Plot {
    /// The plot frame inset by the marker margin: keys and handles map into it.
    inner: Rect,
    x: [f64; 2],
    y: [f64; 2],
}

impl Plot {
    fn new(rect: Rect, cfg: &CurveConfig) -> Self {
        let span = |range: &RangeInclusive<f64>| {
            let (lo, hi) = (*range.start(), *range.end());
            [lo, if hi > lo { hi } else { lo + 1.0 }]
        };
        Self {
            inner: cfg.data_rect(rect),
            x: span(&cfg.x_range),
            y: span(&cfg.y_range),
        }
    }

    /// The screen point of data `(x, y)` (not clamped to the rect).
    fn screen(self, x: f64, y: f64) -> Pos2 {
        let u = (x - self.x[0]) / (self.x[1] - self.x[0]);
        let v = (y - self.y[0]) / (self.y[1] - self.y[0]);
        Pos2::new(
            self.inner.left() + u as f32 * self.inner.width(),
            self.inner.bottom() - v as f32 * self.inner.height(),
        )
    }

    /// The data point of screen `p` (not clamped).
    fn data(self, p: Pos2) -> (f64, f64) {
        let u = f64::from((p.x - self.inner.left()) / self.inner.width().max(1.0));
        let v = f64::from((self.inner.bottom() - p.y) / self.inner.height().max(1.0));
        (
            self.x[0] + u * (self.x[1] - self.x[0]),
            self.y[0] + v * (self.y[1] - self.y[0]),
        )
    }

    fn key<T: Real>(self, k: &Key<T>) -> Pos2 {
        self.screen(k.t().to_f64(), k.v().to_f64())
    }

    /// The screen point of a tangent handle `(dt, dv)` away from key `k`.
    fn handle<T: Real>(self, k: &Key<T>, (dt, dv): (T, T)) -> Pos2 {
        self.screen((k.t() + dt).to_f64(), (k.v() + dv).to_f64())
    }

    /// The smallest time gap between neighbouring keys.
    fn gap(self) -> f64 {
        (self.x[1] - self.x[0]) * GAP
    }
}

/// What the active press-drag is editing, latched at drag-start. `Knot` moves a
/// key; `Handle` edits one tangent side of that key. Indices refer into
/// `track.keys()`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum DragTarget {
    /// Move key `i`.
    Knot(usize),
    /// Edit side `id` of key `i`'s tangent handle.
    Handle(usize, SideId),
}

/// A drag in progress: its target, the dragged key's screen position at the
/// press, and the pointer motion so far (already scaled for Ctrl).
#[derive(Debug, Clone, Copy)]
struct Drag {
    target: DragTarget,
    origin: Pos2,
    motion: Vec2,
    moved: bool,
}

/// The plot's transient UI state (egui memory, keyed by the plot's id).
#[derive(Debug, Clone, Copy, Default)]
struct PlotState {
    /// The track the selection and drag belong to.
    curve: usize,
    selected: Option<usize>,
    drag: Option<Drag>,
}

/// What [`plot`] reports.
struct PlotOutput {
    response: Response,
    phase: EditPhase,
    selected: Option<usize>,
}

/// The bounds a key's time may take: strictly between its neighbours (by the
/// plot's gap) and inside the domain.
fn x_bounds<T: Real>(track: &Track<T>, i: usize, plot: Plot) -> (f64, f64) {
    let gap = plot.gap();
    let keys = track.keys();
    let lo = i
        .checked_sub(1)
        .and_then(|j| keys.get(j))
        .map_or(plot.x[0], |k| k.t().to_f64() + gap);
    let hi = keys.get(i + 1).map_or(plot.x[1], |k| k.t().to_f64() - gap);
    (lo, hi)
}

/// Move key `i` toward `(x, y)`: time between its neighbours (ends pinned with
/// [`CurveConfig::pin_ends`]), value inside the value range. `true` if it moved.
///
/// A move the track refuses (it validates every edit) leaves the track and the
/// index unchanged, so it is just no movement.
fn move_knot<T: Real>(
    track: &mut Track<T>,
    i: usize,
    (x, y): (f64, f64),
    plot: Plot,
    cfg: &CurveConfig,
) -> bool {
    let (lo, hi) = x_bounds(track, i, plot);
    let last = track.len().saturating_sub(1);
    let Some((t0, v0)) = track.keys().get(i).map(|k| (k.t(), k.v())) else {
        return false;
    };
    let x = if cfg.pin_ends && i == 0 {
        plot.x[0]
    } else if cfg.pin_ends && i == last {
        plot.x[1]
    } else if lo <= hi {
        x.clamp(lo, hi)
    } else {
        t0.to_f64()
    };
    let (t, v) = (T::of(x), T::of(y.clamp(plot.y[0], plot.y[1])));
    let i = if t == t0 {
        i
    } else {
        track.move_key(i, t).unwrap_or(i)
    };
    if v != v0 {
        track.set_value(i, v).ok();
    }
    track
        .keys()
        .get(i)
        .is_some_and(|k| k.t() != t0 || k.v() != v0)
}

/// The segment key `i` stands for in the linear toggle: the one after it, or the
/// one arriving at it for the last key.
fn seg_of<T: Real>(track: &Track<T>, i: usize) -> Option<usize> {
    if i + 1 < track.len() {
        Some(i)
    } else {
        i.checked_sub(1)
    }
}

/// Segment `s` is a straight line when both of its handles are `Linear`.
fn is_linear<T: Real>(track: &Track<T>, s: usize) -> bool {
    match (track.keys().get(s), track.keys().get(s + 1)) {
        (Some(a), Some(b)) => a.out.kind == Tan::Linear && b.inn.kind == Tan::Linear,
        _ => false,
    }
}

/// Toggle the segment of key `i` between `Linear` and `back`. `set_seg` restores
/// the original coupling when the neighbours' kind comes back.
fn toggle_linear<T: Real>(track: &mut Track<T>, i: usize, back: Tan) -> bool {
    let Some(s) = seg_of(track, i) else {
        return false;
    };
    let to = if is_linear(track, s) {
        back
    } else {
        Tan::Linear
    };
    track.set_seg(s, to).is_ok()
}

/// The tangent sides of key `i` an editor shows, with their `(dt, dv)` offsets:
/// not the dead sides (no segment) and not holds (a step has no handle).
fn handles<T: Real>(track: &Track<T>, i: usize) -> [Option<(SideId, (T, T))>; 2] {
    let side = |id: SideId| {
        let k = track.keys().get(i)?;
        let kind = match id {
            SideId::In => k.inn.kind,
            SideId::Out => k.out.kind,
        };
        (kind != Tan::Constant)
            .then(|| track.handle(i, id).map(|h| (id, h)))
            .flatten()
    };
    [side(SideId::In), side(SideId::Out)]
}

/// The tangent kind shown for key `i`: its out side, or the in side for the last key.
fn key_tan<T: Real>(track: &Track<T>, i: usize) -> Option<Tan> {
    let k = track.keys().get(i)?;
    Some(if i + 1 < track.len() {
        k.out.kind
    } else {
        k.inn.kind
    })
}

/// The plot of `tracks` with `active` editable (see the crate docs for the
/// gestures). `colors[i]` draws track `i`.
fn plot<T: Real>(
    ui: &mut Ui,
    tracks: &mut [Track<T>],
    active: usize,
    colors: &[Color32],
    cfg: &CurveConfig,
    background: Option<Background<'_>>,
) -> PlotOutput {
    let width = cfg.width.unwrap_or_else(|| ui.available_width());
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, cfg.height), Sense::click_and_drag());
    let enabled = ui.is_enabled();
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, enabled, "curve plot"));
    let id = response.id;
    let mut state: PlotState = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    if state.curve != active {
        state = PlotState {
            curve: active,
            ..PlotState::default()
        };
    }
    let plot = Plot::new(rect, cfg);
    let visuals = ui.visuals().clone();
    let painter = ui.painter_at(rect);
    // Stroke widths come from the shared widget metrics (token source) rather than
    // hard-coded literals, so the ramp stays visually coherent with the other widgets.
    let m = egui_widgets_config::WidgetsConfig::get(ui).metrics;

    painter.rect_filled(rect, CornerRadius::same(FRAME_R), visuals.extreme_bg_color);
    painter.rect_stroke(
        rect,
        CornerRadius::same(FRAME_R),
        visuals.widgets.noninteractive.bg_stroke,
        StrokeKind::Inside,
    );
    if cfg.grid {
        let grid_stroke = Stroke::new(
            m.stroke_thin,
            visuals.widgets.noninteractive.bg_stroke.color,
        );
        for i in 1..4 {
            let f = i as f32 / 4.0;
            let gx = plot.inner.left() + f * plot.inner.width();
            let gy = plot.inner.top() + f * plot.inner.height();
            painter.line_segment(
                [Pos2::new(gx, rect.top()), Pos2::new(gx, rect.bottom())],
                grid_stroke,
            );
            painter.line_segment(
                [Pos2::new(rect.left(), gy), Pos2::new(rect.right(), gy)],
                grid_stroke,
            );
        }
    }
    if let Some(background) = background {
        background(&painter, plot.inner);
    }

    let (shift, ctrl) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
    let mut phase = EditPhase::Idle;
    let mut hover = None;
    if let Some(track) = tracks.get_mut(active) {
        let edit = interact(ui, &response, track, plot, cfg, &mut state, shift, ctrl);
        phase = edit.0;
        hover = edit.1;
    }

    // Curves (painted AFTER editing so there is no one-frame lag between a drag
    // and the drawn curve), sampled so the curved segments show their shape.
    let fallback = cfg
        .color
        .unwrap_or_else(|| egui_widgets_config::semantic(ui).accent);
    let color = |i: usize| colors.get(i).copied().unwrap_or(fallback);
    let samples = (rect.width().round() as usize).clamp(2, 256);
    for (i, track) in tracks.iter().enumerate() {
        let poly: Vec<Pos2> = (0..=samples)
            .map(|s| {
                let x = plot.x[0] + (plot.x[1] - plot.x[0]) * s as f64 / samples as f64;
                plot.screen(x, track.eval(T::of(x)).to_f64())
            })
            .collect();
        let width = if i == active { 2.0 } else { 1.0 };
        painter.add(Shape::line(poly, Stroke::new(width, color(i))));
        if i != active {
            for k in track.keys() {
                painter.circle_filled(plot.key(k), 2.5, color(i));
            }
        }
    }
    if let Some(track) = tracks.get(active) {
        paint_active(
            &painter,
            track,
            plot,
            cfg,
            &visuals,
            (m.stroke_thin, m.stroke_thick),
            (color(active), egui_widgets_config::semantic(ui).selection),
            &state,
            hover,
        );
    }

    let selected = state.selected;
    ui.data_mut(|d| d.insert_temp(id, state));
    let mut response = response;
    if phase != EditPhase::Idle {
        response.mark_changed();
    }
    PlotOutput {
        response,
        phase,
        selected,
    }
}

/// Resolve this frame's gesture on `track`; the phase and the hovered key.
#[allow(clippy::too_many_arguments)]
fn interact<T: Real>(
    ui: &Ui,
    response: &Response,
    track: &mut Track<T>,
    plot: Plot,
    cfg: &CurveConfig,
    state: &mut PlotState,
    shift: bool,
    ctrl: bool,
) -> (EditPhase, Option<usize>) {
    let mut phase = EditPhase::Idle;
    // The key nearest the pointer within the pick radius, with its distance.
    let near_key = |track: &Track<T>, pos: Pos2| -> Option<(usize, f32)> {
        track
            .keys()
            .iter()
            .enumerate()
            .map(|(i, k)| (i, plot.key(k).distance(pos)))
            .filter(|(_, d)| *d <= HANDLE_PICK_R)
            .min_by(|a, b| a.1.total_cmp(&b.1))
    };
    let hit_key = |track: &Track<T>, pos: Pos2| near_key(track, pos).map(|(i, _)| i);
    // Tangent handles: only the selected key's, the closest within the pick radius.
    let tan_pick = cfg.handle_size + 4.0;
    let selected = state.selected;
    let near_tan = |track: &Track<T>, pos: Pos2| -> Option<(DragTarget, f32)> {
        let i = selected?;
        let key = track.keys().get(i)?;
        handles(track, i)
            .into_iter()
            .flatten()
            .map(|(id, h)| (DragTarget::Handle(i, id), plot.handle(key, h).distance(pos)))
            .filter(|(_, d)| *d <= tan_pick)
            .min_by(|a, b| a.1.total_cmp(&b.1))
    };
    // A press takes whichever of key and handle is nearer (a tie: the key), so a
    // short handle lying over its key never makes the key ungrabbable.
    let hit_drag = |track: &Track<T>, pos: Pos2| -> Option<DragTarget> {
        match (near_tan(track, pos), near_key(track, pos)) {
            (Some((t, dt)), Some((_, dk))) if dt < dk => Some(t),
            (_, Some((i, _))) => Some(DragTarget::Knot(i)),
            (tan, None) => tan.map(|(t, _)| t),
        }
    };
    // Drag: latched on the key / handle under the PRESS (egui reports the start
    // only after the pointer moved past its threshold).
    if response.drag_started() {
        response.request_focus();
        let press = ui.input(|i| i.pointer.press_origin());
        let target = press.and_then(|p| hit_drag(track, p));
        state.drag = match (target, press, response.interact_pointer_pos()) {
            (Some(target), Some(press), Some(now)) => {
                let origin = match target {
                    DragTarget::Knot(i) => {
                        state.selected = Some(i);
                        track.keys().get(i).map_or(press, |k| plot.key(k))
                    }
                    DragTarget::Handle(..) => press,
                };
                let scale = if ctrl { FINE } else { 1.0 };
                Some(Drag {
                    target,
                    origin,
                    motion: (now - press) * scale,
                    moved: false,
                })
            }
            _ => None,
        };
    } else if response.dragged()
        && let Some(drag) = state.drag.as_mut()
    {
        let scale = if ctrl { FINE } else { 1.0 };
        drag.motion += response.drag_delta() * scale;
    }
    if (response.drag_started() || response.dragged())
        && let Some(drag) = state.drag.as_mut()
    {
        let moved = match drag.target {
            DragTarget::Knot(i) => {
                let mut motion = drag.motion;
                if shift {
                    if motion.x.abs() >= motion.y.abs() {
                        motion.y = 0.0;
                    } else {
                        motion.x = 0.0;
                    }
                }
                move_knot(track, i, plot.data(drag.origin + motion), plot, cfg)
            }
            DragTarget::Handle(i, id) => response
                .interact_pointer_pos()
                .is_some_and(|p| drag_handle(track, i, id, plot.data(p), shift)),
        };
        if moved {
            drag.moved = true;
            phase = EditPhase::Live;
        }
    }
    if response.drag_stopped()
        && let Some(drag) = state.drag.take()
        && drag.moved
    {
        phase = EditPhase::Done;
    }

    let pointer = response.interact_pointer_pos();
    // Click: select (Shift: toggle the key's segment linear); off the keys: deselect.
    if response.clicked()
        && let Some(pos) = pointer
    {
        response.request_focus();
        match hit_key(track, pos) {
            Some(i) => {
                state.selected = Some(i);
                let back = if cfg.new_tan == Tan::Linear {
                    Tan::Smooth
                } else {
                    cfg.new_tan
                };
                if shift && toggle_linear(track, i, back) {
                    phase = EditPhase::Done;
                }
            }
            None if !response.double_clicked() => state.selected = None,
            None => {}
        }
    }
    // Double-click off the keys: a new key ON the curve at that time.
    if response.double_clicked()
        && let Some(pos) = pointer
        && hit_key(track, pos).is_none()
    {
        let x = plot.data(pos).0.clamp(plot.x[0], plot.x[1]);
        let free = track
            .keys()
            .iter()
            .all(|k| (k.t().to_f64() - x).abs() > plot.gap());
        let y = track.eval(T::of(x));
        // A refused add leaves the track unchanged: no key, no edit.
        if free && let Ok(i) = track.add(T::of(x), y, cfg.new_tan) {
            state.selected = Some(i);
            phase = EditPhase::Done;
        }
    }
    // Delete: right-click a key, or Delete / Backspace on the focused plot.
    let doomed = if response.secondary_clicked() {
        pointer.and_then(|pos| hit_key(track, pos))
    } else if response.has_focus()
        && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
    {
        state.selected
    } else {
        None
    };
    if let Some(i) = doomed
        && track.len() > cfg.min_knots
        && track.remove(i).is_ok()
    {
        state.selected = None;
        state.drag = None;
        phase = EditPhase::Done;
    }
    if state.selected.is_some_and(|i| i >= track.len()) {
        state.selected = None;
    }

    let hover = response.hover_pos().and_then(|p| hit_key(track, p));
    if state.drag.is_some() {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    } else if hover.is_some() {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    }
    (phase, hover)
}

/// Point side `id` of key `i`'s handle at data `(px, py)` (`Track::set_handle`).
/// While the key is unbroken the opposite side follows; Shift breaks the key
/// first so the sides are independent. `true` if the track took the edit.
fn drag_handle<T: Real>(
    track: &mut Track<T>,
    i: usize,
    id: SideId,
    (px, py): (f64, f64),
    shift: bool,
) -> bool {
    let Some((t, v, broken)) = track.keys().get(i).map(|k| (k.t(), k.v(), k.broken)) else {
        return false;
    };
    if shift && !broken && track.set_broken(i, true).is_err() {
        return false;
    }
    let offset = (T::of(px - t.to_f64()), T::of(py - v.to_f64()));
    track.set_handle(i, id, offset).is_ok()
}

/// Paint the editable track's selected-key tangent handles and all keys: a
/// circle for a key on a smooth segment, a square for a linear one; the hovered
/// key grows, the selected one is ringed.
#[allow(clippy::too_many_arguments)]
fn paint_active<T: Real>(
    painter: &egui::Painter,
    track: &Track<T>,
    plot: Plot,
    cfg: &CurveConfig,
    visuals: &egui::Visuals,
    (thin, thick): (f32, f32),
    (color, sel): (Color32, Color32),
    state: &PlotState,
    hover: Option<usize>,
) {
    if let Some(i) = state.selected
        && let Some(key) = track.keys().get(i)
    {
        // Handles + their lines render a bit lighter than the curve so they read
        // as a separate layer (override via `cfg.handle_color`).
        let h_col = cfg.handle_color.unwrap_or_else(|| lighten(color, 0.4));
        let stroke = Stroke::new(thin, h_col);
        let pat = cfg.tangent_line.pattern();
        let key_px = plot.key(key);
        // Unit vector from a handle point back toward its key (apex direction for
        // triangle markers; falls back to “up” for a zero-length handle).
        let dir_to_key = |p: Pos2| -> Vec2 {
            let v = key_px - p;
            let len = v.length();
            if len > 1e-3 {
                v / len
            } else {
                Vec2::new(0.0, -1.0)
            }
        };
        for (_, h) in handles(track, i).into_iter().flatten() {
            let p = plot.handle(key, h);
            paint_dashes(painter, key_px, p, stroke, pat);
            paint_handle(
                painter,
                p,
                cfg.handle_size,
                cfg.handle_shape,
                dir_to_key(p),
                h_col,
                stroke,
            );
        }
    }
    for (i, k) in track.keys().iter().enumerate() {
        let center = plot.key(k);
        let selected = state.selected == Some(i);
        let r = if hover == Some(i) || selected {
            HANDLE_R + 1.5
        } else {
            HANDLE_R
        };
        let (fill, stroke) = if selected {
            (visuals.selection.stroke.color, Stroke::new(thick, sel))
        } else {
            (color, Stroke::new(thin, visuals.strong_text_color()))
        };
        if seg_of(track, i).is_some_and(|s| is_linear(track, s)) {
            let square = Rect::from_center_size(center, Vec2::splat(2.0 * r * 0.9));
            painter.rect(square, CornerRadius::ZERO, fill, stroke, StrokeKind::Middle);
        } else {
            painter.circle(center, r, fill, stroke);
        }
    }
}

/// Draw and edit one value-remap [`Track`] in a framed box (see the crate docs
/// for the gestures).
///
/// Returns the plot's [`Response`]; `.changed()` is `true` on edit frames.
pub fn curve_edit<T: Real>(ui: &mut Ui, track: &mut Track<T>, cfg: &CurveConfig) -> Response {
    let color = cfg
        .color
        .unwrap_or_else(|| egui_widgets_config::semantic(ui).accent);
    plot(ui, std::slice::from_mut(track), 0, &[color], cfg, None).response
}

/// The most tracks [`curves_edit`] shows as a segmented strip (`R G B A`, `Master R G B`);
/// more would not fit the panel width, so they get a dropdown.
pub const STRIP_MAX: usize = 4;

/// An icon button of the attribute grid's look: a square `icon` glyph (side from
/// `metrics.icon_side`) named `name` for AccessKit and the tooltip `tip`, never a text button.
fn icon_btn(ui: &mut Ui, metrics: AttrMetrics, icon: &str, name: &str, tip: &str) -> Response {
    let side = metrics.normalized().icon_side;
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
    let enabled = ui.is_enabled();
    icon_button_tip(ui, rect, icon, name, tip, enabled)
}

/// A set of tracks edited together by [`curves_edit`] (an RGB curve, the curves
/// of an OpenFX Parametric parameter).
pub struct CurveSet<'a, T: Real> {
    /// The tracks, edited in place.
    pub curves: &'a mut [Track<T>],
    /// The channel names (selector entries, host-provided: `R`, `G`, `B`, `Master`...); a
    /// missing one shows `curve <i>`.
    pub labels: &'a [String],
    /// The colour of each track; a missing one takes [`CurveConfig::color`].
    pub colors: &'a [Color32],
    /// What Reset restores (all tracks); `None` hides the reset icon button.
    pub defaults: Option<&'a [Track<T>]>,
}

/// What [`curves_edit`] reports: the plot's response and this frame's
/// [`EditPhase`] over every part of the editor.
pub struct CurveOutput {
    /// The plot's response.
    pub response: Response,
    /// The edit this frame: commit the tracks on [`EditPhase::Done`].
    pub phase: EditPhase,
}

/// Edit a set of tracks: a channel selector (more than one track: the grid's segmented
/// strip for up to [`STRIP_MAX`] tracks, a dropdown beyond), the plot (all tracks drawn, the
/// selected channel editable), a help icon next to the selector with [`CURVE_HINT`] as its
/// tooltip, numeric time / value
/// fields for the selected key, and a Reset icon button restoring [`CurveSet::defaults`].
/// Sizes follow [`CurveConfig::metrics`].
///
/// `id` keys the editor's UI state (unique per editor); `background` draws
/// under the curves.
pub fn curves_edit<T: Real>(
    ui: &mut Ui,
    id: Id,
    set: CurveSet<'_, T>,
    cfg: &CurveConfig,
    background: Option<Background<'_>>,
) -> CurveOutput {
    let CurveSet {
        curves,
        labels,
        colors,
        defaults,
    } = set;
    // A host label borrowed as is; only a missing one is formatted.
    let label = |index: usize| -> Cow<'_, str> {
        match labels.get(index).filter(|label| !label.is_empty()) {
            Some(label) => Cow::Borrowed(label.as_str()),
            None => Cow::Owned(format!("curve {index}")),
        }
    };
    let active_id = id.with("active");
    let live_id = id.with("numeric live");
    ui.push_id(id, |ui| {
        cfg.metrics.apply(ui);
        let mut active: usize = ui.data(|d| d.get_temp(active_id)).unwrap_or(0);
        active = active.min(curves.len().saturating_sub(1));
        // The gesture hint sits right of the channel selector (beside Reset on a single curve),
        // not on the plot, where its tooltip covered the keys.
        let hint_in_header = curves.len() > 1;
        if hint_in_header {
            let side = cfg.metrics.normalized().icon_side;
            let gap = ui.spacing().item_spacing.x;
            let selector_w = (ui.available_width() - side - gap).max(0.0);
            ui.horizontal(|ui| {
                ui.scope(|ui| {
                    ui.set_max_width(selector_w);
                    if curves.len() <= STRIP_MAX {
                        segmented_idx(ui, &mut active, curves.len(), label);
                    } else {
                        let name = label(active);
                        let enabled = ui.is_enabled();
                        let combo = egui::ComboBox::from_id_salt(id.with("channel"))
                            .width(ui.available_width())
                            .selected_text(&*name)
                            .show_ui(ui, |ui| {
                                for index in 0..curves.len() {
                                    ui.selectable_value(&mut active, index, &*label(index));
                                }
                            });
                        // The combo carries its value only; name it for assistive technology.
                        combo.response.widget_info(|| {
                            let mut info = egui::WidgetInfo::labeled(
                                egui::WidgetType::ComboBox,
                                enabled,
                                "Channel",
                            );
                            info.current_text_value = Some(name.to_string());
                            info
                        });
                    }
                });
                icon_btn(ui, cfg.metrics, icons::HELP, "Curve gestures", CURVE_HINT);
            });
        }
        let out = plot(ui, curves, active, colors, cfg, background);
        let mut phase = out.phase;
        let plot_rect = Plot::new(out.response.rect, cfg);
        ui.horizontal(|ui| {
            let mut live: bool = ui.data(|d| d.get_temp(live_id)).unwrap_or(false);
            if let Some(i) = out.selected
                && let Some(track) = curves.get_mut(active)
                && let Some(key) = track.keys().get(i).copied()
            {
                let (lo, hi) = x_bounds(track, i, plot_rect);
                let speed = (plot_rect.x[1] - plot_rect.x[0]) * 1e-3;
                let mut x = key.t().to_f64();
                let mut y = key.v().to_f64();
                let rx = ui.add(
                    egui::DragValue::new(&mut x)
                        .range(lo.min(hi)..=hi.max(lo))
                        .speed(speed)
                        .prefix("x ")
                        .max_decimals(6),
                );
                let ry = ui.add(
                    egui::DragValue::new(&mut y)
                        .speed((plot_rect.y[1] - plot_rect.y[0]) * 1e-3)
                        .prefix("y ")
                        .max_decimals(6),
                );
                // A refused edit leaves the track unchanged (see the crate docs).
                if rx.changed() {
                    track.move_key(i, T::of(x)).ok();
                }
                if ry.changed() {
                    track.set_value(i, T::of(y)).ok();
                }
                for response in [&rx, &ry] {
                    if response.changed() {
                        if response.dragged() {
                            live = true;
                            phase = phase.max(EditPhase::Live);
                        } else {
                            phase = EditPhase::Done;
                        }
                    }
                    if response.drag_stopped() && live {
                        live = false;
                        phase = EditPhase::Done;
                    }
                }
                if let Some(tan) = key_tan(track, i) {
                    ui.weak(tan.label());
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(defaults) = defaults
                    && icon_btn(ui, cfg.metrics, icons::RESET, "Reset", "Reset").clicked()
                {
                    for (track, default) in curves.iter_mut().zip(defaults) {
                        track.clone_from(default);
                    }
                    phase = EditPhase::Done;
                }
                if !hint_in_header {
                    icon_btn(ui, cfg.metrics, icons::HELP, "Curve gestures", CURVE_HINT);
                }
            });
            ui.data_mut(|d| d.insert_temp(live_id, live));
        });
        ui.data_mut(|d| d.insert_temp(active_id, active));
        CurveOutput {
            response: out.response,
            phase,
        }
    })
    .inner
}

// ===========================================================================
// B. Colour GRADIENT / ramp
// ===========================================================================

/// A single colour stop of a [`Gradient`]. `pos` is the position along the ramp
/// (kept in `0..=1`); `color` is the sRGBA colour at that position.
//
// Note: NOT `Eq` (it holds an `f32`); `PartialEq` is enough for the
// find-by-value relocation of the selected stop after a re-sort.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradientStop {
    /// Position along the ramp, clamped to `0..=1`. Stops are kept **sorted**.
    pub pos: f32,
    /// sRGBA colour at `pos`.
    pub color: Color32,
}

impl GradientStop {
    /// Construct a stop.
    pub fn new(pos: f32, color: Color32) -> Self {
        Self { pos, color }
    }
}

/// A colour gradient / ramp: a list of [`GradientStop`]s **sorted by `pos`**,
/// sampled with [`Gradient::sample`]. Invariant: at least two stops, positions
/// non-decreasing. The editor / helpers maintain that; if you mutate `stops`
/// directly, call [`Gradient::keep_sorted`].
#[derive(Debug, Clone, PartialEq)]
pub struct Gradient {
    /// The stops, sorted by `pos`. Treat as an ordered set.
    pub stops: Vec<GradientStop>,
}

impl Default for Gradient {
    fn default() -> Self {
        Self::grayscale()
    }
}

impl Gradient {
    /// Alias for [`Gradient::grayscale`]; kept for symmetry with [`Track::new`].
    pub fn new() -> Self {
        Self::grayscale()
    }

    /// A black→white ramp (`pos 0` = black, `pos 1` = white).
    pub fn grayscale() -> Self {
        Self {
            stops: vec![
                GradientStop::new(0.0, Color32::BLACK),
                GradientStop::new(1.0, Color32::WHITE),
            ],
        }
    }

    /// Re-sort stops by `pos`. `f32::total_cmp` avoids NaN-ordering surprises.
    pub fn keep_sorted(&mut self) {
        self.stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
    }

    /// Insert a stop and keep the list sorted by `pos`. Returns its index.
    pub fn add_stop(&mut self, s: GradientStop) -> usize {
        self.stops.push(s);
        self.keep_sorted();
        self.stops
            .iter()
            .position(|q| *q == s)
            .unwrap_or(self.stops.len() - 1)
    }

    /// The four colour channels (premultiplied linear r, g, b, a) as [`Track`]s over
    /// the stop positions, every key with tangent `interp` (a `Constant` stop is a
    /// hard step to the next stop). `None` for an empty gradient.
    ///
    /// Blending in linear space avoids the muddy midpoints of an sRGB-byte lerp.
    /// Positions are clamped to `0..=1` (non-finite reads as 0) and **stably sorted**
    /// here, so a caller that filled `stops` out of order gets the order it meant,
    /// and equal positions keep their given order.
    ///
    /// A track has no two keys at one time, so a tie (several stops at one position
    /// `p`) becomes a hard edge: the FIRST stop of the tie sits just left of `p` with
    /// a `Constant` out side, the LAST stop sits exactly at `p`; stops between them
    /// have zero width and are invisible. Keys never leave `0..=1`: a tie at `0`
    /// collapses to its last stop, a tie at `1` puts its step at `1 - TIE`, so
    /// `sample(1.0)` is always the last stop and `sample(0.0)` the last stop at 0.
    fn ramp(&self, interp: Tan) -> Option<[Track<f32>; 4]> {
        if self.stops.is_empty() {
            return None;
        }
        let mut stops: Vec<(f32, egui::Rgba)> = self
            .stops
            .iter()
            .map(|s| {
                let pos = if s.pos.is_finite() {
                    s.pos.clamp(0.0, 1.0)
                } else {
                    0.0
                };
                (pos, egui::Rgba::from(s.color))
            })
            .collect();
        // `sort_by` is stable: equal positions keep the caller's order.
        stops.sort_by(|a, b| a.0.total_cmp(&b.0));

        let mut keys: [Vec<Key<f32>>; 4] = std::array::from_fn(|_| Vec::new());
        let mut prev = f32::NEG_INFINITY;
        let mut i = 0;
        while i < stops.len() {
            let pos = stops[i].0;
            let run = stops[i..].iter().take_while(|s| s.0 == pos).count();
            let last = stops[i + run - 1].1;
            // Hard edge: the first stop of the tie holds up to `pos`, just left of it,
            // never closer to the previous key than half the gap.
            if run > 1 && pos > 0.0 {
                let first = stops[i].1;
                let at = pos - TIE.min((pos - prev.max(0.0)) * 0.5);
                if at > prev {
                    for (keys, v) in
                        keys.iter_mut()
                            .zip([first.r(), first.g(), first.b(), first.a()])
                    {
                        let mut k = Key::new(at, v).with_tan(interp);
                        k.broken = true;
                        k.out = Side::new(Tan::Constant);
                        keys.push(k);
                    }
                }
            }
            for (keys, v) in keys
                .iter_mut()
                .zip([last.r(), last.g(), last.b(), last.a()])
            {
                keys.push(Key::new(pos, v).with_tan(interp));
            }
            prev = pos;
            i += run;
        }
        // Finite and strictly increasing by construction, so the tracks are valid;
        // a refusal would be a bug here, and shows as a transparent ramp.
        let [r, g, b, a] = keys.map(|keys| Track::from_keys(keys).ok());
        Some([r?, g?, b?, a?])
    }

    /// Sample the gradient at `t` (clamped to `0..=1`): each colour channel is a
    /// [`Track`] of the stops (see [`Gradient::ramp`]) with tangent kind `interp`.
    /// Outside the stop range the nearest stop's colour is returned.
    ///
    /// Empty gradients return transparent; a single stop returns its colour.
    pub fn sample(&self, t: f32, interp: Tan) -> Color32 {
        self.ramp(interp)
            .map_or(Color32::TRANSPARENT, |ramp| ramp_at(&ramp, t))
    }
}

/// How far (in `0..=1` position units) the left side of a hard edge sits before
/// its position: invisible, but keeps the track keys strictly increasing.
const TIE: f32 = 1e-6;

/// The colour of `ramp` (from [`Gradient::ramp`]) at `t`; each channel clamped to
/// the displayable `0..=1` range, since Catmull-Rom may overshoot between stops.
fn ramp_at(ramp: &[Track<f32>; 4], t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let [r, g, b, a] = ramp.each_ref().map(|c| c.eval(t).clamp(0.0, 1.0));
    Color32::from(egui::Rgba::from_rgba_premultiplied(r, g, b, a))
}

/// Look & behaviour of [`gradient_edit`].
#[derive(Debug, Clone)]
pub struct GradientConfig {
    /// Height of the painted ramp bar in points (width fills the space).
    pub height: f32,
    /// Width/height of a stop marker triangle in points.
    pub marker_size: f32,
    /// Tangent kind of the stops — used by the bar paint and by
    /// [`Gradient::sample`] calls the editor makes. Default [`Tan::Linear`].
    /// Only the kinds in [`GRADIENT_KINDS`] make sense here.
    pub interp: Tan,
    /// Control sizes of the attribute grid; the gap before the colour swatch follows its
    /// `component_gap`.
    pub metrics: AttrMetrics,
}

impl Default for GradientConfig {
    fn default() -> Self {
        Self {
            height: 28.0,
            marker_size: 12.0,
            interp: Tan::Linear,
            metrics: AttrMetrics::default(),
        }
    }
}

/// Number of vertical slices used to paint the ramp bar. Each slice is a flat
/// rect of the sampled colour — enough that the banding is invisible.
const RAMP_SLICES: usize = 128;
/// Width of the selected stop's colour swatch right of the bar.
const SWATCH: f32 = 28.0;

/// Draw and edit a colour [`Gradient`] as a horizontal ramp bar with draggable
/// stop markers below it, plus a colour picker for the selected stop.
///
/// Interactions (all mutate `gradient` in place):
/// - **click the ramp bar** (or empty marker strip) → adds a stop at that
///   position, sampling the current colour there; the new stop becomes selected.
/// - **press-drag a marker** → moves its `pos` (clamped to `0..=1`, re-sorted).
/// - **click a marker** → selects it for colour editing.
/// - **right-click** or **double-click a marker**, **press Delete/Backspace**
///   while hovering one, or **drag it far below the strip** → removes it
///   (never below two stops).
/// - the **colour button** (right of the bar) edits the selected stop's colour.
///
/// The selected-stop index is the only transient state; it lives in egui memory
/// keyed by a PER-CALL id (salted with `ui.next_auto_id()`) so two gradients shown
/// in the same `Ui` — whose `ui.id()` is identical — don't share one selection.
/// Returns the unioned [`Response`]; `.changed()` is `true` on edit frames.
pub fn gradient_edit(ui: &mut Ui, gradient: &mut Gradient, cfg: &GradientConfig) -> Response {
    // `ui.next_auto_id()` advances as each call allocates its widgets, so a second
    // `gradient_edit` in the same `Ui` gets a distinct selection-state key.
    let state_id = ui.id().with(("egui_ramp_gradient_sel", ui.next_auto_id()));
    // Selected stop index, recalled from memory and clamped to range each frame.
    let mut sel = ui
        .ctx()
        .memory(|m| m.data.get_temp::<usize>(state_id))
        .unwrap_or(0);
    if !gradient.stops.is_empty() {
        sel = sel.min(gradient.stops.len() - 1);
    }

    let mut changed = false;

    // Lay out: a [bar + marker strip] block, then a colour button to the right.
    let marker_h = cfg.marker_size;
    let gap = 3.0 * cfg.metrics.normalized().component_gap;
    let block_w = (ui.available_width() - (SWATCH + gap + 2.0)).max(40.0);
    let block_h = cfg.height + marker_h + 4.0;
    // One click-and-drag response over the whole [bar + marker strip] block;
    // the gesture is resolved against the marker positions by hand (see the
    // gradient doc-comment). A single sensing widget avoids egui's deferred
    // hit-test starving overlapping sibling widgets, which previously made the
    // bar/markers unresponsive.
    let (block_rect, mut response) =
        ui.allocate_exact_size(Vec2::new(block_w, block_h), Sense::click_and_drag());

    let bar_rect = Rect::from_min_size(block_rect.min, Vec2::new(block_rect.width(), cfg.height));
    let visuals = ui.visuals().clone();
    let painter = ui.painter_at(block_rect);
    // Stroke widths come from the shared widget metrics (token source) rather than
    // hard-coded literals, so the ramp stays visually coherent with the other widgets.
    let m = egui_widgets_config::WidgetsConfig::get(ui).metrics;

    // --- Paint the ramp bar from sample() ----------------------------------
    // Stop positions map into `track`, the bar inset by the marker half-width (so an end marker
    // stays whole inside the block); the bar's end margins keep the end stops' colours.
    let track = marker_rect(bar_rect, Vec2::new(cfg.marker_size * 0.5 + 1.0, 0.0));
    let slices = RAMP_SLICES
        .min(bar_rect.width().round().max(1.0) as usize)
        .max(1);
    let ramp = gradient.ramp(cfg.interp);
    for s in 0..slices {
        let t0 = s as f32 / slices as f32;
        let t1 = (s + 1) as f32 / slices as f32;
        let x0 = bar_rect.left() + t0 * bar_rect.width();
        let x1 = bar_rect.left() + t1 * bar_rect.width();
        // Sample at the slice midpoint for a representative colour.
        let mid = (x0 + x1) * 0.5;
        let pos = ((mid - track.left()) / track.width().max(1.0)).clamp(0.0, 1.0);
        let c = ramp
            .as_ref()
            .map_or(Color32::TRANSPARENT, |r| ramp_at(r, pos));
        let slice = Rect::from_min_max(
            Pos2::new(x0, bar_rect.top()),
            Pos2::new(x1, bar_rect.bottom()),
        );
        painter.rect_filled(slice, CornerRadius::ZERO, c);
    }
    painter.rect_stroke(
        bar_rect,
        CornerRadius::ZERO,
        visuals.widgets.noninteractive.bg_stroke,
        StrokeKind::Inside,
    );

    // pos <-> x mapping along the bar.
    let pos_to_x = |pos: f32| track.left() + pos.clamp(0.0, 1.0) * track.width();
    let x_to_pos = |x: f32| ((x - track.left()) / track.width().max(1.0)).clamp(0.0, 1.0);
    let marker_y = bar_rect.bottom() + 2.0;

    // --- Resolve the active marker, then dispatch the gesture --------------
    let n = gradient.stops.len();
    let half = cfg.marker_size * 0.5;
    let mut remove: Option<usize> = None;

    // Marker nearest the pointer along x, but only while the pointer is within
    // the marker strip band (so a click on the bar above adds instead of
    // grabbing). Takes an explicit slice to avoid borrowing `gradient`.
    let pick_at = |stops: &[GradientStop], pos: Pos2| -> Option<usize> {
        if pos.y < marker_y - 2.0 || pos.y > marker_y + marker_h + 2.0 {
            return None;
        }
        let mut best: Option<(usize, f32)> = None;
        for (i, s) in stops.iter().enumerate() {
            let d = (pos_to_x(s.pos) - pos.x).abs();
            if d <= half + 2.0 && best.is_none_or(|(_, bd)| d < bd) {
                best = Some((i, d));
            }
        }
        best.map(|(i, _)| i)
    };

    let hover_idx = response
        .hover_pos()
        .and_then(|p| pick_at(&gradient.stops, p));

    // MOVE: a drag latched onto a marker at drag-start follows the pointer.
    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
    {
        // Latch the marker under the press; if the press landed on the bar / empty
        // strip, ADD a stop there and grab it (press-drag = add + place). Makes
        // "add" reliable when a press wiggles into a drag instead of a click (the
        // add-only-on-`clicked()` path drops those — the "catches clicks badly"
        // symptom).
        let mut hit = pick_at(&gradient.stops, pos);
        if hit.is_none() && block_rect.contains(pos) {
            let p = x_to_pos(pos.x);
            let c = gradient.sample(p, cfg.interp);
            let idx = gradient.add_stop(GradientStop::new(p, c));
            sel = idx;
            changed = true;
            hit = Some(idx);
        }
        response
            .ctx
            .data_mut(|d| d.insert_temp(response.id.with("ramp_drag"), hit));
    }
    if response.dragged()
        && let Some(ptr) = response.interact_pointer_pos()
    {
        let dragging: Option<usize> = response
            .ctx
            .data(|d| d.get_temp(response.id.with("ramp_drag")))
            .flatten();
        if let Some(i) = dragging
            && i < gradient.stops.len()
        {
            gradient.stops[i].pos = x_to_pos(ptr.x);
            changed = true;
            // REORDER: a stop dragged past a neighbour must swap order. Re-sort by
            // position and follow the moved stop to its NEW index, then RE-LATCH
            // that index so the same stop keeps tracking the pointer across the
            // swap. Without re-latching, the stored drag index would point at a
            // different stop after the reorder and the drag would grab the wrong
            // marker — which is why stops could not be reordered by dragging.
            let moved = gradient.stops[i];
            gradient.keep_sorted();
            let new_i = gradient.stops.iter().position(|s| *s == moved).unwrap_or(i);
            sel = new_i;
            response
                .ctx
                .data_mut(|d| d.insert_temp(response.id.with("ramp_drag"), Some(new_i)));
            // Drag well below the marker strip → remove (keep >= 2 stops).
            if gradient.stops.len() > 2 && ptr.y > marker_y + marker_h + DRAG_OUT_DIST {
                remove = Some(new_i);
            }
        }
    }
    if response.drag_stopped() {
        response
            .ctx
            .data_mut(|d| d.remove::<Option<usize>>(response.id.with("ramp_drag")));
    }

    // SELECT / ADD: a plain click on a marker selects it; a click elsewhere
    // (the bar, or empty marker strip) adds a stop at that x.
    let mut add_at: Option<f32> = None;
    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
        && block_rect.contains(pos)
    {
        if let Some(i) = pick_at(&gradient.stops, pos) {
            sel = i;
        } else {
            add_at = Some(x_to_pos(pos.x));
        }
    }

    // DELETE: right-click or double-click a marker, or press Delete/Backspace
    // while hovering one (keep >= 2 stops).
    if n > 2 && remove.is_none() {
        let del_key =
            ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace));
        if response.secondary_clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            remove = pick_at(&gradient.stops, pos);
        } else if response.double_clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            remove = pick_at(&gradient.stops, pos);
        } else if del_key {
            remove = hover_idx;
        }
    }

    // Apply removal first (indices above stay valid until here).
    if let Some(i) = remove
        && i < gradient.stops.len()
        && gradient.stops.len() > 2
    {
        gradient.stops.remove(i);
        changed = true;
        if sel >= gradient.stops.len() {
            sel = gradient.stops.len().saturating_sub(1);
        }
    }

    // Apply a pending add (re-sort, select the new stop).
    if let Some(pos) = add_at {
        let c = gradient.sample(pos, cfg.interp);
        sel = gradient.add_stop(GradientStop::new(pos, c));
        changed = true;
    }

    // --- Paint the markers (after editing so positions/selection are final) -
    for (i, stop) in gradient.stops.iter().enumerate() {
        let tip = Pos2::new(pos_to_x(stop.pos), marker_y);
        let outline = if i == sel {
            Stroke::new(m.stroke_thick, visuals.selection.stroke.color)
        } else if hover_idx == Some(i) {
            // 1.5 is a deliberate distinct emphasis width (no token for it).
            Stroke::new(1.5, visuals.strong_text_color())
        } else {
            Stroke::new(
                m.stroke_thin,
                visuals.widgets.noninteractive.bg_stroke.color,
            )
        };
        paint_marker(&painter, tip, half, marker_h, stop.color, outline);
    }

    // --- Selected stop's colour picker (right of the bar) ------------------
    if let Some(stop) = gradient.stops.get_mut(sel) {
        let btn_rect = Rect::from_min_size(
            Pos2::new(block_rect.right() + gap, block_rect.top()),
            Vec2::new(SWATCH, cfg.height.min(SWATCH)),
        );
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(btn_rect));
        let mut col = stop.color;
        if egui_colorpicker::color_button_srgba(&mut child, &mut col).changed() {
            stop.color = col;
            changed = true;
        }
        // Union a hover interaction on the swatch button WITHOUT allocating it in
        // the layout. The bar + marker-strip block (allocated above at full height)
        // already governs where following widgets flow. The old `allocate_rect`
        // here pulled the layout cursor UP to this shorter button's bottom, so the
        // next widget landed on the marker strip and overlapped the stop
        // triangles; `interact` reserves nothing in the flow, leaving the cursor
        // below the full block.
        response = response.union(ui.interact(
            btn_rect,
            response.id.with("gradient_swatch_btn"),
            Sense::hover(),
        ));
    }

    // Persist the (clamped) selection for next frame.
    ui.ctx().memory_mut(|m| m.data.insert_temp(state_id, sel));

    if changed {
        response.mark_changed();
    }
    response
}

/// Paint a downward gradient-stop marker: a small house/pin shape with its tip at
/// `tip` (touching the bar), `half` wide, `h` tall, filled with the stop colour.
fn paint_marker(
    painter: &egui::Painter,
    tip: Pos2,
    half: f32,
    h: f32,
    fill: Color32,
    stroke: Stroke,
) {
    let pts = vec![
        tip,                                   // tip at the bar
        Pos2::new(tip.x - half, tip.y + half), // upper-left shoulder
        Pos2::new(tip.x - half, tip.y + h),    // lower-left
        Pos2::new(tip.x + half, tip.y + h),    // lower-right
        Pos2::new(tip.x + half, tip.y + half), // upper-right shoulder
    ];
    painter.add(Shape::convex_polygon(pts, fill, stroke));
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Modifiers, PointerButton};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable as _;

    /// A track over `(t, v)` points, every key `Smooth`.
    fn track(points: &[(f64, f64)]) -> Track<f64> {
        let keys = points.iter().map(|&(t, v)| Key::new(t, v)).collect();
        Track::from_keys(keys).expect("valid keys")
    }

    /// The test editor: one track, how many frames reported an edit.
    struct State {
        track: Track<f64>,
        cfg: CurveConfig,
        edits: usize,
    }

    fn harness(track: Track<f64>) -> Harness<'static, State> {
        let cfg = CurveConfig {
            width: Some(300.0),
            height: 200.0,
            ..CurveConfig::default()
        };
        let mut harness = Harness::builder()
            .with_size(egui::vec2(400.0, 300.0))
            // 60 Hz frames: two clicks a frame apart fall inside egui's double-click delay.
            .with_step_dt(1.0 / 60.0)
            .with_max_steps(8)
            .build_ui_state(
                |ui, state: &mut State| {
                    let State { track, cfg, edits } = state;
                    if curve_edit(ui, track, cfg).changed() {
                        *edits += 1;
                    }
                },
                State {
                    track,
                    cfg,
                    edits: 0,
                },
            );
        harness.run();
        harness
    }

    fn plot_rect(harness: &Harness<'_, State>) -> Rect {
        harness.get_by_label("curve plot").rect()
    }

    /// The screen point of data `(x, y)` on a plot over `[0,1]²`.
    fn at(rect: Rect, x: f64, y: f64) -> Pos2 {
        let rect = marker_rect(rect, Vec2::splat(marker_pad(&CurveConfig::default())));
        Pos2::new(
            rect.left() + x as f32 * rect.width(),
            rect.bottom() - y as f32 * rect.height(),
        )
    }

    fn press(harness: &mut Harness<'_, State>, pos: Pos2, button: PointerButton, pressed: bool) {
        harness.event(Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }

    fn click(harness: &mut Harness<'_, State>, pos: Pos2, b: PointerButton, mods: Modifiers) {
        harness.event(Event::ModifiersChanged(mods));
        harness.event(Event::PointerMoved(pos));
        press(harness, pos, b, true);
        press(harness, pos, b, false);
        harness.event(Event::ModifiersChanged(Modifiers::NONE));
        harness.run();
    }

    /// Press at `from`, move to `to` in steps, release.
    fn drag(harness: &mut Harness<'_, State>, from: Pos2, to: Pos2, mods: Modifiers) {
        harness.event(Event::ModifiersChanged(mods));
        harness.event(Event::PointerMoved(from));
        harness.run();
        press(harness, from, PointerButton::Primary, true);
        harness.run();
        for step in 1..=6 {
            harness.event(Event::PointerMoved(from.lerp(to, step as f32 / 6.0)));
            harness.run();
        }
        press(harness, to, PointerButton::Primary, false);
        harness.event(Event::ModifiersChanged(Modifiers::NONE));
        harness.run();
    }

    fn double_click(harness: &mut Harness<'_, State>, pos: Pos2) {
        harness.event(Event::PointerMoved(pos));
        for pressed in [true, false, true, false] {
            press(harness, pos, PointerButton::Primary, pressed);
        }
        harness.run();
    }

    /// The end keys map inside the frame by the marker margin, so their markers are whole and
    /// fully clickable (a click on the corner key selects it).
    #[test]
    fn end_keys_stay_inside_the_frame_and_are_clickable() {
        let mut harness = harness(track(&[(0.0, 0.0), (1.0, 1.0)]));
        let rect = plot_rect(&harness);
        let safe = rect.shrink(HANDLE_R + 1.5);
        for (x, y) in [(0.0, 0.0), (1.0, 1.0)] {
            assert!(
                safe.contains(at(rect, x, y)),
                "({x}, {y}) too near the edge"
            );
        }
        click(
            &mut harness,
            at(rect, 1.0, 1.0),
            PointerButton::Primary,
            Modifiers::NONE,
        );
        // Delete removes the selected key, but never below the two the track needs: a
        // selection would show in the numeric fields instead; here the click must not add one.
        assert_eq!(harness.state().track.len(), 2, "a click on a key adds none");
    }

    fn times(track: &Track<f64>) -> Vec<f64> {
        track.keys().iter().map(Key::t).collect()
    }

    /// Double-click adds a key ON the curve (its value there) with `new_tan`, as one
    /// edit; right-click deletes it; deletion stops at `min_knots`.
    #[test]
    fn add_on_the_curve_and_delete() {
        let start = track(&[(0.0, 0.0), (1.0, 1.0)]);
        let mut harness = harness(start.clone());
        let rect = plot_rect(&harness);
        // Off the line on purpose: the new key takes the curve's value at t = 0.25.
        double_click(&mut harness, at(rect, 0.25, 0.7));
        let state = harness.state();
        assert_eq!(state.edits, 1, "one edit");
        assert_eq!(state.track.len(), 3);
        let key = state.track.keys()[1];
        assert!((key.t() - 0.25).abs() < 0.01, "time {}", key.t());
        assert!(
            (key.v() - start.eval(key.t())).abs() < 1e-12,
            "on the curve"
        );
        assert_eq!((key.inn.kind, key.out.kind), (Tan::Smooth, Tan::Smooth));

        click(
            &mut harness,
            at(rect, key.t(), key.v()),
            PointerButton::Secondary,
            Modifiers::NONE,
        );
        assert_eq!(harness.state().track.len(), 2, "deleted");
        // The two ends stay: min_knots = 2.
        click(
            &mut harness,
            at(rect, 0.0, 0.0),
            PointerButton::Secondary,
            Modifiers::NONE,
        );
        assert_eq!(harness.state().track.len(), 2, "never below min_knots");
    }

    /// A dragged key moves in time between its neighbours and in value inside the
    /// range; Shift keeps one axis; the pinned ends keep their time.
    #[test]
    fn drag_moves_between_neighbours() {
        let mut harness = harness(track(&[(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)]));
        let rect = plot_rect(&harness);
        drag(
            &mut harness,
            at(rect, 0.5, 0.5),
            at(rect, 0.6, 0.8),
            Modifiers::NONE,
        );
        let k = harness.state().track.keys()[1];
        assert!(
            (k.t() - 0.6).abs() < 0.02 && (k.v() - 0.8).abs() < 0.02,
            "{k:?}"
        );
        // Far past the right neighbour: clamped just before it, never crossing.
        drag(
            &mut harness,
            at(rect, k.t(), k.v()),
            Pos2::new(rect.right() + 40.0, at(rect, 0.0, k.v()).y),
            Modifiers::NONE,
        );
        let t = times(&harness.state().track);
        assert_eq!(t.len(), 3);
        assert!(t[0] < t[1] && t[1] < t[2] && t[2] == 1.0, "{t:?}");
        // Shift: the larger motion (horizontal) wins, the value stays.
        let k = harness.state().track.keys()[1];
        drag(
            &mut harness,
            at(rect, k.t(), k.v()),
            at(rect, 0.3, k.v() - 0.1),
            Modifiers::SHIFT,
        );
        let moved = harness.state().track.keys()[1];
        assert!((moved.t() - 0.3).abs() < 0.02, "{moved:?}");
        assert!((moved.v() - k.v()).abs() < 1e-9, "one axis only");
        // The end is pinned: dragging it changes only the value.
        drag(
            &mut harness,
            at(rect, 1.0, 1.0),
            at(rect, 0.7, 0.4),
            Modifiers::NONE,
        );
        let end = harness.state().track.keys()[2];
        assert_eq!(end.t(), 1.0);
        assert!((end.v() - 0.4).abs() < 0.02, "{end:?}");
    }

    /// Shift-click toggles the segment after the key between Linear and the
    /// new-key kind; the second toggle restores the track exactly.
    #[test]
    fn shift_click_toggles_linear_and_back() {
        let start = track(&[(0.0, 0.0), (0.4, 0.9), (1.0, 1.0)]);
        let mut harness = harness(start.clone());
        let rect = plot_rect(&harness);
        let pos = at(rect, 0.4, 0.9);
        click(&mut harness, pos, PointerButton::Primary, Modifiers::SHIFT);
        {
            let tr = &harness.state().track;
            assert!(is_linear(tr, 1), "segment 1 is linear");
            assert!(!is_linear(tr, 0), "segment 0 untouched");
            assert!(tr.keys()[1].broken, "sides differ: broken");
            // Straight: the midpoint of segment 1 is the chord's midpoint.
            assert!((tr.eval(0.7) - 0.95).abs() < 1e-9);
        }
        click(&mut harness, pos, PointerButton::Primary, Modifiers::SHIFT);
        assert_eq!(harness.state().track, start, "toggled back");
        assert_eq!(harness.state().edits, 2);
    }

    /// Dragging a handle of the selected key shapes that side (a User handle at the
    /// pointer) and, on an unbroken key, the other side follows; Shift breaks the
    /// key so the sides are independent.
    #[test]
    fn handle_drag_couples_and_shift_breaks() {
        let mut harness = harness(track(&[(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)]));
        let rect = plot_rect(&harness);
        // Select the middle key, then find its out handle.
        click(
            &mut harness,
            at(rect, 0.5, 0.5),
            PointerButton::Primary,
            Modifiers::NONE,
        );
        let handle_pos = |h: &Harness<'_, State>, id: SideId| {
            let tr = &h.state().track;
            let (dt, dv) = tr.handle(1, id).expect("live side");
            let k = tr.keys()[1];
            at(rect, k.t() + dt, k.v() + dv)
        };
        let from = handle_pos(&harness, SideId::Out);
        drag(&mut harness, from, at(rect, 0.8, 0.9), Modifiers::NONE);
        {
            let k = harness.state().track.keys()[1];
            assert_eq!((k.out.kind, k.inn.kind), (Tan::User, Tan::User), "coupled");
            assert_eq!(k.out.slope, k.inn.slope);
            assert!(!k.broken);
            let (dt, dv) = harness.state().track.handle(1, SideId::Out).expect("out");
            assert!((k.t() + dt - 0.8).abs() < 0.02 && (k.v() + dv - 0.9).abs() < 0.02);
        }
        // Shift: break first, then the in side no longer follows.
        let in_before = harness.state().track.keys()[1].inn;
        let from = handle_pos(&harness, SideId::Out);
        drag(&mut harness, from, at(rect, 0.9, 0.5), Modifiers::SHIFT);
        let k = harness.state().track.keys()[1];
        assert!(k.broken);
        assert_eq!(k.inn, in_before, "in side independent");
        assert_ne!(k.out.slope, k.inn.slope);
    }

    /// Delete with the plot focused removes the selected key.
    #[test]
    fn delete_key_removes_selected() {
        let mut harness = harness(track(&[(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)]));
        let rect = plot_rect(&harness);
        click(
            &mut harness,
            at(rect, 0.5, 0.5),
            PointerButton::Primary,
            Modifiers::NONE,
        );
        harness.key_press(egui::Key::Delete);
        harness.run();
        assert_eq!(times(&harness.state().track), vec![0.0, 1.0]);
    }

    /// A stop list: positions and colours.
    fn stops(list: &[(f32, Color32)]) -> Gradient {
        Gradient {
            stops: list.iter().map(|&(p, c)| GradientStop::new(p, c)).collect(),
        }
    }

    /// Every kind samples the end stops exactly and stays a valid colour
    /// (premultiplied channels never exceed alpha) over the whole `0..=1` range.
    #[test]
    fn gradient_sampling_stays_in_range() {
        let g = stops(&[
            (0.0, Color32::BLACK),
            (0.2, Color32::RED),
            (0.5, Color32::from_rgba_unmultiplied(0, 255, 0, 128)),
            (0.9, Color32::WHITE),
            (1.0, Color32::BLUE),
        ]);
        for kind in GRADIENT_KINDS {
            assert_eq!(g.sample(0.0, kind), Color32::BLACK, "{kind:?}");
            assert_eq!(g.sample(1.0, kind), Color32::BLUE, "{kind:?}");
            for i in 0..=400 {
                let c = g.sample(i as f32 / 400.0, kind);
                assert!(
                    c.r() <= c.a() && c.g() <= c.a() && c.b() <= c.a(),
                    "{kind:?} {c:?}"
                );
            }
            // Out-of-range t clamps to the end stops.
            assert_eq!(g.sample(-3.0, kind), g.sample(0.0, kind));
            assert_eq!(g.sample(7.0, kind), g.sample(1.0, kind));
        }
    }

    /// Linear is the linear-space lerp; Constant is a hard step; equal positions are
    /// a hard edge; empty and single-stop gradients.
    #[test]
    fn gradient_kinds() {
        let g = stops(&[(0.0, Color32::BLACK), (1.0, Color32::WHITE)]);
        let lin = egui::Rgba::from(g.sample(0.5, Tan::Linear));
        assert!(
            (lin.r() - 0.5).abs() < 0.01,
            "linear-space midpoint {}",
            lin.r()
        );
        assert_eq!(g.sample(0.999, Tan::Constant), Color32::BLACK, "hold");

        let edge = stops(&[
            (0.0, Color32::BLACK),
            (0.5, Color32::RED),
            (0.5, Color32::BLUE),
            (1.0, Color32::BLUE),
        ]);
        assert_eq!(edge.sample(0.49, Tan::Linear).b(), 0, "left of the edge");
        assert_eq!(
            edge.sample(0.51, Tan::Linear),
            Color32::BLUE,
            "right of the edge"
        );

        let empty = Gradient { stops: Vec::new() };
        assert_eq!(empty.sample(0.5, Tan::Linear), Color32::TRANSPARENT);
        let one = stops(&[(0.3, Color32::GREEN)]);
        assert_eq!(one.sample(0.9, Tan::Smooth), Color32::GREEN);
    }

    /// Ties are hard edges whose right side is the LAST stop of the tie, at the
    /// ends too (`sample(1.0)` / `sample(0.0)`), for 2 and 3 stops, every kind.
    #[test]
    fn gradient_ties() {
        let (r, g, b, w) = (Color32::RED, Color32::GREEN, Color32::BLUE, Color32::WHITE);
        for kind in GRADIENT_KINDS {
            let end2 = stops(&[(0.0, Color32::BLACK), (1.0, r), (1.0, g)]);
            assert_eq!(end2.sample(1.0, kind), g, "{kind:?} end x2");
            let end3 = stops(&[(0.0, Color32::BLACK), (1.0, r), (1.0, g), (1.0, b)]);
            assert_eq!(end3.sample(1.0, kind), b, "{kind:?} end x3");
            assert_eq!(end3.sample(5.0, kind), b, "{kind:?} clamp");
            let start2 = stops(&[(0.0, r), (0.0, g), (1.0, w)]);
            assert_eq!(start2.sample(0.0, kind), g, "{kind:?} start x2");
            let start3 = stops(&[(0.0, r), (0.0, g), (0.0, b), (1.0, w)]);
            assert_eq!(start3.sample(0.0, kind), b, "{kind:?} start x3");
            let mid3 = stops(&[
                (0.0, Color32::BLACK),
                (0.5, r),
                (0.5, g),
                (0.5, b),
                (1.0, w),
            ]);
            assert_eq!(mid3.sample(0.5, kind), b, "{kind:?} mid x3");
            assert!(mid3.sample(0.51, kind).b() > 0, "{kind:?} right of mid");
            let mid2 = stops(&[(0.0, Color32::BLACK), (0.5, r), (0.5, g), (1.0, w)]);
            assert_eq!(mid2.sample(0.5, kind), g, "{kind:?} mid x2");
        }
        // The left side of a Constant edge is the first stop of the tie.
        let mid = stops(&[(0.0, Color32::BLACK), (0.5, r), (0.5, g), (1.0, w)]);
        assert_eq!(mid.sample(0.4999, Tan::Constant), Color32::BLACK, "hold");
        assert_eq!(mid.sample(0.5, Tan::Constant), g);
    }

    /// Stops supplied out of order are sorted stably, not nudged into a wrong ramp.
    #[test]
    fn gradient_unsorted_input() {
        let (r, g, b) = (Color32::RED, Color32::GREEN, Color32::BLUE);
        let a = stops(&[(1.0, b), (0.0, r), (0.5, g)]);
        let sorted = stops(&[(0.0, r), (0.5, g), (1.0, b)]);
        for kind in GRADIENT_KINDS {
            for i in 0..=20 {
                let t = i as f32 / 20.0;
                assert_eq!(a.sample(t, kind), sorted.sample(t, kind), "{kind:?} {t}");
            }
        }
        // Equal positions keep the given order: the later one wins on the right.
        let tie = stops(&[(1.0, r), (0.0, b), (1.0, g)]);
        assert_eq!(tie.sample(1.0, Tan::Linear), g);
        assert_eq!(tie.sample(0.0, Tan::Linear), b);
    }

    /// The set editor over `n` named tracks: the editor's tracks and last phase.
    struct SetState {
        curves: Vec<Track<f64>>,
        labels: Vec<String>,
        defaults: Vec<Track<f64>>,
        phase: EditPhase,
    }

    fn set_harness(n: usize) -> Harness<'static, SetState> {
        let line = || track(&[(0.0, 0.0), (1.0, 1.0)]);
        let names = ["R", "G", "B", "A", "L", "M"];
        let state = SetState {
            curves: (0..n).map(|_| line()).collect(),
            labels: names.iter().take(n).map(|n| (*n).to_owned()).collect(),
            defaults: (0..n).map(|_| line()).collect(),
            phase: EditPhase::Idle,
        };
        let mut harness = Harness::builder()
            .with_size(egui::vec2(320.0, 320.0))
            .with_step_dt(1.0 / 60.0)
            .with_max_steps(8)
            .build_ui_state(
                |ui, st: &mut SetState| {
                    let cfg = CurveConfig {
                        height: 160.0,
                        ..CurveConfig::default()
                    };
                    let out = curves_edit(
                        ui,
                        Id::new("set"),
                        CurveSet {
                            curves: &mut st.curves,
                            labels: &st.labels,
                            colors: &[],
                            defaults: Some(&st.defaults),
                        },
                        &cfg,
                        None,
                    );
                    if out.phase != EditPhase::Idle {
                        st.phase = out.phase;
                    }
                },
                state,
            );
        harness.run();
        harness
    }

    /// Up to four curves select through the grid's segmented strip by their host label;
    /// the plot fills the panel width; Reset is an icon button named "Reset".
    #[test]
    fn set_strip_selects_by_label_and_reset_is_an_icon() {
        let mut harness = set_harness(3);
        assert!(harness.get_by_label("curve plot").rect().width() > 280.0);
        harness.get_by_label("G").click();
        // Let egui's click-count window of the strip click expire, or the plot's double-click
        // below would count as part of it.
        harness.run_steps(40);
        harness.run();
        let rect = harness.get_by_label("curve plot").rect();
        double_click_set(&mut harness, at(rect, 0.5, 0.5));
        let st = harness.state();
        assert_eq!(
            [
                st.curves[0].keys().len(),
                st.curves[1].keys().len(),
                st.curves[2].keys().len()
            ],
            [2, 3, 2],
            "the key went to the selected G channel only"
        );
        harness.get_by_label("Reset").click();
        harness.run();
        assert_eq!(harness.state().curves[1].keys().len(), 2);
        assert_eq!(harness.state().phase, EditPhase::Done);
    }

    /// More than four curves fall back to a dropdown (no segment labels).
    #[test]
    fn set_beyond_four_uses_a_dropdown() {
        let harness = set_harness(6);
        assert!(harness.query_by_label("G").is_none());
    }

    fn double_click_set(harness: &mut Harness<'_, SetState>, pos: Pos2) {
        harness.event(Event::PointerMoved(pos));
        harness.run();
        for pressed in [true, false, true, false] {
            harness.event(Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
        }
        harness.run();
    }

    /// Renders the curve set editor (3 and 6 channels) and the gradient editor to PNGs in
    /// `$WIDGET_SHOTS`: `WIDGET_SHOTS=dir cargo test -p egui-ramp -- --ignored shots`. Needs a GPU.
    #[test]
    #[ignore = "writes screenshots; needs a GPU adapter and WIDGET_SHOTS"]
    fn shots() {
        let Some(dir) = std::env::var_os("WIDGET_SHOTS") else {
            return;
        };
        for (name, n) in [("ramp-style-curve.png", 4), ("ramp-style-curve-6.png", 6)] {
            let mut harness = set_harness(n);
            egui_widgets_config::ensure_icon_font(&harness.ctx);
            if n <= STRIP_MAX {
                harness.get_by_label("B").click();
            }
            // A new key mid-curve is selected: the shot shows its ring and tangent handles.
            harness.run_steps(40);
            let rect = harness.get_by_label("curve plot").rect();
            double_click_set(&mut harness, at(rect, 0.5, 0.5));
            harness.event(Event::PointerMoved(Pos2::new(310.0, 310.0)));
            harness.run();
            let image = harness.render().expect("render");
            image
                .save(std::path::Path::new(&dir).join(name))
                .expect("save");
        }
        let colors = [
            Color32::from_rgb(20, 20, 60),
            Color32::from_rgb(200, 40, 40),
            Color32::from_rgb(240, 190, 40),
            Color32::from_rgb(60, 180, 90),
            Color32::from_rgb(240, 240, 255),
        ];
        let mut gradient = Gradient::new();
        gradient.stops = colors
            .iter()
            .enumerate()
            .map(|(i, &c)| GradientStop::new(i as f32 / 4.0, c))
            .collect();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(320.0, 90.0))
            .with_max_steps(8)
            .build_ui_state(
                |ui, (g, rect): &mut (Gradient, Rect)| {
                    *rect = gradient_edit(ui, g, &GradientConfig::default()).rect;
                },
                (gradient, Rect::NOTHING),
            );
        egui_widgets_config::ensure_icon_font(&harness.ctx);
        harness.run();
        // Select the second stop: click its marker (block = the response minus gap + swatch).
        let cfg = GradientConfig::default();
        let rect = harness.state().1;
        let block_w = rect.width() - 3.0 * cfg.metrics.normalized().component_gap - SWATCH;
        let marker = Pos2::new(
            rect.left() + 0.25 * block_w,
            rect.top() + cfg.height + 2.0 + cfg.marker_size * 0.7,
        );
        harness.event(Event::PointerMoved(marker));
        harness.run();
        for pressed in [true, false] {
            harness.event(Event::PointerButton {
                pos: marker,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
        }
        harness.event(Event::PointerMoved(Pos2::new(300.0, 80.0)));
        harness.run();
        let image = harness.render().expect("render");
        image
            .save(std::path::Path::new(&dir).join("ramp-style-gradient.png"))
            .expect("save");
    }
}
