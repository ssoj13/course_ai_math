//! A slim, custom-painted horizontal slider for egui — a thin rail + a SHORT flat
//! handle + an optional trailing fill and a trailing numeric value box.
//!
//! ## Why this is painted, not a wrapped `egui::Slider`
//! egui 0.34's `Slider` clamps its own height to `max(body_text_height,
//! interact_size.y)` (see `Slider::add_contents`) and sizes the handle to ~0.8x
//! that. So the position handle can never be shorter than the body font — it always
//! looks tall/full-height in a dense toolbar, and `spacing` can't fix it. To get a
//! genuinely SHORT flat handle we paint the rail + handle ourselves and run the
//! drag math directly. Interaction is a plain `Sense::click_and_drag` on the track
//! (click-to-jump + drag), matching a normal slider. The optional value box is a
//! real `egui::DragValue` editing the same value.
//!
//! ```no_run
//! # egui::__run_test_ui(|ui| {
//! let mut zoom = 40.0_f32;
//! ui.add(egui_slider::SlimSlider::new(&mut zoom, 2.0..=2000.0).logarithmic(true));
//! let mut h = 56.0_f32;
//! egui_slider::slim_slider(ui, &mut h, 24.0..=240.0);
//! # });
//! ```
//!
//! Generic over `egui::emath::Numeric`; carries no application types.
//!
//! **Showing never writes the value.** The default `clamping` is `Edits`: a stored value
//! outside the range is drawn as it is and only an edit is clamped. `Always` (clamp on show)
//! is an explicit opt-in for widgets that own their value.

use std::ops::RangeInclusive;

use egui::emath::Numeric;
use egui::{
    Color32, CornerRadius, DragValue, Id, Rect, Response, Sense, StrokeKind, Ui, Widget,
    WidgetInfo, lerp, pos2, vec2,
};

/// Default rail thickness (px).
pub const RAIL_HEIGHT: f32 = 4.0;
/// Default handle bar height (px) — deliberately SHORT (egui's stock handle is ~15+).
pub const HANDLE_HEIGHT: f32 = 10.0;
/// Default handle bar width (px).
pub const HANDLE_WIDTH: f32 = 7.0;
/// Narrowest track (px) a [`SlimSlider::fill`] slider is squeezed to: below this a rail stops
/// being usable as a drag target, so the row overflows instead (value taken from the former
/// attribute-table rows, which clamped their rail the same way).
pub const MIN_TRACK: f32 = 24.0;

/// How many decimals a ranged row's value box shows: those of its `step`, else enough for the
/// rail's resolution over its span (about a hundredth of it: 2 for a 0..1 span, 1 for 0..10),
/// and none for an integral value. Feed it to [`SlimSlider::decimals`] and to [`field_text`]
/// so the box draws what a host measured.
pub fn field_decimals(integral: bool, step: Option<f64>, lo: f64, hi: f64) -> usize {
    if integral {
        return 0;
    }
    let step = step.filter(|s| s.is_finite() && *s > 0.0).or_else(|| {
        let span = (hi - lo).abs();
        (span.is_finite() && span > 0.0).then(|| span / 100.0)
    });
    // Smallest d with 10^-d <= step (within float noise), at most 6.
    step.map_or(2, |s| ((-s.log10() - 1e-9).ceil().max(0.0) as usize).min(6))
}

/// The text of the value box for `value` at `decimals`, with its `suffix`: exactly what the box
/// draws ([`SlimSlider::decimals`]), so a measured width is the drawn width.
pub fn field_text(value: f64, decimals: usize, suffix: &str) -> String {
    format!(
        "{}{suffix}",
        egui::emath::format_with_decimals_in_range(value, decimals..=decimals)
    )
}

/// The font the value box draws with in `style`: the override, else the drag-value text style.
pub fn field_font(style: &egui::Style) -> egui::FontId {
    style
        .override_font_id
        .clone()
        .unwrap_or_else(|| style.drag_value_text_style.resolve(style))
}

/// Snap + clamp, ONE rule for the rail, the value box and the keys (egui `Slider::set_value`'s):
/// clamp into `bounds` (none = a soft value), snap to `step` counted from `origin` (the range
/// start), round integers, clamp again (the snap may overshoot the end).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Snap {
    origin: f64,
    bounds: Option<(f64, f64)>,
    step: Option<f64>,
    integral: bool,
}

impl Snap {
    fn apply(self, v: f64) -> f64 {
        let clamp = |v: f64| self.bounds.map_or(v, |(lo, hi)| v.clamp(lo, hi));
        let mut v = clamp(v);
        if let Some(step) = self.step.filter(|s| *s > 0.0) {
            v = self.origin + ((v - self.origin) / step).round() * step;
        }
        if self.integral {
            v = v.round();
        }
        clamp(v)
    }
}

/// A slim, custom-painted slider: thin rail + a short flat handle + optional trailing
/// fill and numeric value box. Build it and `ui.add(..)` it, or use [`slim_slider`].
pub struct SlimSlider<'a, Num: Numeric> {
    value: &'a mut Num,
    range: RangeInclusive<Num>,
    /// Range the value box accepts; None = the rail range.
    field_range: Option<RangeInclusive<Num>>,
    rail_height: f32,
    handle_height: f32,
    handle_width: f32,
    handle_corner: Option<f32>,
    row_height: Option<f32>,
    trailing_fill: bool,
    value_field: bool,
    field_width: Option<f32>,
    width: Option<f32>,
    fill: bool,
    default: Option<Num>,
    defer_reset: bool,
    labelled_by: Option<Id>,
    logarithmic: bool,
    step: Option<f64>,
    suffix: String,
    accent: Option<Color32>,
    text: Option<egui::WidgetText>,
    integer: bool,
    max_decimals: Option<usize>,
    decimals: Option<usize>,
    clamping: egui::SliderClamping,
}

impl<'a, Num: Numeric> SlimSlider<'a, Num> {
    /// A slim slider over `value`, constrained to `range`.
    pub fn new(value: &'a mut Num, range: RangeInclusive<Num>) -> Self {
        Self {
            value,
            range,
            field_range: None,
            rail_height: RAIL_HEIGHT,
            handle_height: HANDLE_HEIGHT,
            handle_width: HANDLE_WIDTH,
            handle_corner: None,
            row_height: None,
            trailing_fill: true,
            value_field: true,
            field_width: None,
            width: None,
            fill: false,
            default: None,
            defer_reset: false,
            labelled_by: None,
            logarithmic: false,
            step: None,
            suffix: String::new(),
            accent: None,
            text: None,
            integer: false,
            max_decimals: None,
            decimals: None,
            // Showing a widget never writes its value (see `Self::clamping`).
            clamping: egui::SliderClamping::Edits,
        }
    }

    /// Rail thickness in px (default [`RAIL_HEIGHT`]).
    pub fn rail_height(mut self, h: f32) -> Self {
        self.rail_height = h;
        self
    }

    /// Handle bar height in px (default [`HANDLE_HEIGHT`]) — the knob most likely to
    /// tune: smaller = a flatter, shorter position handle.
    pub fn handle_height(mut self, h: f32) -> Self {
        self.handle_height = h;
        self
    }

    /// Handle bar width in px (default [`HANDLE_WIDTH`]).
    pub fn handle_width(mut self, w: f32) -> Self {
        self.handle_width = w;
        self
    }

    /// Handle corner radius in px (default: the egui theme's widget corner radius).
    /// `0.0` = sharp corners; a value >= half the smaller side = a rounded pill.
    pub fn handle_corner_radius(mut self, r: f32) -> Self {
        self.handle_corner = Some(r);
        self
    }

    /// Clickable row height in px (default: the style's `interact_size.y`, so a host that
    /// compacts its controls compacts the slider too); the hit target and value-box height. The
    /// handle is centred in it, so this need not match the handle height.
    pub fn row_height(mut self, h: f32) -> Self {
        self.row_height = Some(h);
        self
    }

    /// Fill the rail up to the handle with the accent colour (default `true`).
    pub fn trailing_fill(mut self, on: bool) -> Self {
        self.trailing_fill = on;
        self
    }

    /// Show the trailing numeric [`DragValue`] box (default `true`).
    pub fn value_field(mut self, on: bool) -> Self {
        self.value_field = on;
        self
    }

    /// Range the value box accepts (default: the rail range). A wider one makes the rail a
    /// soft range: dragging covers the useful span, typing may go past it.
    pub fn field_range(mut self, range: RangeInclusive<Num>) -> Self {
        self.field_range = Some(range);
        self
    }

    /// Width (px) of the trailing value box (default: the style's `interact_size.x`). A host
    /// that wants every row's box to line up passes one shared width here.
    pub fn field_width(mut self, w: f32) -> Self {
        self.field_width = Some(w);
        self
    }

    /// Fix the rail track width (px). Default: egui's `spacing.slider_width`.
    pub fn width(mut self, w: f32) -> Self {
        self.width = Some(w);
        self
    }

    /// Let the rail take all the available width minus the value box (and the gap before it),
    /// so the box ends flush with the right edge of the cell: rows of different content all end
    /// at one edge. Ignored when [`Self::width`] is set; a [`Self::text`] label is not budgeted.
    pub fn fill(mut self, on: bool) -> Self {
        self.fill = on;
        self
    }

    /// The value the set's reset gesture (`egui_widgets_config::reset`: Ctrl/Cmd-click or a
    /// right click, on the rail or the value box) restores (default: none, a click jumps like
    /// any other).
    pub fn default_value(mut self, v: Num) -> Self {
        self.default = Some(v);
        self
    }

    /// Ctrl/Cmd-click on the rail or the box neither jumps nor resets nor starts typing: it only
    /// reports `clicked()`, and the host applies its own reset (so its veto holds); the right
    /// click stays the host's (its context menu). Takes precedence over [`Self::default_value`].
    pub fn defer_reset(mut self) -> Self {
        self.defer_reset = true;
        self
    }

    /// Name the slider by another widget (a row label drawn elsewhere) for the accessibility
    /// tree, like `Response::labelled_by`; [`Self::text`] names it by its own label otherwise.
    /// Snap to whole numbers on a float: the rail steps by 1 and the box shows no decimals (an
    /// integer `Num` is always so).
    pub fn integer(mut self) -> Self {
        self.integer = true;
        self
    }

    /// Most decimals the value box shows (the stored value is never rounded by display).
    pub fn max_decimals(mut self, n: usize) -> Self {
        self.max_decimals = Some(n);
        self
    }

    /// Exactly this many decimals in the value box ([`field_text`] is its text): the width a
    /// host measures for the box is then the width it draws.
    pub fn decimals(mut self, n: usize) -> Self {
        self.decimals = Some(n);
        self
    }

    /// Which values the range clamps, with `egui::SliderClamping`'s modes. Default `Edits`.
    ///
    /// The rule: **showing a slider never writes its value; only an edit (rail drag or click,
    /// typed value, arrow keys, AccessKit set) is clamped.** A stored value outside the range is
    /// drawn as it is (handle pinned to the nearer rail end, the box showing the real number)
    /// and the response is not `changed()`. A host's scene or document data therefore survives
    /// being displayed.
    ///
    /// - `Edits` (default): edits are clamped into the range, a stored value is left alone.
    /// - `Always`: explicit opt-in. Additionally brings a stored out-of-range value into the
    ///   range on every show and reports it as a change, so only use it where the widget owns
    ///   the value.
    /// - `Never`: a soft range; edits may leave it too.
    pub fn clamping(mut self, clamping: egui::SliderClamping) -> Self {
        self.clamping = clamping;
        self
    }

    pub fn labelled_by(mut self, id: Id) -> Self {
        self.labelled_by = Some(id);
        self
    }

    /// Use a logarithmic value scale (default `false`). Requires a positive range.
    pub fn logarithmic(mut self, on: bool) -> Self {
        self.logarithmic = on;
        self
    }

    /// Quantise dragging to `step`.
    pub fn step_by(mut self, step: f64) -> Self {
        self.step = Some(step);
        self
    }

    /// Suffix appended to the value box (e.g. `" px"`, `"%"`).
    pub fn suffix(mut self, s: impl ToString) -> Self {
        self.suffix = s.to_string();
        self
    }

    /// Override the trailing-fill accent colour (default: `visuals.selection.bg_fill`).
    pub fn accent(mut self, c: Color32) -> Self {
        self.accent = Some(c);
        self
    }

    /// Trailing label after the value box (like `egui::Slider::text`).
    pub fn text(mut self, text: impl Into<egui::WidgetText>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// The value the keyboard (arrow keys while the rail is focused) or an AccessKit action asks
    /// for, if any. One arrow press is `step`, else 1% of the rail (at least 1 for integers).
    fn keyed_value(&self, ui: &Ui, resp: &Response) -> Option<f64> {
        use egui::accesskit::{Action, ActionData};
        let (lo, hi) = (self.range.start().to_f64(), self.range.end().to_f64());
        let integral = Num::INTEGRAL || self.integer;
        let unit = self
            .step
            .filter(|s| *s > 0.0)
            .unwrap_or_else(|| ((hi - lo).abs() / 100.0).max(if integral { 1.0 } else { 0.0 }));
        let mut delta = 0.0;
        if resp.has_focus() {
            use egui::{EventFilter, Key, Modifiers};
            // Without this egui also takes the arrow press as "move focus": to the value box.
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    resp.id,
                    EventFilter {
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        ..Default::default()
                    },
                );
            });
            ui.input_mut(|i| {
                for (key, sign) in [
                    (Key::ArrowRight, 1.0),
                    (Key::ArrowUp, 1.0),
                    (Key::ArrowLeft, -1.0),
                    (Key::ArrowDown, -1.0),
                ] {
                    if i.consume_key(Modifiers::NONE, key) {
                        delta += sign * unit;
                    }
                }
            });
        }
        let mut set = None;
        ui.input(|i| {
            delta += unit * i.num_accesskit_action_requests(resp.id, Action::Increment) as f64;
            delta -= unit * i.num_accesskit_action_requests(resp.id, Action::Decrement) as f64;
            for request in i.accesskit_action_requests(resp.id, Action::SetValue) {
                if let Some(ActionData::NumericValue(v)) = request.data {
                    set = Some(v);
                }
            }
        });
        let current = self.value.to_f64();
        let v = set.unwrap_or(current) + delta;
        if v == current {
            return None;
        }
        let v = self.snap(self.clamping != egui::SliderClamping::Never).apply(v);
        (v != current).then_some(v)
    }

    /// Map a value to a 0..=1 track fraction (log-aware).
    fn value_to_t(&self, v: f64) -> f32 {
        let (lo, hi) = (self.range.start().to_f64(), self.range.end().to_f64());
        let t = if self.logarithmic && lo > 0.0 && hi > 0.0 && v > 0.0 {
            (v.ln() - lo.ln()) / (hi.ln() - lo.ln())
        } else if hi != lo {
            (v - lo) / (hi - lo)
        } else {
            0.0
        };
        (t as f32).clamp(0.0, 1.0)
    }

    /// Map a 0..=1 track fraction back to a value (log-aware), quantised + clamped.
    fn t_to_value(&self, t: f32) -> f64 {
        let (lo, hi) = (self.range.start().to_f64(), self.range.end().to_f64());
        let t = t.clamp(0.0, 1.0) as f64;
        let v = if self.logarithmic && lo > 0.0 && hi > 0.0 {
            (lo.ln() + t * (hi.ln() - lo.ln())).exp()
        } else {
            lo + t * (hi - lo)
        };
        // The rail is bounded by its own range whatever the clamping mode.
        self.snap(true).apply(v)
    }

    /// Width of the value box when the host sets none: the style's `interact_size.x`, grown to
    /// the widest text the box can show (either end of the rail; the current value only when it
    /// lies outside the rail, which Edits / Never allow) plus its padding, so "1024 MiB" is
    /// never cut off. The width must not follow the value inside the rail, or the box would jitter
    /// while dragging ("99" -> "100"). The same decimals and suffix the box draws.
    fn auto_field_width(&self, ui: &Ui) -> f32 {
        let (lo, hi) = (self.range.start().to_f64(), self.range.end().to_f64());
        let integral = Num::INTEGRAL || self.integer;
        let decimals = if self.integer {
            0
        } else {
            self.decimals
                .or(self.max_decimals)
                .unwrap_or_else(|| field_decimals(integral, self.step, lo, hi))
        };
        let font = field_font(ui.style());
        let value = self.value.to_f64();
        let outside = value < lo.min(hi) || value > lo.max(hi);
        let widest = [Some(lo), Some(hi), outside.then_some(value)]
            .into_iter()
            .flatten()
            .map(|v| {
                let text = field_text(v, decimals, &self.suffix);
                ui.ctx().fonts_mut(|f| f.layout_no_wrap(text, font.clone(), Color32::WHITE).size().x)
            })
            .fold(0.0, f32::max);
        (widest + 2.0 * ui.spacing().button_padding.x).max(ui.spacing().interact_size.x)
    }

    /// The range the value box accepts (and an `Always` slider clamps a stored value into).
    fn field_bounds(&self) -> (f64, f64) {
        let r = self.field_range.as_ref().unwrap_or(&self.range);
        let (a, b) = (r.start().to_f64(), r.end().to_f64());
        (a.min(b), a.max(b))
    }

    /// The one snap + clamp rule over the rail range; `bounded` clamps into it, else the value
    /// is soft.
    fn snap(&self, bounded: bool) -> Snap {
        let (a, b) = (self.range.start().to_f64(), self.range.end().to_f64());
        Snap {
            origin: a,
            bounds: bounded.then_some((a.min(b), a.max(b))),
            step: self.step,
            integral: Num::INTEGRAL || self.integer,
        }
    }

    /// [`Self::snap`] for the value box: its own range, soft under `Never`.
    fn box_snap(&self) -> Snap {
        Snap {
            bounds: (self.clamping != egui::SliderClamping::Never).then(|| self.field_bounds()),
            ..self.snap(false)
        }
    }
}

impl<'a, Num: Numeric> Widget for SlimSlider<'a, Num> {
    fn ui(self, ui: &mut Ui) -> Response {
        let row_h = self
            .row_height
            .unwrap_or_else(|| ui.spacing().interact_size.y);
        // No box, or a host-fixed width: no text layout at all.
        let field_w = match self.field_width {
            Some(w) => w,
            None if self.value_field => self.auto_field_width(ui),
            None => 0.0,
        };
        ui.horizontal(|ui| {
            let gap = ui.spacing().item_spacing.x;
            let avail = ui.available_width();
            let track_w = match self.width {
                Some(w) => w,
                None if self.fill && avail.is_finite() => {
                    let boxed = if self.value_field { field_w + gap } else { 0.0 };
                    (avail - boxed).max(MIN_TRACK)
                }
                None => ui.spacing().slider_width,
            };
            let (rect, mut resp) =
                ui.allocate_exact_size(vec2(track_w, row_h), Sense::click_and_drag());

            // Only the explicit `Always` opt-in (egui's own default) writes on show: a stored
            // value outside the range is brought into it. `Edits` (ours) and `Never` leave it be.
            if self.clamping == egui::SliderClamping::Always {
                let (a, b) = self.field_bounds();
                let current = self.value.to_f64();
                let clamped = current.clamp(a, b);
                if clamped != current {
                    *self.value = Num::from_f64(clamped);
                    resp.mark_changed();
                }
            }

            // The handle centre travels within the rect inset by half the handle width,
            // so the handle never overhangs the track ends.
            let half = self.handle_width * 0.5;
            let x0 = rect.left() + half;
            let x1 = (rect.right() - half).max(x0 + f32::EPSILON);

            // Interaction: the set's reset gesture restores the default (if any); a deferred
            // reset is only the Ctrl/Cmd-click (the host owns the right click); otherwise
            // click-to-jump + drag, mapping pointer x to the value.
            let reset = if self.defer_reset {
                egui_widgets_config::reset::ctrl_click(ui, &resp)
            } else {
                self.default.is_some() && egui_widgets_config::reset::requested(ui, &resp)
            };
            if reset {
                // An edit only when it moves the value: a reset at the default changes nothing.
                if let Some(default) = self.default.filter(|_| !self.defer_reset)
                    && self.value.to_f64() != default.to_f64()
                {
                    *self.value = default;
                    resp.mark_changed();
                }
            } else if (resp.dragged() || resp.clicked())
                // A Ctrl/Cmd press is the start of a reset (which fires on release): moving the
                // value under the pointer meanwhile would preview a value the click never sets.
                && !((self.defer_reset || self.default.is_some()) && ui.input(|i| i.modifiers.command))
                && let Some(p) = resp.interact_pointer_pos()
            {
                let t = ((p.x - x0) / (x1 - x0)).clamp(0.0, 1.0);
                let nv = self.t_to_value(t);
                *self.value = Num::from_f64(nv);
                resp.mark_changed();
            }

            // Keyboard and assistive technology: arrow keys on the focused rail, and AccessKit
            // Increment / Decrement / SetValue, move the value like the egui slider's.
            if let Some(nv) = self.keyed_value(ui, &resp) {
                *self.value = Num::from_f64(nv);
                resp.mark_changed();
            }

            // The numeric box edits the SAME value; it is drawn BEFORE the rail is described and
            // painted so both show the value after this frame's edit, whichever widget made it.
            let mut boxed = None;
            if self.value_field {
                let (flo, fhi) = if self.clamping == egui::SliderClamping::Never {
                    (Num::MIN.to_f64(), Num::MAX.to_f64())
                } else {
                    self.field_bounds()
                };
                // Drag at the rail's pace (its span over ~200 px), not egui's 1 per px: a soft
                // field has no range clamp to hide a 0..1 value jumping by whole units.
                let (lo, hi) = (self.range.start().to_f64(), self.range.end().to_f64());
                // Every write goes through the ONE snap + clamp rule, inside the setter (as egui's
                // Slider does) so a typed 30.4 on a step of 1 is 30 and the text being edited
                // stays in step with the stored value.
                let snap = self.box_snap();
                let value = &mut *self.value;
                let mut dv = DragValue::from_get_set(|v: Option<f64>| {
                    if let Some(v) = v {
                        *value = Num::from_f64(snap.apply(v));
                    }
                    value.to_f64()
                })
                .range(flo..=fhi)
                .clamp_existing_to_range(false)
                .speed(((hi - lo).abs() / 200.0).max(f64::MIN_POSITIVE));
                if !self.suffix.is_empty() {
                    dv = dv.suffix(self.suffix.clone());
                }
                if Num::INTEGRAL {
                    // `DragValue::new` does this for integers; `from_get_set` cannot know.
                    dv = dv.max_decimals(0);
                }
                if self.integer {
                    dv = dv.max_decimals(0);
                } else if let Some(n) = self.max_decimals {
                    dv = dv.max_decimals(n);
                }
                if let Some(n) = self.decimals.filter(|_| !self.integer) {
                    dv = dv.fixed_decimals(n);
                }
                let mut field = ui.add_sized([field_w, row_h], dv);
                // The box answers the same gesture as the rail. It must be handled HERE: the
                // caller only sees the union response, whose id is the rail's, so it could not
                // take back the text edit the box's DragValue starts on the same click.
                if self.defer_reset {
                    egui_widgets_config::reset::ctrl_click(ui, &field);
                } else if let Some(default) = self.default {
                    egui_widgets_config::reset::apply(ui, &mut field, &mut *self.value, default);
                }
                boxed = Some(field);
            }

            // Accessibility: a slider with its value, range and name, like `egui::Slider`, so
            // AccessKit-driven tests and screen readers find it by role and label.
            let label = self
                .text
                .as_ref()
                .map(|t| t.text().to_owned())
                .unwrap_or_default();
            resp.widget_info(|| {
                WidgetInfo::slider(ui.is_enabled(), self.value.to_f64(), label.as_str())
            });
            ui.ctx().accesskit_node_builder(resp.id, |node| {
                node.set_min_numeric_value(self.range.start().to_f64());
                node.set_max_numeric_value(self.range.end().to_f64());
                if let Some(step) = self.step {
                    node.set_numeric_value_step(step);
                }
                node.add_action(egui::accesskit::Action::SetValue);
                node.add_action(egui::accesskit::Action::Increment);
                node.add_action(egui::accesskit::Action::Decrement);
            });
            // On the rail only: the box is its value, not a second slider of that name.
            if let Some(id) = self.labelled_by {
                resp = resp.labelled_by(id);
            }

            // Paint. Compute the (possibly updated) handle position from the value.
            let t = self.value_to_t(self.value.to_f64());
            let hx = lerp(x0..=x1, t);
            let mid_y = rect.center().y;
            let wv = *ui.style().interact(&resp);
            let painter = ui.painter();

            let rail_round = CornerRadius::same((self.rail_height * 0.5).round().max(0.0) as u8);
            let rail = Rect::from_min_max(
                pos2(rect.left(), mid_y - self.rail_height * 0.5),
                pos2(rect.right(), mid_y + self.rail_height * 0.5),
            );
            painter.rect_filled(rail, rail_round, ui.visuals().widgets.inactive.bg_fill);

            if self.trailing_fill {
                let fill = Rect::from_min_max(rail.min, pos2(hx, rail.max.y));
                let col = self.accent.unwrap_or(ui.visuals().selection.bg_fill);
                painter.rect_filled(fill, rail_round, col);
            }

            let handle = Rect::from_center_size(
                pos2(hx, mid_y),
                vec2(self.handle_width, self.handle_height),
            );
            let hc = self
                .handle_corner
                .map(|r| CornerRadius::same(r.round().max(0.0) as u8))
                .unwrap_or(wv.corner_radius);
            painter.rect_filled(handle, hc, wv.bg_fill);
            painter.rect_stroke(handle, hc, wv.fg_stroke, StrokeKind::Inside);

            if let Some(boxed) = boxed {
                resp |= boxed;
            }
            if let Some(text) = self.text {
                resp |= ui.label(text);
            }
            resp
        })
        .inner
    }
}

/// Terse convenience: add a default [`SlimSlider`] and return its [`Response`].
pub fn slim_slider<Num: Numeric>(
    ui: &mut Ui,
    value: &mut Num,
    range: RangeInclusive<Num>,
) -> Response {
    ui.add(SlimSlider::new(value, range))
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };

    /// Rect of each slider drawn in a 300 px wide column.
    fn rows(f: impl Fn(&mut Ui) -> Vec<Response>) -> Vec<Rect> {
        let mut rects = Vec::new();
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(Default::default(), |ui| {
            ui.set_max_width(300.0);
            rects = f(ui).iter().map(|r| r.rect).collect();
        });
        out.textures_delta.clear();
        rects
    }

    #[test]
    fn fill_ends_every_row_at_one_right_edge_with_the_shared_box_width() {
        let rects = rows(|ui| {
            let (mut a, mut b) = (0.5_f32, 7_i32);
            vec![
                ui.add(
                    SlimSlider::new(&mut a, 0.0..=1.0)
                        .fill(true)
                        .field_width(40.0),
                ),
                ui.add(
                    SlimSlider::new(&mut b, 0..=10)
                        .fill(true)
                        .field_width(40.0)
                        .suffix(" px"),
                ),
            ]
        });
        assert_eq!(rects[0].right(), rects[1].right(), "rows end at one edge");
        assert_eq!(rects[0].width(), rects[1].width());
        assert!(
            (rects[0].width() - 300.0).abs() <= 1.0,
            "the row fills the width: {rects:?}"
        );
    }

    #[test]
    fn box_and_row_sizes_follow_the_style_not_constants() {
        let ctx = egui::Context::default();
        let mut heights = Vec::new();
        for h in [14.0, 30.0] {
            let mut out = ctx.run_ui(Default::default(), |ui| {
                ui.spacing_mut().interact_size = egui::vec2(36.0, h);
                let mut v = 0.5_f32;
                heights.push(
                    ui.add(SlimSlider::new(&mut v, 0.0..=1.0).value_field(false))
                        .rect
                        .height(),
                );
            });
            out.textures_delta.clear();
        }
        assert_eq!(heights, [14.0, 30.0]);
    }

    #[test]
    fn ctrl_click_resets_to_default_and_plain_click_jumps() {
        let ctx = egui::Context::default();
        let mut v = 0.9_f32;
        let frame = |mut events: Vec<egui::Event>, mods: egui::Modifiers, v: &mut f32| {
            events.insert(0, egui::Event::ModifiersChanged(mods));
            let mut rail = Rect::NOTHING;
            let mut out = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    let slider = SlimSlider::new(v, 0.0..=1.0)
                        .default_value(0.25)
                        .value_field(false);
                    rail = ui.add(slider).rect;
                },
            );
            out.textures_delta.clear();
            rail
        };
        let rail = frame(Vec::new(), Default::default(), &mut v);
        let pos = rail.left_center() + egui::vec2(rail.width() * 0.6, 0.0);
        let press = |pressed, modifiers| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        };
        let ctrl = egui::Modifiers::COMMAND;
        frame(
            vec![egui::Event::PointerMoved(pos), press(true, ctrl)],
            ctrl,
            &mut v,
        );
        frame(vec![press(false, ctrl)], ctrl, &mut v);
        assert_eq!(v, 0.25, "Ctrl-click resets");

        // A Ctrl press that then drags is no scrub either: the value stays put.
        v = 0.9;
        frame(vec![press(true, ctrl)], ctrl, &mut v);
        let away = pos + egui::vec2(-rail.width() * 0.4, 0.0);
        frame(vec![egui::Event::PointerMoved(away)], ctrl, &mut v);
        frame(vec![egui::Event::PointerMoved(away + egui::vec2(-2.0, 0.0))], ctrl, &mut v);
        assert_eq!(v, 0.9, "a Ctrl drag moved the value");
        frame(vec![press(false, ctrl)], ctrl, &mut v);
        frame(vec![egui::Event::PointerMoved(pos)], egui::Modifiers::NONE, &mut v);

        let none = egui::Modifiers::NONE;
        frame(vec![press(true, none)], none, &mut v);
        frame(vec![press(false, none)], none, &mut v);
        assert!(
            (v - 0.6).abs() < 0.1,
            "a plain click jumps to the pointer: {v}"
        );
    }

    /// The set's reset gesture on every part of a slider with a default: a right click on the
    /// rail, and a Ctrl/Cmd-click on the value box, which must not also start typing in it (the
    /// caller only sees the union response and could not take that edit back).
    #[test]
    fn right_click_on_the_rail_and_ctrl_click_on_the_box_reset() {
        // Press and release `button` with `mods` at the rail (`on_box` false) or the box;
        // returns the value and whether any widget holds the keyboard focus afterwards.
        let click = |on_box: bool, button: egui::PointerButton, mods: egui::Modifiers| {
            let ctx = egui::Context::default();
            let mut v = 0.9_f32;
            let frame = |mut events: Vec<egui::Event>, v: &mut f32| {
                events.insert(0, egui::Event::ModifiersChanged(mods));
                let mut seen = (Rect::NOTHING, Rect::NOTHING, false);
                let mut out = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                    let r = ui.add(SlimSlider::new(v, 0.0..=1.0).default_value(0.25).width(200.0));
                    // The union spans rail + box; the rail is its first 200 px.
                    let rail = Rect::from_min_size(r.rect.min, egui::vec2(200.0, r.rect.height()));
                    let boxed = Rect::from_min_max(egui::pos2(r.rect.right() - 6.0, r.rect.top()), r.rect.max);
                    seen = (rail, boxed, ui.ctx().memory(|m| m.focused().is_some()));
                });
                out.textures_delta.clear();
                seen
            };
            let (rail, boxed, _) = frame(Vec::new(), &mut v);
            let pos = if on_box { boxed.center() } else { rail.left_center() + egui::vec2(150.0, 0.0) };
            let press = |pressed| egui::Event::PointerButton { pos, button, pressed, modifiers: mods };
            frame(vec![egui::Event::PointerMoved(pos), press(true)], &mut v);
            let focused = frame(vec![press(false)], &mut v).2;
            (v, focused)
        };
        let (v, _) = click(false, egui::PointerButton::Secondary, egui::Modifiers::NONE);
        assert_eq!(v, 0.25, "right click on the rail resets");
        let (v, focused) = click(true, egui::PointerButton::Primary, egui::Modifiers::COMMAND);
        assert_eq!(v, 0.25, "Ctrl/Cmd-click on the box resets");
        assert!(!focused, "the reset click also opened the box for typing");
        let (v, focused) = click(true, egui::PointerButton::Primary, egui::Modifiers::NONE);
        assert_eq!(v, 0.9, "a plain click on the box is no reset");
        assert!(focused, "a plain click on the box starts typing (the probe must hit the box)");
    }

    /// Type `text` into the value box of a 0..=`hi` slider (step 1 when `step`) holding `start`,
    /// under `clamping` (`None` = the widget's default); returns the stored value after Enter.
    /// One headless context, frame by frame: click the box (it starts typing), let it take
    /// focus, type, Enter.
    fn type_into_box(clamping: Option<egui::SliderClamping>, step: bool, start: f32, hi: f32, text: &str) -> f32 {
        let ctx = egui::Context::default();
        let mut v = start;
        let frame = |events: Vec<egui::Event>, v: &mut f32| {
            let mut boxed = egui::Pos2::ZERO;
            let mut out = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                let mut s = SlimSlider::new(v, 0.0..=hi).width(100.0);
                if let Some(c) = clamping {
                    s = s.clamping(c);
                }
                if step {
                    s = s.step_by(1.0);
                }
                let r = ui.add(s);
                boxed = egui::pos2(r.rect.right() - 6.0, r.rect.center().y);
            });
            out.textures_delta.clear();
            boxed
        };
        let pos = frame(Vec::new(), &mut v);
        let button = egui::PointerButton::Primary;
        let press = |pressed| egui::Event::PointerButton { pos, button, pressed, modifiers: Default::default() };
        frame(vec![egui::Event::PointerMoved(pos), press(true)], &mut v);
        frame(vec![press(false)], &mut v);
        frame(Vec::new(), &mut v);
        frame(vec![egui::Event::Text(text.to_owned())], &mut v);
        let key = |pressed| egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Default::default(),
        };
        frame(vec![key(true), key(false)], &mut v);
        frame(Vec::new(), &mut v);
        v
    }

    /// A value typed into the box snaps to `step` exactly like the rail (one rule): 30.4 with a
    /// step of 1 is stored as 30, not 30.4.
    #[test]
    fn a_typed_value_snaps_to_the_step() {
        assert_eq!(type_into_box(None, true, 5.0, 100.0, "30.4"), 30.0);
        assert_eq!(type_into_box(None, true, 5.0, 100.0, "30.6"), 31.0);
        assert_eq!(type_into_box(None, false, 5.0, 100.0, "30.4"), 30.4, "no step: kept as typed");
    }

    /// Show `0.0..=4.0` slider holding 10.0 under `clamping` (`None` = default) for one frame;
    /// returns the stored value and whether the response reports a change.
    fn show_out_of_range(clamping: Option<egui::SliderClamping>) -> (f32, bool) {
        let ctx = egui::Context::default();
        let mut v = 10.0_f32;
        let mut changed = false;
        let mut out = ctx.run_ui(Default::default(), |ui| {
            let mut s = SlimSlider::new(&mut v, 0.0..=4.0);
            if let Some(c) = clamping {
                s = s.clamping(c);
            }
            changed = ui.add(s).changed();
        });
        out.textures_delta.clear();
        (v, changed)
    }

    /// The rule: showing never writes the value. The DEFAULT mode draws an out-of-range stored
    /// value as it is and reports no change.
    #[test]
    fn showing_with_the_default_never_writes_an_out_of_range_value() {
        assert_eq!(show_out_of_range(None), (10.0, false));
    }

    /// `Always` is the explicit opt-in that clamps on show (and says so via `changed`).
    #[test]
    fn always_clamps_a_stored_value_on_show() {
        assert_eq!(show_out_of_range(Some(egui::SliderClamping::Always)), (4.0, true));
    }

    /// `Edits` and `Never` leave the stored value alone on show too.
    #[test]
    fn edits_and_never_leave_a_stored_value_on_show() {
        use egui::SliderClamping::{Edits, Never};
        assert_eq!(show_out_of_range(Some(Edits)), (10.0, false));
        assert_eq!(show_out_of_range(Some(Never)), (10.0, false));
    }

    /// A typed value is clamped by the default mode and by `Always`; `Never` is soft.
    #[test]
    fn a_typed_value_is_clamped_by_the_default() {
        use egui::SliderClamping::{Always, Never};
        assert_eq!(type_into_box(None, false, 1.0, 4.0, "99"), 4.0, "default: an edit is clamped");
        assert_eq!(type_into_box(Some(Always), false, 1.0, 4.0, "99"), 4.0);
        assert_eq!(type_into_box(Some(Never), false, 1.0, 4.0, "99"), 99.0, "soft: an edit may leave the range");
    }

    /// A drag on the rail is an edit: with the default mode it replaces an out-of-range stored
    /// value with an in-range one, and a drag past the rail end lands on the range end.
    #[test]
    fn a_drag_is_clamped_by_the_default() {
        let ctx = egui::Context::default();
        let mut v = 10.0_f32;
        let frame = |events: Vec<egui::Event>, v: &mut f32| {
            let mut rail = Rect::NOTHING;
            let mut out = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                rail = ui.add(SlimSlider::new(v, 0.0..=4.0).width(100.0).value_field(false)).rect;
            });
            out.textures_delta.clear();
            rail
        };
        let rail = frame(Vec::new(), &mut v);
        assert_eq!(v, 10.0, "showing wrote the value");
        let start = rail.left_center() + egui::vec2(50.0, 0.0);
        let button = egui::PointerButton::Primary;
        let press = |pressed| egui::Event::PointerButton { pos: start, button, pressed, modifiers: Default::default() };
        frame(vec![egui::Event::PointerMoved(start), press(true)], &mut v);
        let past = rail.right_center() + egui::vec2(60.0, 0.0);
        frame(vec![egui::Event::PointerMoved(past)], &mut v);
        frame(vec![egui::Event::PointerMoved(past + egui::vec2(1.0, 0.0))], &mut v);
        frame(vec![press(false)], &mut v);
        assert_eq!(v, 4.0, "a drag past the rail end is clamped to the range end");
    }

    /// The value box is wide enough for the widest text it can show, unless the host fixes it.
    #[test]
    fn the_box_fits_the_widest_formatted_end() {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(Default::default(), |ui| {
            ui.set_max_width(600.0);
            let gap = ui.spacing().item_spacing.x;
            let pad = ui.spacing().button_padding.x;
            let measure = |ui: &Ui, text: &str| {
                ui.ctx().fonts_mut(|f| f.layout_no_wrap(text.to_owned(), field_font(ui.style()), Color32::WHITE).size().x)
            };
            let mut mib = 64_u32;
            let r = ui.add(SlimSlider::new(&mut mib, 8..=1024).width(100.0).suffix(" MiB"));
            let box_w = r.rect.width() - 100.0 - gap;
            assert!(box_w >= measure(ui, "1024 MiB") + 2.0 * pad - 0.5, "1024 MiB is cut off in {box_w}");
            let mut pt = 10.5_f32;
            let r = ui.add(SlimSlider::new(&mut pt, 6.0..=32.0).width(100.0).step_by(0.5).max_decimals(1).suffix(" pt"));
            let box_w = r.rect.width() - 100.0 - gap;
            assert!(box_w >= measure(ui, "32.0 pt") + 2.0 * pad - 0.5, "32.0 pt is cut off in {box_w}");
            // A short range keeps the style's width; a host's own width wins.
            let mut small = 1.0_f32;
            let r = ui.add(SlimSlider::new(&mut small, 0.0..=1.0).width(100.0));
            assert!((r.rect.width() - 100.0 - gap - ui.spacing().interact_size.x).abs() < 1.0, "{} {} {}", r.rect.width(), gap, ui.spacing().interact_size.x);
            let r = ui.add(SlimSlider::new(&mut small, 0.0..=1.0).width(100.0).field_width(70.0));
            assert!((r.rect.width() - 100.0 - gap - 70.0).abs() < 1.0, "a fixed field width is kept");
        });
        out.textures_delta.clear();
    }

    /// The box keeps its width while the value moves INSIDE the rail ("99" -> "100" must not
    /// jitter the layout); a value outside it (`Never`) widens it.
    #[test]
    fn the_box_width_does_not_follow_the_value_inside_the_rail() {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(Default::default(), |ui| {
            ui.set_max_width(600.0);
            let width = |ui: &mut Ui, v: i32, c| {
                let mut v = v;
                ui.add(SlimSlider::new(&mut v, 0..=1024).width(100.0).suffix(" MiB").clamping(c)).rect.width()
            };
            use egui::SliderClamping::{Always, Never};
            let (a, b, c) = (width(ui, 99, Always), width(ui, 100, Always), width(ui, 1024, Always));
            assert_eq!((a, b), (b, c), "the width follows the rail, not the value: {a} {b} {c}");
            let wide = width(ui, 123_456_789, Never);
            assert!(wide > a + 20.0, "a value outside the rail widens the box: {wide} vs {a}");
            // The measurement itself (the DragValue grows to its text anyway, so the drawn width
            // alone cannot tell): inside the rail the value is not measured, outside it is.
            let measured = |ui: &Ui, mut v: i32| SlimSlider::new(&mut v, 0..=1024).suffix(" MiB").auto_field_width(ui);
            assert_eq!(measured(ui, 7), measured(ui, 1024));
            assert!(measured(ui, 123_456_789) > measured(ui, 7) + 20.0);
        });
        out.textures_delta.clear();
    }

    /// Snapping counts from the range START, as egui's `Slider::set_value` does
    /// (egui-0.36.2 `widgets/slider.rs:613-614`: `start + ((value - start) / step).round() * step`):
    /// 0.5..=10 with a step of 1 lands on 1.5, 2.5, ... (not on whole numbers), on the rail and
    /// in the box alike (both go through `Snap`).
    #[test]
    fn steps_count_from_the_range_start_like_egui() {
        let mut v = 0.5_f32;
        let slider = SlimSlider::new(&mut v, 0.5..=10.0).step_by(1.0);
        let at = |x: f64| slider.t_to_value(((x - 0.5) / 9.5) as f32);
        assert_eq!(at(1.9), 1.5);
        assert_eq!(at(2.2), 2.5);
        assert_eq!(at(9.9), 9.5);
        assert_eq!(at(10.0), 10.0, "the end stays reachable (clamped)");
        let box_snap = slider.box_snap();
        assert_eq!(box_snap.apply(2.2), 2.5, "the box counts from the start too");
        assert_eq!(box_snap.apply(99.0), 10.0);
    }

    #[test]
    fn exposes_slider_role_label_and_value_to_accesskit() {
        let harness = Harness::new_ui_state(
            |ui, v: &mut f32| {
                ui.add(SlimSlider::new(v, 0.0..=2.0).text("Gain"));
            },
            1.5_f32,
        );
        let node = harness.get_by_role_and_label(egui::accesskit::Role::Slider, "Gain");
        assert_eq!(node.accesskit_node().role(), egui::accesskit::Role::Slider);
        assert_eq!(node.accesskit_node().numeric_value(), Some(1.5));
    }

    #[test]
    fn arrow_keys_on_the_focused_rail_step_the_value_and_clamp_unless_never() {
        for (clamping, expect_after_max) in [
            (egui::SliderClamping::Always, 10.0),
            (egui::SliderClamping::Never, 11.0),
        ] {
            let mut harness = Harness::new_ui_state(
                move |ui, v: &mut f32| {
                    ui.add(
                        SlimSlider::new(v, 0.0..=10.0)
                            .step_by(1.0)
                            .clamping(clamping)
                            .text("Gain"),
                    );
                },
                9.0_f32,
            );
            harness.get_by_role(egui::accesskit::Role::Slider).focus();
            harness.run();
            // `step`, not `run`: a press that changes nothing requests no repaint, and `run`
            // would leave the event queued behind the next one.
            harness.key_press(egui::Key::ArrowRight);
            harness.step();
            // `step`, not `run`: a press that changes nothing requests no repaint, and `run`
            // would leave the event queued behind the next one.
            harness.key_press(egui::Key::ArrowRight);
            harness.step();
            assert_eq!(*harness.state(), expect_after_max, "{clamping:?}");
            harness.key_press(egui::Key::ArrowLeft);
            harness.step();
            assert_eq!(*harness.state(), expect_after_max - 1.0);
        }
    }

    #[test]
    fn integer_rounds_a_float_rail_and_max_decimals_reaches_the_box() {
        let ctx = egui::Context::default();
        let mut v = 0.0_f32;
        let mut out = ctx.run_ui(Default::default(), |ui| {
            // 0.37 of a 0..10 rail is 3.7: an integer slider lands on 4.
            let slider = SlimSlider::new(&mut v, 0.0..=10.0)
                .integer()
                .max_decimals(1);
            assert_eq!(slider.t_to_value(0.37), 4.0);
            ui.add(slider);
        });
        out.textures_delta.clear();
    }
}
