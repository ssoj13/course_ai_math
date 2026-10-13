//! A collapsible toolbar strip for the bottom of an egui viewport.
//!
//! The strip spans one horizontal edge of a caller-supplied viewport [`Rect`]
//! ([`ToolbarEdge`], bottom by default) and rolls up
//! to the LEFT into a small square button (and back), animated. The widget owns no
//! application types: the HOST draws the actual controls into the strip through a
//! closure, so the same crate carries rv-rs's A/B-compare + RGBA-channel controls,
//! exr-view's display adjustments, or anything else.
//!
//! ```no_run
//! # use egui_viewport_toolbar::{ToolbarState, ViewportToolbar};
//! # fn demo(ui: &mut egui::Ui, viewport: egui::Rect, state: &mut ToolbarState) {
//! let bar = ViewportToolbar::default().show(ui, viewport, state, |ui| {
//!     ui.label("A/B");
//!     ui.label("RGBA");
//! });
//! if bar.toggled {
//!     // `state.expanded` changed — re-save your config.
//! }
//! # }
//! ```
//!
//! # The claimed area (READ THIS — it is the whole contract)
//!
//! The widget claims **exactly the rectangle it paints**, and nothing else:
//!
//! | state      | claimed area                                                        |
//! |------------|---------------------------------------------------------------------|
//! | expanded   | `viewport` inset by `margin`, at least `height` tall, along [`ToolbarEdge`] |
//! | collapsed  | only the `button_width` x `height` toggle button at that edge's left |
//! | animating  | the same strip at its current eased width (left edge fixed)          |
//! | no room    | nothing at all ([`Rect::NOTHING`]) — see [`ViewportToolbar::show`]   |
//!
//! [`ToolbarResponse::rect`] is that rectangle and [`ToolbarResponse::contains_pointer`]
//! is whether the pointer is inside it.
//!
//! # Input leakage — what is automatic and what YOU must do
//!
//! A viewport typically binds raw pointer gestures (drag = scrub, MMB = pan, wheel
//! = zoom, double-click = fit). A toolbar floating on top of it must not fight
//! those bindings. Two halves:
//!
//! * **Automatic (clicks and drags).** The strip is drawn in its own
//!   [`egui::Area`] at [`egui::Order::Middle`], above the host's panel, and it
//!   allocates its whole claimed rect with [`egui::Sense::click_and_drag`] before
//!   drawing anything into it. egui's hit-test therefore resolves a press inside
//!   the strip to the toolbar, and a host widget covering the viewport does not
//!   see it — including presses on the strip's empty background, while animating,
//!   and while collapsed to the button. It also makes
//!   [`egui::Context::is_pointer_over_area`] true over the strip.
//! * **Yours (wheel, and anything read from raw input).** Scroll and key events
//!   are NOT routed by widget hit-testing: a host that reads
//!   `ui.input(|i| i.smooth_scroll_delta)` sees a wheel turn wherever the pointer
//!   is. So gate those handlers on [`ToolbarResponse::contains_pointer`]:
//!
//! ```no_run
//! # use egui_viewport_toolbar::ToolbarResponse;
//! # fn demo(ui: &mut egui::Ui, bar: ToolbarResponse) {
//! if !bar.contains_pointer {
//!     let wheel = ui.input(|i| i.smooth_scroll_delta.y); // viewport zoom
//!     let _ = wheel;
//! }
//! # }
//! ```
//!
//! # Overflow
//!
//! Content wider than the strip scrolls HORIZONTALLY (scrollbar shown only when
//! needed) rather than wrapping. A viewport toolbar is chrome laid over a picture
//! the host has already sized: growing it vertically to wrap would eat the image,
//! and clipping would put controls permanently out of reach. Scrolling keeps the
//! height stable and every control reachable. The strip reserves a separate solid
//! scroll track below the control row; its minimum height follows the current
//! font and button metrics so the scrollbar never overlays the controls.
//!
//! # Viewer backdrop
//!
//! [`backdrop_picker`] is a compact control for what shows under the image's alpha
//! (checker, black, 50% grey, a custom colour): a swatch button opening a drop-up
//! menu, returning the changed [`egui_display::Backdrop`] for the host to persist.
//! Any `Ui` can hold it; inside this strip it goes in the `add_contents` closure.

mod backdrop;
mod controls;
pub use backdrop::backdrop_picker;
pub use controls::exposure_control;

use egui::{Align, Id, Layout, Order, Rect, Sense, StrokeKind, Ui, UiBuilder, pos2, vec2};
use serde::{Deserialize, Serialize};

/// Persisted toolbar state. The host owns this and passes it back each frame;
/// only the width animation stays transient inside egui.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolbarState {
    /// `true` = strip rolled out, `false` = collapsed to the toggle button.
    pub expanded: bool,
    /// Distance in points from the configured edge toward the viewport centre.
    /// Clamped to the visible viewport when drawn, including after a resize.
    pub vertical_offset: f32,
}

impl Default for ToolbarState {
    /// Expanded at the configured edge, so the controls are visible on first run.
    fn default() -> Self {
        Self {
            expanded: true,
            vertical_offset: 0.0,
        }
    }
}

/// Which horizontal edge of the viewport the strip sits on.
///
/// The roll is always horizontal (left edge fixed, right edge travels) — only the
/// vertical anchor changes. `Bottom` is the default because that is where viewer
/// chrome traditionally lives and because it was the widget's only behaviour
/// before this existed; a host that never sets `edge` keeps exactly what it had.
///
/// Pick `Top` when the strip carries controls the user reaches for BEFORE looking
/// at the picture (tools, what-to-render), and `Bottom` when it annotates what
/// they are already looking at (channels, A/B compare).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToolbarEdge {
    Top,
    #[default]
    Bottom,
}

/// The toolbar widget (layout constants only). Cheap to construct each frame.
#[derive(Clone, Copy, Debug)]
pub struct ViewportToolbar {
    /// Minimum strip height in points. The control row and its solid horizontal
    /// scroll track can require a taller strip.
    pub height: f32,
    /// Inset from the viewport's left / right and anchored edges.
    pub margin: f32,
    /// Which edge the strip is anchored to. Default [`ToolbarEdge::Bottom`].
    pub edge: ToolbarEdge,
    /// Width of the square toggle button — also the collapsed width of the whole
    /// strip.
    pub button_width: f32,
    /// Distinguishes several toolbars sharing one parent [`Ui`], and keeps the
    /// animation keyed to this one.
    pub id_salt: &'static str,
    /// Tooltip on the collapsed button. Name what it opens — collapsed, the
    /// button is all the user can see of the toolbar.
    pub expand_tooltip: &'static str,
    /// Tooltip on the button while expanded.
    pub collapse_tooltip: &'static str,
}

impl Default for ViewportToolbar {
    fn default() -> Self {
        Self {
            height: 28.0,
            margin: 8.0,
            button_width: 22.0,
            id_salt: "egui_viewport_toolbar",
            edge: ToolbarEdge::default(),
            expand_tooltip: "Show viewport toolbar",
            collapse_tooltip: "Hide viewport toolbar",
        }
    }
}

/// What [`ViewportToolbar::show`] reports back.
#[derive(Clone, Copy, Debug)]
pub struct ToolbarResponse {
    /// The area the toolbar claimed this frame — exactly what it painted and
    /// interacted with. [`Rect::NOTHING`] when the viewport was too small to draw
    /// in (it contains no point, so `contains` checks stay correct).
    pub rect: Rect,
    /// Pointer is inside [`Self::rect`] or the toggle currently owns a drag.
    /// Gate host wheel / raw-input viewport handling on `!contains_pointer`.
    pub contains_pointer: bool,
    /// The user toggled the strip this frame, so `state.expanded` changed.
    pub toggled: bool,
    /// The user dragged the toggle vertically this frame, changing the saved position.
    pub moved: bool,
    /// The layer of the strip's `Area`. A host that routes input per panel registers the strip
    /// with it (the pointer over the strip hits this layer, not the host's), e.g.
    /// `egui_hotkeys::register_at(.., bar.layer, scope, bar.rect)`.
    pub layer: egui::LayerId,
}

impl ViewportToolbar {
    pub fn new() -> Self {
        Self::default()
    }

    /// Draw the strip along [`Self::edge`] of `viewport` and run `add_contents`
    /// for the controls inside it.
    ///
    /// Draws NOTHING (and claims [`Rect::NOTHING`]) when `viewport` cannot hold
    /// even the collapsed button plus its margins — painting chrome onto a
    /// viewport with no room for it would cover the picture it annotates.
    /// `add_contents` is likewise skipped while the strip is too narrow to show
    /// any of it, so a collapsed toolbar costs nothing to lay out.
    pub fn show(
        &self,
        ui: &mut Ui,
        viewport: Rect,
        state: &mut ToolbarState,
        add_contents: impl FnOnce(&mut Ui),
    ) -> ToolbarResponse {
        let ctx = ui.ctx().clone();
        let cfg = egui_widgets_config::WidgetsConfig::get(ui);
        let metrics = cfg.metrics;
        let id = ui.id().with(self.id_salt);
        let scroll_style = egui::style::ScrollStyle::solid();
        let row_height = ui.spacing().interact_size.y.max(
            ui.text_style_height(&egui::TextStyle::Body) + 2.0 * ui.spacing().button_padding.y,
        );
        let height = self.height.max(row_height + scroll_style.allocated_width());

        let area_id = id.with("area");
        let layer = egui::LayerId::new(Order::Middle, area_id);

        // Negated `>=` so a NaN / inverted viewport takes the bail-out branch too.
        let fits = viewport.width() >= self.button_width + 2.0 * self.margin
            && viewport.height() >= height + 2.0 * self.margin;
        if !fits {
            return ToolbarResponse {
                rect: Rect::NOTHING,
                contains_pointer: false,
                toggled: false,
                moved: false,
                layer,
            };
        }

        let expanded_w = viewport.width() - 2.0 * self.margin;
        let target_w = if state.expanded {
            expanded_w
        } else {
            self.button_width
        };
        // The eased width is what makes the strip roll: the LEFT edge is fixed, so
        // only the right edge travels. Clamped because the animation interpolates
        // from wherever it was — a viewport resize mid-roll can otherwise leave it
        // briefly wider than the viewport it is supposed to sit inside.
        let w = egui_widgets_config::anim::value(&ctx, id.with("width"), target_w, cfg.anim.med)
            .clamp(self.button_width, expanded_w);
        let travel = viewport.height() - 2.0 * self.margin - height;
        let offset = if state.vertical_offset.is_finite() {
            state.vertical_offset.clamp(0.0, travel)
        } else {
            0.0
        };
        let top = match self.edge {
            ToolbarEdge::Top => viewport.top() + self.margin + offset,
            ToolbarEdge::Bottom => viewport.bottom() - self.margin - height - offset,
        };
        let rect = Rect::from_min_size(pos2(viewport.left() + self.margin, top), vec2(w, height));

        // Hosted inside a window (an egui `Window`, a floating dock window: an `Order::Middle`
        // layer), the strip must ride on that window's layer, or a click on the window raises
        // it above the strip. egui links sublayers per pass (`end_pass` drops them), so this
        // runs every frame; it only applies across the same order (egui's own precondition).
        let host = ui.layer_id();
        if host.order == Order::Middle {
            ctx.set_sublayer(host, layer);
        }

        let (toggled, moved, dragging) = egui::Area::new(area_id)
            .order(Order::Middle)
            .fixed_pos(rect.min)
            .constrain_to(viewport)
            .show(&ctx, |ui| {
                ui.set_clip_rect(rect);

                // CLAIM THE STRIP FIRST. This allocation is what stops a press on
                // the toolbar's empty background from falling through to the
                // host's viewport bindings (see the crate docs), and it sizes the
                // Area so the whole strip counts as "pointer over an area".
                // Registered before the button so the button, registered later,
                // wins the overlap.
                let _bg = ui.allocate_rect(rect, Sense::click_and_drag());

                let fill = ui.visuals().window_fill.gamma_multiply(0.92);
                let stroke = ui.visuals().window_stroke;
                ui.painter().rect_filled(rect, metrics.radius_md, fill);
                ui.painter()
                    .rect_stroke(rect, metrics.radius_md, stroke, StrokeKind::Inside);

                let (toggled, moved, dragging) =
                    self.toggle_button(ui, id, rect, offset, travel, state);

                // Content gets whatever the roll has uncovered so far, so the
                // controls slide out with the strip instead of popping in at the
                // end. Below a usable width there is nothing to show.
                let content = Rect::from_min_max(
                    pos2(
                        rect.left() + self.button_width + metrics.space_sm,
                        rect.top(),
                    ),
                    rect.max,
                );
                if content.width() >= metrics.row_h {
                    let mut child = ui.new_child(
                        UiBuilder::new()
                            .max_rect(content)
                            .layout(Layout::left_to_right(Align::Center)),
                    );
                    child.set_clip_rect(content);
                    child.spacing_mut().scroll = scroll_style;
                    egui::ScrollArea::horizontal()
                        .id_salt(id.with("scroll"))
                        .scroll_bar_visibility(
                            egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                        )
                        .show(&mut child, add_contents);
                }
                (toggled, moved, dragging)
            })
            .inner;

        ToolbarResponse {
            rect,
            contains_pointer: dragging
                || ctx.pointer_latest_pos().is_some_and(|p| rect.contains(p)),
            toggled,
            moved,
            layer,
        }
    }

    /// A click rolls the strip; a vertical drag moves it without toggling.
    fn toggle_button(
        &self,
        ui: &mut Ui,
        id: Id,
        rect: Rect,
        offset: f32,
        travel: f32,
        state: &mut ToolbarState,
    ) -> (bool, bool, bool) {
        let btn_rect = Rect::from_min_size(rect.min, vec2(self.button_width, rect.height()));
        let anim = egui_widgets_config::anim(ui);
        // A CHILD id, never the Area's own — re-using an Area id inside itself is
        // egui's "Double use of widget id" (learned in egui-toast).
        let resp = ui.interact(btn_rect, id.with("toggle"), Sense::click_and_drag());
        let tip = if state.expanded {
            self.collapse_tooltip
        } else {
            self.expand_tooltip
        };
        let resp = resp
            .on_hover_cursor(egui::CursorIcon::Grab)
            .on_hover_ui(|ui| {
                ui.label(tip);
                ui.label("Drag up or down to move");
            });

        let hot = egui_widgets_config::anim::bool_factor(
            ui.ctx(),
            id.with("toggle_hot"),
            resp.hovered(),
            anim.fast,
        );
        let visuals = ui.visuals();
        let tint = visuals
            .widgets
            .inactive
            .fg_stroke
            .color
            .lerp_to_gamma(visuals.widgets.hovered.fg_stroke.color, hot);
        // Caret points the way the strip will travel: right = roll out, left = roll up.
        let icon = if state.expanded {
            egui_widgets_config::icons::CARET_LEFT
        } else {
            egui_widgets_config::icons::CARET_RIGHT
        };
        let font = egui::TextStyle::Body.resolve(ui.style());
        ui.painter().text(
            btn_rect.center(),
            egui::Align2::CENTER_CENTER,
            icon,
            font,
            tint,
        );

        let moved = if resp.dragged() {
            let delta = match self.edge {
                ToolbarEdge::Top => resp.drag_delta().y,
                ToolbarEdge::Bottom => -resp.drag_delta().y,
            };
            let next = (offset + delta).clamp(0.0, travel);
            if next != offset {
                state.vertical_offset = next;
                true
            } else {
                false
            }
        } else {
            false
        };
        let toggled = resp.clicked();
        if toggled {
            state.expanded = !state.expanded;
        }
        (toggled, moved, resp.dragged())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Context, Event, Pos2, RawInput, vec2};

    const VIEWPORT: Rect = Rect {
        min: Pos2 { x: 40.0, y: 30.0 },
        max: Pos2 { x: 640.0, y: 430.0 },
    };

    /// A headless host that draws its OWN viewport binding (a click+drag widget
    /// covering the whole viewport, exactly as rv-rs does) and then the toolbar on
    /// top, so the tests can see which of the two gets the pointer.
    struct Host {
        ctx: Context,
        time: f64,
        state: ToolbarState,
        viewport: Rect,
        bar: ViewportToolbar,
    }

    impl Host {
        fn expected_height(&self) -> f32 {
            let style = self.ctx.global_style();
            let text_height = self
                .ctx
                .fonts_mut(|fonts| fonts.row_height(&egui::TextStyle::Body.resolve(&style)));
            let row_height = style
                .spacing
                .interact_size
                .y
                .max(text_height + 2.0 * style.spacing.button_padding.y);
            self.bar
                .height
                .max(row_height + egui::style::ScrollStyle::solid().allocated_width())
        }

        fn new(expanded: bool) -> Self {
            Self {
                ctx: Context::default(),
                time: 0.0,
                state: ToolbarState {
                    expanded,
                    ..Default::default()
                },
                viewport: VIEWPORT,
                bar: ViewportToolbar::default(),
            }
        }

        /// Run one frame. Returns the toolbar's response and whether the HOST's
        /// viewport widget saw the pointer as hovering it.
        fn frame(&mut self, pointer: Option<Pos2>) -> (ToolbarResponse, bool) {
            self.frame_events(pointer.map(Event::PointerMoved).into_iter().collect())
        }

        fn frame_events(&mut self, events: Vec<Event>) -> (ToolbarResponse, bool) {
            self.time += 0.016;
            let input = RawInput {
                time: Some(self.time),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                events,
                ..Default::default()
            };
            let ctx = self.ctx.clone();
            let viewport = self.viewport;
            let bar = self.bar;
            let state = &mut self.state;
            let mut out = None;
            let mut host_hovered = false;
            // `run_ui`'s root Ui sits on `LayerId::background()` — the same layer a
            // host's CentralPanel viewport would, which is exactly the layer the
            // toolbar's Area has to beat.
            let mut output = ctx.run_ui(input, |ui| {
                let host = ui.interact(viewport, Id::new("host_viewport"), Sense::click_and_drag());
                host_hovered = host.hovered();
                out = Some(bar.show(ui, viewport, state, |ui| {
                    ui.label("A/B");
                    ui.label("RGBA");
                }));
            });
            // egui 0.36 made `TexturesDelta` panic on drop with unapplied deltas ("Deltas need to
            // be handled"). These labels upload font glyphs, so the delta is never empty; a
            // headless frame has no painter to apply it to, hence the prescribed `clear`.
            output.textures_delta.clear();
            (out.expect("toolbar ran"), host_hovered)
        }

        fn button_frame(&mut self, pos: Pos2, pressed: bool) -> (ToolbarResponse, bool) {
            self.frame_events(vec![Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }])
        }

        /// Settle for `n` frames and return the last response. Hover resolution
        /// needs the widget rects of a previous frame, so pointer assertions must
        /// never be made on a single frame.
        fn settle(&mut self, n: usize, pointer: Option<Pos2>) -> (ToolbarResponse, bool) {
            let mut last = self.frame(pointer);
            for _ in 1..n {
                last = self.frame(pointer);
            }
            last
        }
    }

    /// Inside a window (an `Order::Middle` layer) the strip is that window's sublayer, so a
    /// click on the window cannot raise it above the strip; on the background it is not.
    #[test]
    fn the_strip_rides_on_a_host_window_layer() {
        let ctx = Context::default();
        let input = || RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
            ..Default::default()
        };
        let bar = ViewportToolbar::default();
        let mut state = ToolbarState::default();
        // (host layer, the strip's parent layer) as egui records them during the pass.
        let mut seen = Vec::new();
        let mut output = ctx.run_ui(input(), |ui| {
            let strip = |ui: &Ui| egui::LayerId::new(Order::Middle, ui.id().with(bar.id_salt).with("area"));
            let shown = bar.show(ui, VIEWPORT, &mut state, |_| {});
            // The response names the Area's layer, which hosts register for input routing.
            assert_eq!(shown.layer, strip(ui));
            seen.push((ui.layer_id(), ui.ctx().memory(|m| m.areas().parent_layer(strip(ui)))));
            egui::Window::new("host").show(ui.ctx(), |ui| {
                bar.show(ui, Rect::from_min_size(ui.max_rect().min, vec2(400.0, 300.0)), &mut state, |_| {});
                seen.push((ui.layer_id(), ui.ctx().memory(|m| m.areas().parent_layer(strip(ui)))));
            });
        });
        output.textures_delta.clear();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].1, None, "a background host links nothing");
        assert_eq!(seen[1].0.order, Order::Middle);
        assert_eq!(seen[1].1, Some(seen[1].0), "the strip is the window's sublayer");
    }

    #[test]
    fn state_round_trips_through_serde() {
        for expanded in [true, false] {
            let s = ToolbarState {
                expanded,
                vertical_offset: 73.5,
            };
            let json = serde_json::to_string(&s).expect("serialize");
            let back: ToolbarState = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(s, back);
        }
        for incomplete in ["{}", r#"{"expanded":false}"#, r#"{"vertical_offset":0}"#] {
            assert!(serde_json::from_str::<ToolbarState>(incomplete).is_err());
        }
    }

    #[test]
    fn overflowing_controls_keep_natural_width_and_do_not_intersect_scroll_track() {
        for large_fonts in [false, true] {
            let ctx = Context::default();
            if large_fonts {
                ctx.all_styles_mut(|style| {
                    style
                        .text_styles
                        .insert(egui::TextStyle::Body, egui::FontId::proportional(28.0));
                    style.spacing.interact_size.y = 44.0;
                    style.spacing.button_padding.y = 1.0;
                });
            }
            let viewport = Rect::from_min_size(Pos2::ZERO, vec2(220.0, 100.0));
            let bar = ViewportToolbar {
                height: 34.0,
                margin: 0.0,
                edge: ToolbarEdge::Top,
                ..Default::default()
            };
            let mut state = ToolbarState::default();
            let mut buttons = Vec::new();
            let mut track = None;
            let mut painted_rects = Vec::new();
            for frame in 0..40 {
                buttons.clear();
                let mut output = ctx.run_ui(
                    RawInput {
                        time: Some(frame as f64 / 60.0),
                        screen_rect: Some(viewport),
                        ..Default::default()
                    },
                    |ui| {
                        bar.show(ui, viewport, &mut state, |ui| {
                            ui.horizontal(|ui| {
                                for index in 0..6 {
                                    buttons.push(ui.button(format!("Control {index}")).rect);
                                }
                            });
                        });
                    },
                );
                track = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Rect(rect)
                            if (rect.rect.height()
                                - egui::style::ScrollStyle::solid().bar_width)
                                .abs()
                                < 0.1 =>
                        {
                            Some(rect.rect)
                        }
                        _ => None,
                    })
                    .max_by(|a, b| a.width().total_cmp(&b.width()));
                painted_rects = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Rect(rect) => Some(rect.rect),
                        _ => None,
                    })
                    .collect();
                output.textures_delta.clear();
            }
            let track = track.unwrap_or_else(|| panic!("horizontal scrollbar was painted: large_fonts={large_fonts}, rects={painted_rects:?}, buttons={buttons:?}"));
            assert!(
                buttons.last().unwrap().right() > viewport.right(),
                "contents must overflow horizontally"
            );
            for pair in buttons.windows(2) {
                assert!(
                    pair[0].right() < pair[1].left(),
                    "controls must retain their natural width and spacing"
                );
            }
            for button in &buttons {
                assert!(
                    button.bottom() <= track.top(),
                    "scroll track must have its own allocated space below the control row"
                );
            }
        }
    }

    #[test]
    fn expanded_claims_the_documented_strip() {
        let bar = ViewportToolbar::default();
        let mut host = Host::new(true);
        let (r, _) = host.settle(30, None);
        assert_eq!(r.rect.left(), VIEWPORT.left() + bar.margin);
        assert_eq!(r.rect.right(), VIEWPORT.right() - bar.margin);
        assert_eq!(r.rect.bottom(), VIEWPORT.bottom() - bar.margin);
        assert_eq!(r.rect.height(), host.expected_height());
    }

    #[test]
    fn collapsed_claims_only_the_button() {
        let bar = ViewportToolbar::default();
        let mut host = Host::new(false);
        let (r, _) = host.settle(30, None);
        assert_eq!(r.rect.width(), bar.button_width);
        assert_eq!(r.rect.left(), VIEWPORT.left() + bar.margin);
        assert_eq!(r.rect.bottom(), VIEWPORT.bottom() - bar.margin);
        assert_eq!(r.rect.height(), host.expected_height());
    }

    /// `Top` anchors to the TOP edge and changes nothing else — same left, same
    /// width, same height. The roll stays horizontal; only the vertical anchor moves.
    #[test]
    fn top_edge_anchors_to_the_top() {
        let mut host = Host::new(true);
        host.bar.edge = ToolbarEdge::Top;
        let bar = host.bar;
        let (r, _) = host.settle(30, None);
        assert_eq!(r.rect.top(), VIEWPORT.top() + bar.margin);
        assert_eq!(r.rect.left(), VIEWPORT.left() + bar.margin);
        assert_eq!(r.rect.right(), VIEWPORT.right() - bar.margin);
        assert_eq!(r.rect.height(), host.expected_height());
    }

    #[test]
    fn large_font_row_sets_one_effective_height_for_edges_roll_and_room_check() {
        for edge in [ToolbarEdge::Top, ToolbarEdge::Bottom] {
            for expanded in [true, false] {
                let mut host = Host::new(expanded);
                host.bar.edge = edge;
                host.ctx.all_styles_mut(|style| {
                    style
                        .text_styles
                        .insert(egui::TextStyle::Body, egui::FontId::proportional(28.0));
                    style.spacing.interact_size.y = 44.0;
                    style.spacing.button_padding.y = 1.0;
                });
                let (response, _) = host.settle(30, None);
                let expected = 44.0 + egui::style::ScrollStyle::solid().allocated_width();
                assert_eq!(response.rect.height(), expected);
                assert_eq!(
                    response.rect.width(),
                    if expanded {
                        VIEWPORT.width() - 2.0 * host.bar.margin
                    } else {
                        host.bar.button_width
                    }
                );
                match edge {
                    ToolbarEdge::Top => {
                        assert_eq!(response.rect.top(), VIEWPORT.top() + host.bar.margin)
                    }
                    ToolbarEdge::Bottom => {
                        assert_eq!(response.rect.bottom(), VIEWPORT.bottom() - host.bar.margin)
                    }
                }
                host.viewport = Rect::from_min_size(
                    VIEWPORT.min,
                    vec2(300.0, expected + 2.0 * host.bar.margin - 1.0),
                );
                assert_eq!(host.frame(None).0.rect, Rect::NOTHING);
            }
        }
    }

    #[test]
    fn toggle_click_and_vertical_drag_are_distinct() {
        for expanded in [true, false] {
            let mut host = Host::new(expanded);
            host.bar.edge = ToolbarEdge::Top;
            let button = host.settle(3, None).0.rect.min + vec2(11.0, 14.0);
            host.settle(3, Some(button));
            host.button_frame(button, true);
            let (clicked, _) = host.button_frame(button, false);
            assert!(clicked.toggled, "plain click must toggle");
            assert!(!clicked.moved);
            assert_eq!(host.state.expanded, !expanded);
            assert_eq!(host.state.vertical_offset, 0.0);

            host.settle(3, Some(button));
            host.button_frame(button, true);
            let drag_to = button + vec2(0.0, 45.0);
            let (drag, _) = host.frame(Some(drag_to));
            assert!(drag.moved, "drag must move the toolbar");
            assert!(!drag.toggled);
            assert!(
                drag.contains_pointer,
                "drag capture must shield the viewport"
            );
            let (released, _) = host.button_frame(drag_to, false);
            assert!(!released.toggled, "drag release must not toggle");
            assert_eq!(host.state.expanded, !expanded);
            assert!(host.state.vertical_offset >= 40.0);
            let (settled, _) = host.settle(2, None);
            assert_eq!(
                settled.rect.top(),
                VIEWPORT.top() + host.bar.margin + host.state.vertical_offset
            );
        }
    }

    #[test]
    fn bottom_edge_drag_moves_up_from_its_anchor() {
        let mut host = Host::new(true);
        let button = host.settle(3, None).0.rect.min + vec2(11.0, 14.0);
        host.settle(3, Some(button));
        host.button_frame(button, true);
        let (drag, _) = host.frame(Some(button - vec2(0.0, 35.0)));
        assert!(drag.moved);
        host.button_frame(button - vec2(0.0, 35.0), false);
        let (settled, _) = host.settle(2, None);
        assert_eq!(
            settled.rect.bottom(),
            VIEWPORT.bottom() - host.bar.margin - host.state.vertical_offset
        );
    }

    #[test]
    fn persisted_offset_is_clamped_to_the_visible_viewport() {
        let mut host = Host::new(false);
        host.bar.edge = ToolbarEdge::Top;
        host.state.vertical_offset = 10_000.0;
        let (at_bottom, _) = host.frame(None);
        assert_eq!(at_bottom.rect.bottom(), VIEWPORT.bottom() - host.bar.margin);
        host.viewport = Rect::from_min_size(VIEWPORT.min, vec2(300.0, 80.0));
        let (resized, _) = host.frame(None);
        assert_eq!(
            resized.rect.bottom(),
            host.viewport.bottom() - host.bar.margin
        );
    }

    /// The default is unchanged by the edge field existing: a host that never sets
    /// it keeps the bottom strip it had before.
    #[test]
    fn default_edge_is_bottom() {
        assert_eq!(ViewportToolbar::default().edge, ToolbarEdge::Bottom);
    }

    #[test]
    fn too_small_viewport_claims_nothing() {
        let mut host = Host::new(true);
        host.viewport = Rect::from_min_size(VIEWPORT.min, vec2(20.0, 20.0));
        let (r, _) = host.frame(None);
        assert_eq!(r.rect, Rect::NOTHING);
        assert!(!r.contains_pointer);
        // `Rect::NOTHING` must not swallow a pointer that is nowhere near it.
        assert!(!r.rect.contains(VIEWPORT.center()));
    }

    #[test]
    fn animation_converges_and_stays_put() {
        let mut host = Host::new(true);
        let expanded_w = host.settle(40, None).0.rect.width();

        // Collapse and let it run well past `Anim::med`.
        host.state.expanded = false;
        let widths: Vec<f32> = (0..40).map(|_| host.frame(None).0.rect.width()).collect();
        let settled = *widths.last().expect("frames ran");
        assert_eq!(settled, ViewportToolbar::default().button_width);
        // It actually animated rather than snapping (some frame sat in between).
        assert!(
            widths.iter().any(|w| *w > settled && *w < expanded_w),
            "expected intermediate widths, got {widths:?}"
        );
        // And it is stable: no jitter once converged.
        for _ in 0..10 {
            assert_eq!(host.frame(None).0.rect.width(), settled);
        }
    }

    #[test]
    fn pointer_over_the_strip_does_not_reach_the_host_viewport() {
        let mut host = Host::new(true);
        let (r, _) = host.settle(30, None);

        // Over the strip's own background: claimed by us, invisible to the host.
        let (bar, host_hovered) = host.settle(3, Some(r.rect.center()));
        assert!(bar.contains_pointer);
        assert!(
            !host_hovered,
            "toolbar leaked a pointer through to the host viewport"
        );

        // Elsewhere in the viewport: the host keeps its bindings.
        let above = pos2(r.rect.center().x, VIEWPORT.top() + 20.0);
        let (bar, host_hovered) = host.settle(3, Some(above));
        assert!(!bar.contains_pointer);
        assert!(host_hovered, "toolbar swallowed input outside its own area");
    }

    #[test]
    fn collapsed_toolbar_only_claims_the_button_not_the_old_strip() {
        let mut host = Host::new(false);
        let (r, _) = host.settle(30, None);

        // Where the EXPANDED strip used to be — must belong to the host again.
        let vacated = pos2(VIEWPORT.right() - 60.0, r.rect.center().y);
        let (bar, host_hovered) = host.settle(3, Some(vacated));
        assert!(!bar.contains_pointer);
        assert!(
            host_hovered,
            "collapsed toolbar still claimed the full strip"
        );

        // The button itself is still ours.
        let (bar, host_hovered) = host.settle(3, Some(r.rect.center()));
        assert!(bar.contains_pointer);
        assert!(!host_hovered);
    }
}
