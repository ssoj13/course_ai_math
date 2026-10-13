//! The widget entry point: [`TrackTimeline::show`].
//!
//! Ties together input handling, the drag state machine ([`DragState`]) and the
//! pure painters in [`crate::paint`]. The widget mutates [`TimelineView`] in
//! place (zoom / pan / drag) and returns a [`TimelineResponse`] carrying the
//! model edits plus layout rects and hover/double-click hit-tests for the caller
//! to apply / layer on. It never mutates the [`TimelineModel`].

use egui::{Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2};

use crate::actions::TimelineAction;
use crate::model::{Clip, ClipId, KeyPos, TimelineModel, Track};
use crate::paint::{self, ClipGeom, Tool};
use crate::view::{
    DragState, TimelineConfig, TimelineView, ZOOM_MAX, ZOOM_MIN, track_top, y_to_track,
};

/// What [`TrackTimeline::show`] reports back this frame.
///
/// `actions` are the model edit intents (as before); the remaining fields expose
/// the widget's internal layout + hit-testing so a host can layer its own
/// features (context menus, custom overlays, tooltips) on top *without* a second
/// hit-test pass. All are derived from the same geometry the widget itself uses.
#[derive(Clone, Debug)]
pub struct TimelineResponse {
    /// Edit intents produced this frame, for the caller to apply to its model.
    pub actions: Vec<TimelineAction>,
    /// The allocated ruler rect (top strip with the frame ticks).
    pub ruler_rect: Rect,
    /// The allocated track-area rect (the stacked rows below the ruler).
    pub track_rect: Rect,
    /// Clip whose full bar currently contains the pointer (`None` if none).
    pub hovered: Option<ClipId>,
    /// Clip double-clicked this frame (`None` if the double-click missed a clip).
    pub double_clicked: Option<ClipId>,
    /// Screen-space top Y of each track, in `model.tracks` order (prefix-sum of
    /// [`crate::Track::height`] from [`TimelineResponse::track_rect`]'s top). The
    /// host outline mirrors these to align its rows 1:1 with the widget — it must
    /// never re-derive them from a uniform row height.
    pub track_tops: Vec<f32>,
    /// The effective lane sub-row height used this frame (from the config), so the
    /// host outline can size its expanded lanes identically.
    pub lane_height: f32,
    /// Total painted content height of the track area (sum of every track's
    /// [`crate::Track::height`]); equals `track_rect.height()`.
    pub content_height: f32,
}

/// Stateless widget facade. Holds only static config; the mutable view state and
/// the model are passed to [`TrackTimeline::show`] each frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct TrackTimeline {
    config: TimelineConfig,
}

impl TrackTimeline {
    /// Create a widget with the given layout config.
    pub fn new(config: TimelineConfig) -> Self {
        Self { config }
    }

    /// Borrow the layout config.
    pub fn config(&self) -> &TimelineConfig {
        &self.config
    }

    /// Render the timeline and return a [`TimelineResponse`] (edit intents plus
    /// layout rects and hover / double-click hit-tests) for this frame.
    ///
    /// * `view` — widget-owned view state, mutated in place (zoom / pan / drag).
    /// * `model` — read-only input the caller rebuilt this frame.
    pub fn show(
        &self,
        ui: &mut Ui,
        view: &mut TimelineView,
        model: &TimelineModel,
    ) -> TimelineResponse {
        self.show_impl(ui, view, model, None)
    }

    /// Keep the ruler at `top` in screen coordinates while the host scrolls rows.
    /// Logical track positions and content height remain unchanged. Painting and
    /// new row gestures are clipped below the ruler, so hidden keys and bars
    /// cannot intercept scrubbing. The same view owns all timeline gestures.
    pub fn show_pinned_ruler(
        &self,
        ui: &mut Ui,
        view: &mut TimelineView,
        model: &TimelineModel,
        top: f32,
    ) -> TimelineResponse {
        self.show_impl(ui, view, model, Some(top))
    }

    fn show_impl(
        &self,
        ui: &mut Ui,
        view: &mut TimelineView,
        model: &TimelineModel,
        ruler_top: Option<f32>,
    ) -> TimelineResponse {
        let config = &self.config;
        // Design tokens for this frame: fonts/metrics (`tok`) + theme-resolved
        // semantic colours (`sem`). Resolved once and handed to the pure painters.
        let tok = egui_widgets_config::WidgetsConfig::get(ui);
        let sem = egui_widgets_config::semantic(ui);
        let mut actions = Vec::new();

        // ---- Layout metrics ------------------------------------------------
        let avail_w = ui.available_width();

        // Extended frame range so dragging past the edges still has canvas.
        let (mut min_f, mut max_f) = model.frame_bounds().unwrap_or((0, 100));
        min_f = min_f.min(model.playhead);
        max_f = max_f.max(model.playhead + 1);
        let margin = (avail_w / view.ppf(config)).ceil() as i64 + 100;
        let lo = min_f - margin;
        let hi = max_f + margin;
        let total_frames = (hi - lo + 1).max(100);
        let content_w = (total_frames as f32 * view.ppf(config)).max(avail_w);

        let ruler_h = 20.0;
        // Cumulative height: each track contributes its own `height()` (clip row
        // plus lane bands when expanded), not a uniform `row_height`. Keep a
        // one-row floor so an empty timeline still allocates a sensible strip.
        let rows_h = model
            .tracks
            .iter()
            .map(|t| t.height(config.row_height, config.lane_height))
            .sum::<f32>()
            .max(config.row_height);

        ui.spacing_mut().item_spacing.y = 0.0;

        // ---- Ruler ---------------------------------------------------------
        let (logical_ruler, _) =
            ui.allocate_exact_size(Vec2::new(content_w, ruler_h), Sense::hover());
        let ruler_rect = ruler_top.map_or(logical_ruler, |top| {
            Rect::from_min_size(Pos2::new(logical_ruler.left(), top), logical_ruler.size())
        });
        let ruler_resp = ui.interact(
            ruler_rect.intersect(ui.clip_rect()),
            ui.id().with("timeline-ruler"),
            Sense::click_and_drag(),
        );
        paint::draw_ruler(
            ui.painter(),
            ruler_rect,
            model.playhead,
            &model.markers,
            config,
            view,
            &tok,
            &sem,
        );

        let middle_down = ui
            .ctx()
            .input(|i| i.pointer.button_down(egui::PointerButton::Middle));

        // WORK-AREA HANDLES, tested BEFORE the scrub.
        //
        // They sit ON the ruler, where dragging otherwise seeks. If the scrub ran first, every
        // attempt to pull a handle would jump the playhead instead - so a handle claims the
        // gesture and only a miss falls through to Seek.
        let handles = model.work_area.filter(|_| config.show_work_area).map(|wa| {
            let sx = view.frame_to_x(wa.start as f32, ruler_rect.min.x, config);
            let ex = view.frame_to_x(wa.end as f32, ruler_rect.min.x, config);
            (sx, ex, wa)
        });
        if let Some((sx, ex, _)) = handles {
            paint::draw_work_handles(ui.painter(), ruler_rect, sx, ex, &sem);
        }

        if let Some((sx, ex, wa)) = handles
            && !middle_down
            && ruler_resp.drag_started()
            && let Some(pos) = ruler_resp.interact_pointer_pos()
            && let Some(moving_end) = paint::work_handle_at(pos.x, sx, ex)
        {
            // The opposite edge is captured ONCE, here. Reading it from the model each frame
            // would let a caller that applies the action mid-drag feed its own output back in.
            view.drag = Some(DragState::WorkArea {
                moving_end,
                other: if moving_end { wa.start } else { wa.end },
            });
        }

        let dragging_work = matches!(view.drag, Some(DragState::WorkArea { .. }));
        if let Some(DragState::WorkArea { moving_end, other }) = view.drag
            && let Some(pos) = ruler_resp.interact_pointer_pos()
        {
            // Read the handle from the LIVE cursor, not an accumulated delta, so a long drag
            // cannot creep away from the pointer.
            let f = view.x_to_frame(pos.x, ruler_rect.min.x, config).round() as i64;
            let (start, end) = if moving_end {
                (other, f.max(other + 1))
            } else {
                (f.min(other - 1), other)
            };
            actions.push(TimelineAction::SetWorkArea { start, end });
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if dragging_work && ruler_resp.drag_stopped() {
            view.drag = None;
        }

        // Ruler scrub (left button) -> Seek.
        if !middle_down
            && !dragging_work
            && (ruler_resp.clicked() || ruler_resp.dragged())
            && let Some(pos) = ruler_resp.interact_pointer_pos()
        {
            let frame = view.x_to_frame(pos.x, ruler_rect.min.x, config).round() as i64;
            actions.push(TimelineAction::Seek { frame });
        }

        ui.add_space(4.0);

        // ---- Track area ----------------------------------------------------
        let full_clip = ui.clip_rect();
        if ruler_top.is_some() {
            let mut rows_clip = full_clip;
            rows_clip.min.y = rows_clip.min.y.max(ruler_rect.bottom() + 4.0);
            ui.set_clip_rect(rows_clip);
        }
        let (area, area_resp) =
            ui.allocate_exact_size(Vec2::new(content_w, rows_h), Sense::click_and_drag());
        let tab_rect = ruler_rect.union(area);
        // Logical content extends beyond the viewport for scrolling. New gestures
        // belong only to the visible part on this UI layer; existing drags may
        // continue outside it until release.
        let pointer_inside = ui.rect_contains_pointer(tab_rect.intersect(ui.clip_rect()));

        paint::draw_row_backgrounds(ui.painter(), area, &model.tracks, config, &sem);
        // What of the area is on screen: every paint loop below culls with it.
        let screen = ui.clip_rect();
        let frames = view.visible_frames(area, screen, config);

        // Prefix-sum of every track's top Y. Computed once and reused for clip
        // geometry, lane painting, keyframe hit-tests, and the host-facing
        // `track_tops` export — the single source of vertical layout.
        let mut track_tops: Vec<f32> = Vec::with_capacity(model.tracks.len());
        {
            let mut y = area.min.y;
            for track in &model.tracks {
                track_tops.push(y);
                y += track.height(config.row_height, config.lane_height);
            }
        }

        // Compute clip geometry once; reused for paint + interaction.
        let mut geoms: Vec<Vec<ClipGeom>> = Vec::with_capacity(model.tracks.len());
        for (t, track) in model.tracks.iter().enumerate() {
            let row_y = track_tops[t];
            let mut row = Vec::with_capacity(track.clips.len());
            for clip in &track.clips {
                row.push(ClipGeom::place(
                    clip, track, row_y, area.min.x, config, view,
                ));
            }
            geoms.push(row);
        }

        // Paint clips: the ones on screen (lane clips after their lanes, below).
        for (t, track) in model.tracks.iter().enumerate() {
            for (c, clip) in track.clips.iter().enumerate() {
                if clip.lane.is_some() || !geoms[t][c].full.intersects(screen) {
                    continue;
                }
                let selected = model.selection.contains(&clip.id);
                paint::draw_clip(
                    ui.painter(),
                    clip,
                    &geoms[t][c],
                    selected,
                    false,
                    &tok,
                    &sem,
                );
            }
        }

        // Paint expanded property lanes below their clip row: the ones on screen.
        for (t, track) in model.tracks.iter().enumerate() {
            let Some(frames) = frames.filter(|_| track.expanded) else {
                continue;
            };
            for (l, lane) in track.lanes.iter().enumerate() {
                let lane_top = config.lane_center_y(track_tops[t], l) - config.lane_height / 2.0;
                let lane_rect = Rect::from_min_size(
                    Pos2::new(area.min.x, lane_top),
                    Vec2::new(area.width(), config.lane_height),
                );
                if !lane_rect.intersects(screen) {
                    continue;
                }
                paint::draw_lane(ui.painter(), lane_rect, lane, frames, config, view, &sem);
            }
        }
        for (t, track) in model.tracks.iter().enumerate() {
            for (c, clip) in track.clips.iter().enumerate() {
                if clip.lane.is_none() || !geoms[t][c].full.intersects(screen) {
                    continue;
                }
                let selected = model.selection.contains(&clip.id);
                paint::draw_clip(
                    ui.painter(),
                    clip,
                    &geoms[t][c],
                    selected,
                    false,
                    &tok,
                    &sem,
                );
            }
        }

        // ---- Input: pan init (middle drag) --------------------------------
        if let Some(pos) = ui.ctx().pointer_hover_pos()
            && pointer_inside
            && view.drag.is_none()
            && middle_down
        {
            view.drag = Some(DragState::Pan {
                start_x: pos.x,
                initial_pan_offset: view.pan_offset,
            });
        }

        // ---- Input: scroll-wheel horizontal pan ---------------------------
        let scroll = ui.ctx().input(|i| i.smooth_scroll_delta);
        if pointer_inside && scroll.x.abs() > 0.0 {
            view.pan_offset -= scroll.x / view.ppf(config);
        }

        // ---- Input: tool hover + gesture start ----------------------------
        if view.drag.is_none()
            && pointer_inside
            && let Some(pos) = ui.ctx().input(|i| i.pointer.interact_pos())
            && let Some((t, c, tool)) = Self::hit_tool(model, &geoms, pos, config.edge_threshold)
            // AE-style pick priority: a keyframe diamond sits on top of a clip's
            // trim/slide fringe (the thin strip where the dilated clip bar overlaps
            // lane 0). When a key is under the cursor and outranks the tool
            // (`prio_key > prio_tool`), yield the whole gesture — cursor feedback and
            // the press claim — to the keyframe block below so the diamond wins.
            && !(config.prio_key > config.prio_tool
                && Self::hit_key(model, &track_tops, pos, area, config, view).is_some())
        {
            ui.ctx().set_cursor_icon(tool.cursor());
            if ui.ctx().input(|i| i.pointer.primary_pressed()) {
                let clip = &model.tracks[t].clips[c];
                // Selecting an unselected clip on press makes the gesture operate on
                // it (drag-to-select). Record whether we emitted here so the body-click
                // handler below does NOT emit a duplicate Select for the same gesture
                // (a clip click always starts+ends a drag, so both sites would fire —
                // a ctrl-click would then toggle on, then off = no-op).
                let mut emitted_select = false;
                if !model.selection.contains(&clip.id) {
                    let (additive, range) = read_modifiers(ui);
                    actions.push(TimelineAction::Select {
                        id: clip.id,
                        additive,
                        range,
                    });
                    emitted_select = true;
                }
                let press_id = ui.id().with("egui_track_timeline_press_select");
                egui_widgets_config::state::set(ui.ctx(), press_id, emitted_select);
                view.drag = Some(Self::begin_drag(tool, t, c, clip, pos));
            }
        }

        // ---- Input: keyframe (property-lane) gestures ---------------------
        // Full AE-style interaction matrix (see `TimelineAction`). Runs only when
        // no clip gesture claimed the press: lane bands sit strictly below the
        // clip band, so these hit-tests are disjoint from `hit_tool`/`hit_clip`.
        //
        //   modifier | on diamond              | on empty lane
        //   ---------|-------------------------|----------------------
        //   LMB click| SelectKeys(replace)     | ClearKeySelection
        //   LMB drag | MoveKeys (selection)    | Marquee(replace)
        //   Shift+LMB| SelectKeys(add) / drag  | Marquee(add)
        //   Ctrl+LMB | AddKey @cursor          | AddKey @cursor
        //   Ctrl+RMB | DeleteKeys([this])      | —
        //   RMB      | context menu (Delete)   | context menu (Delete)
        //   Del/Bksp | DeleteKeys(selection, when the area holds focus)
        //
        // `area_resp` (the click_and_drag Sense) provides the ~6px click-vs-drag
        // threshold so a high-zoom click never emits a 1-frame move.
        {
            let shift = ui.ctx().input(|i| i.modifiers.shift);
            let ctrl = ui.ctx().input(|i| i.modifiers.ctrl || i.modifiers.command);
            let press_pos = if pointer_inside {
                ui.ctx().input(|i| i.pointer.interact_pos())
            } else {
                None
            };

            // Ctrl+RMB on a diamond -> delete just that key (bypasses the menu).
            if ctrl
                && ui.ctx().input(|i| i.pointer.secondary_clicked())
                && let Some(pos) = press_pos
                && let Some((t, l, frame)) =
                    Self::hit_key(model, &track_tops, pos, area, config, view)
            {
                actions.push(TimelineAction::DeleteKeys {
                    keys: vec![KeyPos {
                        track: t,
                        lane: l,
                        frame,
                    }],
                });
            }

            // Ctrl+LMB press anywhere in a lane -> create a key at the cursor's time (the
            // nearest whole frame while `snap_keys_to_frames`, else the exact time).
            if view.drag.is_none()
                && ctrl
                && ui.ctx().input(|i| i.pointer.primary_pressed())
                && let Some(pos) = press_pos
                && let Some((t, l)) = Self::hit_lane(model, &track_tops, pos, area, config)
            {
                let frame = crate::view::key_time(
                    f64::from(view.x_to_frame(pos.x, area.min.x, config)),
                    config.snap_keys_to_frames,
                );
                actions.push(TimelineAction::AddKey {
                    track: t,
                    lane: l,
                    frame,
                });
                area_resp.request_focus();
            }

            // LMB click (no drag): select a diamond (replace / add) or clear on
            // an empty lane. `area_resp.clicked()` fires only for a press+release
            // without a drag, so this never races the MoveKeys/Marquee drag below.
            if !ctrl
                && area_resp.clicked()
                && let Some(pos) = area_resp.interact_pointer_pos()
            {
                if let Some((t, l, frame)) =
                    Self::hit_key(model, &track_tops, pos, area, config, view)
                {
                    actions.push(TimelineAction::SelectKeys {
                        keys: vec![KeyPos {
                            track: t,
                            lane: l,
                            frame,
                        }],
                        add: shift,
                    });
                    area_resp.request_focus();
                } else if !shift && Self::hit_lane(model, &track_tops, pos, area, config).is_some()
                {
                    actions.push(TimelineAction::ClearKeySelection);
                    area_resp.request_focus();
                }
            }

            // LMB drag-start: block-move the selection (on a diamond) or
            // marquee-select (on empty lane space). Ctrl is reserved for AddKey.
            if view.drag.is_none()
                && !ctrl
                && area_resp.drag_started()
                && let Some(pos) = area_resp.interact_pointer_pos()
            {
                if let Some((t, l, frame)) =
                    Self::hit_key(model, &track_tops, pos, area, config, view)
                {
                    let anchor = KeyPos {
                        track: t,
                        lane: l,
                        frame,
                    };
                    // AE select-on-mousedown: if the grabbed key isn't already
                    // selected, make it THE selection so the drag has something to
                    // move and the host selection follows. The move/preview set
                    // below ALWAYS includes the anchor regardless of the 1-frame
                    // select round-trip, so a lone unselected key drags too.
                    let anchor_selected = model
                        .tracks
                        .get(t)
                        .and_then(|tr| tr.lanes.get(l))
                        .map(|ln| ln.keys.iter().any(|k| k.frame == frame && k.selected))
                        .unwrap_or(false);
                    if !anchor_selected {
                        actions.push(TimelineAction::SelectKeys {
                            keys: vec![anchor],
                            add: false,
                        });
                    }
                    view.drag = Some(DragState::MoveKeys {
                        anchor,
                        start_x: pos.x,
                    });
                    area_resp.request_focus();
                } else if Self::hit_lane(model, &track_tops, pos, area, config).is_some() {
                    view.drag = Some(DragState::Marquee {
                        start: pos,
                        add: shift,
                    });
                    area_resp.request_focus();
                }
            }

            // Delete / Backspace: remove the selection, but only when the track
            // area holds keyboard focus — otherwise we'd steal Del from a focused
            // host TextEdit sharing the same frame.
            if area_resp.has_focus()
                && ui.ctx().input(|i| {
                    i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)
                })
            {
                let keys = Self::selected_keys(model);
                if !keys.is_empty() {
                    actions.push(TimelineAction::DeleteKeys { keys });
                }
            }

            // Plain RMB (no Ctrl): context menu offering "Delete" for the current
            // selection. Ctrl+RMB above already handled the single-key delete.
            if !ctrl {
                area_resp.context_menu(|ui| {
                    let keys = Self::selected_keys(model);
                    if ui
                        .add_enabled(!keys.is_empty(), egui::Button::new("Delete"))
                        .clicked()
                    {
                        actions.push(TimelineAction::DeleteKeys { keys });
                        ui.close();
                    }
                });
            }
        }

        // ---- Drag processing ----------------------------------------------
        if let Some(drag) = view.drag {
            let mut keep = true;
            let released = ui.ctx().input(|i| i.pointer.any_released());
            if let Some(cur) = ui.ctx().input(|i| i.pointer.latest_pos()) {
                match drag {
                    DragState::Pan {
                        start_x,
                        initial_pan_offset,
                    } => {
                        // Pan mutates the view directly (no action emitted).
                        let delta = (cur.x - start_x) / view.ppf(config);
                        view.pan_offset = initial_pan_offset - delta;
                        if released {
                            keep = false;
                        }
                    }
                    other => {
                        self.process_clip_drag(
                            ui,
                            view,
                            model,
                            &track_tops,
                            &other,
                            cur,
                            area,
                            released,
                            &mut actions,
                            &mut keep,
                        );
                    }
                }
            }
            if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
                keep = false;
            }
            view.drag = if keep { Some(drag) } else { None };
        }

        // ---- Work area overlay --------------------------------------------
        if config.show_work_area
            && let Some(wa) = model.work_area
        {
            paint::draw_work_area(ui.painter(), area, wa.start, wa.end, config, view);
        }

        // ---- Empty-space / body click: select / seek / clear --------------
        if view.drag.is_none()
            && pointer_inside
            && let Some(pos) = ui.ctx().input(|i| i.pointer.interact_pos())
            && tab_rect.contains(pos)
            && !ruler_rect.contains(pos)
            && area_resp.clicked()
            && !middle_down
        {
            // A press inside an expanded lane band is a keyframe gesture (handled
            // by the lane input block above); don't also seek/select/clear here.
            if Self::hit_lane(model, &track_tops, pos, area, config).is_some() {
                // Intentionally empty: handled by the keyframe-lane gesture path.
            } else if let Some(id) = Self::hit_band(model, &geoms, pos) {
                // A band's click picks what it marks: its own id, in place of the clip.
                let press_id = ui.id().with("egui_track_timeline_press_select");
                egui_widgets_config::state::clear::<bool>(ui.ctx(), press_id);
                let (additive, range) = read_modifiers(ui);
                actions.push(TimelineAction::Select {
                    id,
                    additive,
                    range,
                });
            } else if let Some(id) = Self::hit_clip(model, &geoms, pos) {
                // Skip the Select if the gesture-start handler already emitted one for
                // this same click (avoids the ctrl-click toggle firing twice).
                let press_id = ui.id().with("egui_track_timeline_press_select");
                let press_emitted =
                    egui_widgets_config::state::get::<bool>(ui.ctx(), press_id).unwrap_or(false);
                egui_widgets_config::state::clear::<bool>(ui.ctx(), press_id);
                if !press_emitted {
                    let (additive, range) = read_modifiers(ui);
                    actions.push(TimelineAction::Select {
                        id,
                        additive,
                        range,
                    });
                }
            } else {
                if pos.x >= area.min.x && pos.x <= area.max.x {
                    let frame = view.x_to_frame(pos.x, area.min.x, config).round() as i64;
                    actions.push(TimelineAction::Seek { frame });
                }
                if !model.selection.is_empty() {
                    actions.push(TimelineAction::ClearSelection);
                }
            }
        }

        // Restore the viewport before drawing the fixed playhead head and stem.
        let rows_clip = ui.clip_rect();
        ui.set_clip_rect(full_clip);
        // ---- Playhead ------------------------------------------------------
        paint::draw_playhead(
            ui.painter(),
            ruler_rect,
            tab_rect.max.y,
            model.playhead,
            view,
            config,
            &sem,
        );

        // ---- Zoom (ctrl+wheel / +/- keys) when hovered --------------------
        if pointer_inside {
            let wheel = ui.ctx().input(|i| i.smooth_scroll_delta.y);
            if wheel != 0.0 && ui.ctx().input(|i| i.modifiers.ctrl) {
                view.apply_zoom(if wheel > 0.0 { 1.1 } else { 0.9 });
            }
            ui.ctx().input(|i| {
                if i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals) {
                    view.zoom = (view.zoom * 1.2).clamp(ZOOM_MIN, ZOOM_MAX);
                }
                if i.key_pressed(egui::Key::Minus) {
                    view.zoom = (view.zoom / 1.2).clamp(ZOOM_MIN, ZOOM_MAX);
                }
            });
        }

        // ---- Host-facing hit-test results (reuse the same clip geometry) ----
        // `hovered`: topmost clip under the live pointer, if any.
        let hovered = ui
            .ctx()
            .pointer_hover_pos()
            .filter(|p| rows_clip.contains(*p))
            .and_then(|p| Self::hit_clip(model, &geoms, p));
        // `double_clicked`: the track-area's double-click, resolved to a clip via
        // the same hit-test (None when the double-click landed on empty space).
        let double_clicked = if area_resp.double_clicked() {
            area_resp
                .interact_pointer_pos()
                .and_then(|p| Self::hit_clip(model, &geoms, p))
        } else {
            None
        };

        TimelineResponse {
            actions,
            ruler_rect,
            track_rect: area,
            hovered,
            double_clicked,
            track_tops,
            lane_height: config.lane_height,
            content_height: area.height(),
        }
    }

    /// Topmost clip id whose *full* bar contains `pos` (top track first,
    /// last-drawn clip first — matching the paint/interaction z-order). Shared by
    /// the empty-space click path, hover and double-click so there is a single
    /// hit-test definition.
    fn hit_clip(model: &TimelineModel, geoms: &[Vec<ClipGeom>], pos: Pos2) -> Option<ClipId> {
        for (t, track) in model.tracks.iter().enumerate() {
            for c in (0..track.clips.len()).rev() {
                if geoms[t][c].full.contains(pos) {
                    return Some(track.clips[c].id);
                }
            }
        }
        None
    }

    /// The id of the band (`Clip::bands`) whose click zone holds `pos`, topmost clip first.
    fn hit_band(model: &TimelineModel, geoms: &[Vec<ClipGeom>], pos: Pos2) -> Option<ClipId> {
        for (t, track) in model.tracks.iter().enumerate() {
            for c in (0..track.clips.len()).rev() {
                if geoms[t][c].full.contains(pos)
                    && let Some(id) = paint::hit_band(&track.clips[c], &geoms[t][c], pos)
                {
                    return Some(id);
                }
            }
        }
        None
    }

    /// Find the `(track, clip, tool)` under `pos`, scanning top track first and
    /// later clips first (last-drawn = topmost).
    fn hit_tool(
        model: &TimelineModel,
        geoms: &[Vec<ClipGeom>],
        pos: Pos2,
        threshold: f32,
    ) -> Option<(usize, usize, Tool)> {
        for (t, track) in model.tracks.iter().enumerate() {
            for c in (0..track.clips.len()).rev() {
                if let Some(tool) = paint::detect_tool(pos, &geoms[t][c], threshold) {
                    return Some((t, c, tool));
                }
            }
        }
        None
    }

    /// Keyframe diamond nearest to `pos`, as `(track, lane, key_frame)`. Scans
    /// expanded tracks only; a diamond is a candidate when `pos` falls inside its
    /// lane band vertically and within the drawn radius + `config.key_grab_pad`
    /// of its x on the shared time axis - its EXACT position, a key between frames
    /// included. Among candidates the one closest in x
    /// wins, so clicking between two clustered diamonds grabs the intended one
    /// instead of the first scanned — this is what makes the hit-test robust. The
    /// returned frame is the key's own [`crate::Keyframe::frame`], never one
    /// recomputed from `pos`.
    fn hit_key(
        model: &TimelineModel,
        track_tops: &[f32],
        pos: Pos2,
        area: Rect,
        config: &TimelineConfig,
        view: &TimelineView,
    ) -> Option<(usize, usize, f64)> {
        // Grab half-width mirrors the drawn diamond radius (see `paint::draw_lane`)
        // plus the configured padding token, so the hit zone tracks the glyph size
        // at any density and is tunable via `TimelineMetrics::key_grab_pad`.
        let grab = config.key_radius() + config.key_grab_pad;
        let mut best: Option<(usize, usize, f64)> = None;
        let mut best_dx = f32::INFINITY;
        for (t, track) in model.tracks.iter().enumerate() {
            if !track.expanded {
                continue;
            }
            for (l, lane) in track.lanes.iter().enumerate() {
                let cy = config.lane_center_y(track_tops[t], l);
                if (pos.y - cy).abs() > config.lane_height / 2.0 {
                    continue;
                }
                for key in &lane.keys {
                    let x = view.frame_to_x(key.frame as f32, area.min.x, config);
                    let dx = (pos.x - x).abs();
                    if dx <= grab && dx < best_dx {
                        best_dx = dx;
                        best = Some((t, l, key.frame));
                    }
                }
            }
        }
        best
    }

    /// Which expanded lane band contains `pos` (ignoring diamonds), as
    /// `(track, lane)`. Routes empty-lane clicks to `AddKey` and keeps lane
    /// clicks out of the body-click seek/clear path.
    fn hit_lane(
        model: &TimelineModel,
        track_tops: &[f32],
        pos: Pos2,
        area: Rect,
        config: &TimelineConfig,
    ) -> Option<(usize, usize)> {
        if pos.x < area.min.x || pos.x > area.max.x {
            return None;
        }
        for (t, track) in model.tracks.iter().enumerate() {
            if !track.expanded {
                continue;
            }
            let lanes_top = track_tops[t] + config.row_height;
            for l in 0..track.lanes.len() {
                let top = lanes_top + l as f32 * config.lane_height;
                if pos.y >= top && pos.y < top + config.lane_height {
                    return Some((t, l));
                }
            }
        }
        None
    }

    /// Every keyframe currently flagged [`crate::Keyframe::selected`], as
    /// positional [`KeyPos`]es (scanning all lanes, expanded or not). This is the
    /// widget's view of "the selection"; the host maps each `KeyPos` to its own
    /// domain identity. Used by Delete/Backspace, the RMB menu, and block-move.
    fn selected_keys(model: &TimelineModel) -> Vec<KeyPos> {
        let mut keys = Vec::new();
        for (t, track) in model.tracks.iter().enumerate() {
            for (l, lane) in track.lanes.iter().enumerate() {
                for key in &lane.keys {
                    if key.selected {
                        keys.push(KeyPos {
                            track: t,
                            lane: l,
                            frame: key.frame,
                        });
                    }
                }
            }
        }
        keys
    }

    /// Snap radius in FRAMES for the current zoom, or `-1` when snapping is off.
    ///
    /// `config.snap_threshold` is in points on purpose (see its doc); converting here is what
    /// makes the magnet feel the same whether one frame is 40 points wide or a fortieth of one.
    fn snap_dist(view: &TimelineView, config: &TimelineConfig) -> i64 {
        if config.snap_threshold <= 0.0 {
            return -1;
        }
        view.px_to_frames(config.snap_threshold, config).max(1)
    }

    /// Build the initial [`DragState`] for a started gesture.
    fn begin_drag(tool: Tool, track: usize, clip: usize, clip_data: &Clip, pos: Pos2) -> DragState {
        match tool {
            Tool::Move => DragState::Move {
                track,
                clip,
                initial_start: clip_data.start,
                start_x: pos.x,
            },
            Tool::TrimStart => DragState::TrimStart {
                track,
                clip,
                start_x: pos.x,
            },
            Tool::TrimEnd => DragState::TrimEnd {
                track,
                clip,
                start_x: pos.x,
            },
            Tool::Slide => DragState::Slide {
                track,
                clip,
                start_x: pos.x,
            },
        }
    }

    /// Advance a clip-editing drag (everything except Pan): paint its ghost and,
    /// on release, push the corresponding action.
    #[allow(clippy::too_many_arguments)]
    fn process_clip_drag(
        &self,
        ui: &Ui,
        view: &TimelineView,
        model: &TimelineModel,
        track_tops: &[f32],
        drag: &DragState,
        cur: Pos2,
        area: Rect,
        released: bool,
        actions: &mut Vec<TimelineAction>,
        keep: &mut bool,
    ) {
        let config = &self.config;
        let painter = ui.painter();

        match *drag {
            DragState::Pan { .. } => {}
            // Driven on the RULER, before the track area is even allocated - the handles live
            // there, and the gesture is resolved against the ruler's own rect. Nothing to do
            // here, and nothing may be added: a second driver would fight the first.
            DragState::WorkArea { .. } => {}
            DragState::Move {
                track,
                clip,
                initial_start,
                start_x,
            } => {
                let Some(c) = model.tracks.get(track).and_then(|t| t.clips.get(clip)) else {
                    *keep = false;
                    return;
                };
                let dframes = view.px_to_frames(cur.x - start_x, config);
                let raw_start = initial_start + dframes;
                let targets = crate::view::snap_targets(model, Some((track, clip)));
                let snap_r = Self::snap_dist(view, config);
                let new_start = crate::view::snap_move(raw_start, c.duration, &targets, snap_r);
                let shift = ui.input(|i| i.modifiers.shift);
                // Destination track from the live cursor Y via the cumulative
                // lookup (NOT `dy / row_height`, which is wrong once rows vary in
                // height). Falls back to the source track when the cursor leaves
                // the track area, then clamps into range.
                // Shift locks the row (AE axis constrain on a 2D drag); a lane clip belongs
                // to its track's property and never changes track.
                let new_track = if shift || c.lane.is_some() {
                    track
                } else {
                    y_to_track(
                        &model.tracks,
                        cur.y,
                        area.min.y,
                        config.row_height,
                        config.lane_height,
                    )
                    .unwrap_or(track)
                    .min(model.tracks.len().saturating_sub(1))
                };

                let row_y = track_top(
                    &model.tracks,
                    new_track,
                    area.min.y,
                    config.row_height,
                    config.lane_height,
                );
                let ghost = ClipGeom::place(
                    &Clip {
                        start: new_start,
                        ..c.clone()
                    },
                    &model.tracks[new_track],
                    row_y,
                    area.min.x,
                    config,
                    view,
                )
                .full;
                paint::draw_ghost(
                    painter,
                    ghost,
                    Color32::from_rgba_unmultiplied(100, 220, 255, 180),
                );
                if new_start != raw_start && snap_r >= 0 {
                    let head_x = view.frame_to_x(new_start as f32, area.min.x, config);
                    let tail_x =
                        view.frame_to_x((new_start + c.duration) as f32, area.min.x, config);
                    let by_head = crate::view::snap_frame(raw_start, &targets, snap_r) == new_start;
                    let x = if by_head { head_x } else { tail_x };
                    let stroke =
                        Stroke::new(1.0, Color32::from_rgba_unmultiplied(100, 220, 255, 220));
                    painter
                        .line_segment([Pos2::new(x, area.min.y), Pos2::new(x, area.max.y)], stroke);
                }
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);

                if released {
                    if new_start != initial_start || new_track != track {
                        actions.push(TimelineAction::MoveClip {
                            id: c.id,
                            new_start,
                            new_track,
                        });
                    }
                    *keep = false;
                }
            }
            DragState::TrimStart {
                track,
                clip,
                start_x,
            } => {
                let Some(c) = model.tracks.get(track).and_then(|t| t.clips.get(clip)) else {
                    *keep = false;
                    return;
                };
                // Snap the EDGE, then express it back as a delta - snapping the delta itself
                // would align the distance dragged, which is not a thing the user can see.
                let raw = view.px_to_frames(cur.x - start_x, config);
                let edge = crate::view::snap_frame(
                    c.start + raw,
                    &crate::view::snap_targets(model, Some((track, clip))),
                    Self::snap_dist(view, config),
                );
                let delta = edge - c.start;
                self.draw_trim_ghost(
                    painter,
                    view,
                    &model.tracks,
                    c,
                    track,
                    area,
                    c.trim_in + delta,
                    c.trim_out,
                );
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                if released {
                    if delta != 0 {
                        actions.push(TimelineAction::TrimStart { id: c.id, delta });
                    }
                    *keep = false;
                }
            }
            DragState::TrimEnd {
                track,
                clip,
                start_x,
            } => {
                let Some(c) = model.tracks.get(track).and_then(|t| t.clips.get(clip)) else {
                    *keep = false;
                    return;
                };
                // Dragging the tail edge left (negative dx) trims more off the tail.
                let raw = -view.px_to_frames(cur.x - start_x, config);
                // Same rule as the head: snap where the EDGE lands, then re-express as a delta.
                let tail = c.start + c.duration;
                let delta = tail
                    - crate::view::snap_frame(
                        tail - raw,
                        &crate::view::snap_targets(model, Some((track, clip))),
                        Self::snap_dist(view, config),
                    );
                self.draw_trim_ghost(
                    painter,
                    view,
                    &model.tracks,
                    c,
                    track,
                    area,
                    c.trim_in,
                    c.trim_out + delta,
                );
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                if released {
                    if delta != 0 {
                        actions.push(TimelineAction::TrimEnd { id: c.id, delta });
                    }
                    *keep = false;
                }
            }
            DragState::Slide {
                track,
                clip,
                start_x,
            } => {
                let Some(c) = model.tracks.get(track).and_then(|t| t.clips.get(clip)) else {
                    *keep = false;
                    return;
                };
                let delta = view.px_to_frames(cur.x - start_x, config);
                let row_y = track_top(
                    &model.tracks,
                    track,
                    area.min.y,
                    config.row_height,
                    config.lane_height,
                );
                let ghost = ClipGeom::place(
                    &Clip {
                        start: c.start + delta,
                        ..c.clone()
                    },
                    &model.tracks[track],
                    row_y,
                    area.min.x,
                    config,
                    view,
                )
                .full;
                paint::draw_ghost(
                    painter,
                    ghost,
                    Color32::from_rgba_unmultiplied(255, 180, 100, 200),
                );
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeColumn);
                if released {
                    if delta != 0 {
                        actions.push(TimelineAction::Slide { id: c.id, delta });
                    }
                    *keep = false;
                }
            }
            DragState::MoveKeys { anchor, start_x } => {
                // Block delta in frames from the grab X: whole frames while
                // `snap_keys_to_frames` (a key between frames keeps its fraction), else exact.
                let delta = crate::view::key_delta(
                    f64::from((cur.x - start_x) / view.ppf(config)),
                    config.snap_keys_to_frames,
                );
                let r = config.key_radius();
                // Live preview: every selected diamond drawn shifted by `delta` (no model
                // mutation; the host applies the move on release) - the ones on screen.
                let shown =
                    view.visible_frames(area, painter.clip_rect(), config)
                        .map(|(first, last)| {
                            let margin = r / view.ppf(config);
                            (f64::from(first - margin), f64::from(last + margin))
                        });
                for (t, track) in model.tracks.iter().enumerate() {
                    let Some((first, last)) = shown.filter(|_| track.expanded) else {
                        continue;
                    };
                    for (l, lane) in track.lanes.iter().enumerate() {
                        let cy = config.lane_center_y(track_tops[t], l);
                        for key in &lane.keys {
                            if !(first..=last).contains(&(key.frame + delta)) {
                                continue;
                            }
                            let is_anchor =
                                t == anchor.track && l == anchor.lane && key.frame == anchor.frame;
                            if !key.selected && !is_anchor {
                                continue;
                            }
                            let x = view.frame_to_x((key.frame + delta) as f32, area.min.x, config);
                            paint::draw_key_diamond(
                                painter,
                                Pos2::new(x, cy),
                                r,
                                Color32::from_rgba_unmultiplied(100, 220, 255, 220),
                                egui::Stroke::NONE,
                                false,
                            );
                        }
                    }
                }
                // Faint vertical guide at the anchor diamond's target frame.
                let gx = view.frame_to_x((anchor.frame + delta) as f32, area.min.x, config);
                painter.line_segment(
                    [Pos2::new(gx, area.min.y), Pos2::new(gx, area.max.y)],
                    egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(100, 220, 255, 120)),
                );
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                if released {
                    if delta != 0.0 {
                        let mut keys = Self::selected_keys(model);
                        // The anchor always moves, even if the select round-trip
                        // hasn't flagged it in the model yet (lone-key drag).
                        if !keys.contains(&anchor) {
                            keys.push(anchor);
                        }
                        actions.push(TimelineAction::MoveKeys { keys, delta });
                    }
                    *keep = false;
                }
            }
            DragState::Marquee { start, add } => {
                let rect = Rect::from_two_pos(start, cur);
                paint::draw_marquee(painter, rect);
                if released {
                    // Collect every diamond whose (x, band-center y) falls in the rect.
                    let mut keys = Vec::new();
                    for (t, track) in model.tracks.iter().enumerate() {
                        if !track.expanded {
                            continue;
                        }
                        for (l, lane) in track.lanes.iter().enumerate() {
                            let cy = config.lane_center_y(track_tops[t], l);
                            for key in &lane.keys {
                                let x = view.frame_to_x(key.frame as f32, area.min.x, config);
                                if rect.contains(Pos2::new(x, cy)) {
                                    keys.push(KeyPos {
                                        track: t,
                                        lane: l,
                                        frame: key.frame,
                                    });
                                }
                            }
                        }
                    }
                    if keys.is_empty() {
                        // Empty non-additive marquee clears; additive leaves as-is.
                        if !add {
                            actions.push(TimelineAction::ClearKeySelection);
                        }
                    } else {
                        actions.push(TimelineAction::SelectKeys { keys, add });
                    }
                    *keep = false;
                }
            }
        }
    }

    /// Ghost outline for a trim preview given prospective trim values.
    #[allow(clippy::too_many_arguments)]
    fn draw_trim_ghost(
        &self,
        painter: &egui::Painter,
        view: &TimelineView,
        tracks: &[Track],
        clip: &Clip,
        track: usize,
        area: Rect,
        trim_in: i64,
        trim_out: i64,
    ) {
        let config = &self.config;
        let row_y = track_top(
            tracks,
            track,
            area.min.y,
            config.row_height,
            config.lane_height,
        );
        let preview = Clip {
            trim_in: trim_in.max(0),
            trim_out: trim_out.max(0),
            ..clip.clone()
        };
        if let Some(visible) =
            ClipGeom::place(&preview, &tracks[track], row_y, area.min.x, config, view).visible
        {
            paint::draw_ghost(
                painter,
                visible,
                Color32::from_rgba_unmultiplied(100, 220, 255, 200),
            );
        }
    }
}

/// Read `(additive, range)` selection modifiers from live egui input:
/// additive = Ctrl/Cmd (toggle), range = Shift.
fn read_modifiers(ui: &Ui) -> (bool, bool) {
    let m = ui.ctx().input(|i| i.modifiers);
    (m.ctrl || m.command, m.shift)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Keyframe, PropLane, TimelineModel, Track};
    use egui::{Pos2, Rect, Vec2};

    /// One expanded track, one lane, one keyframe at frame 10. `track_tops[0]`
    /// is the area top (single track), matching the widget's prefix-sum layout.
    fn fixture() -> (TimelineModel, TimelineConfig, TimelineView, Rect, Vec<f32>) {
        let lane = PropLane::new("opacity", vec![Keyframe::new(10.0, 1.0)]);
        let track = Track::new("V1", vec![])
            .with_lanes(vec![lane])
            .expanded(true);
        let model = TimelineModel::new(24.0, vec![track]);
        let config = TimelineConfig::default();
        let view = TimelineView::default();
        let area = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(400.0, 200.0));
        let track_tops = vec![area.min.y];
        (model, config, view, area, track_tops)
    }

    /// (diamond x, lane-centre y) for the single keyframe — computed via the same
    /// `frame_to_x` the widget paints with, so the test never hardcodes the map.
    fn diamond(view: &TimelineView, area: Rect, config: &TimelineConfig) -> (f32, f32) {
        let x = view.frame_to_x(10.0, area.min.x, config);
        let cy = area.min.y + config.row_height + config.lane_height / 2.0;
        (x, cy)
    }

    #[test]
    fn pinned_ruler_stays_visible_and_hidden_bars_cannot_capture_scrubbing() {
        let ctx = egui::Context::default();
        let mut view = TimelineView::default();
        let mut model = TimelineModel::default();
        for id in 0..20 {
            model
                .tracks
                .push(Track::new("Layer", vec![Clip::new(id, 0, 100, "Clip")]));
        }
        let mut draw = |offset: f32, events| {
            let mut response = None;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(400.0))),
                    events,
                    ..Default::default()
                },
                |root| {
                    let mut canvas =
                        root.new_child(egui::UiBuilder::new().id_salt("pinned-test").max_rect(
                            Rect::from_min_size(
                                Pos2::new(0.0, 10.0 - offset),
                                Vec2::new(300.0, 800.0),
                            ),
                        ));
                    canvas.set_clip_rect(Rect::from_min_max(
                        Pos2::new(0.0, 10.0),
                        Pos2::new(300.0, 160.0),
                    ));
                    response = Some(TrackTimeline::default().show_pinned_ruler(
                        &mut canvas,
                        &mut view,
                        &model,
                        10.0,
                    ));
                },
            );
            output.textures_delta.clear();
            response.unwrap()
        };
        let first = draw(0.0, vec![]);
        let scrolled = draw(22.0, vec![]);
        assert_eq!(first.ruler_rect, scrolled.ruler_rect);
        assert_eq!(first.track_tops[0] - scrolled.track_tops[0], 22.0);
        assert_eq!(first.content_height, scrolled.content_height);
        let pos = Pos2::new(100.0, 20.0);
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        draw(22.0, vec![egui::Event::PointerMoved(pos), button(true)]);
        let clicked = draw(22.0, vec![button(false)]);
        assert!(
            clicked
                .actions
                .iter()
                .any(|a| matches!(a, TimelineAction::Seek { .. }))
        );
        assert!(!clicked.actions.iter().any(|a| matches!(
            a,
            TimelineAction::Select { .. } | TimelineAction::Slide { .. }
        )));
        assert!(clicked.hovered.is_none());
    }

    #[test]
    fn clipped_logical_canvas_does_not_seek_from_neighboring_panel() {
        let ctx = egui::Context::default();
        let mut view = TimelineView::default();
        let model = TimelineModel::default();
        let mut frame = |events: Vec<egui::Event>, overlay: bool| {
            let mut actions = Vec::new();
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(500.0))),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(100.0, 80.0));
                    let mut canvas = ui.new_child(
                        egui::UiBuilder::new()
                            .id_salt("clipped-timeline")
                            .max_rect(rect),
                    );
                    canvas.set_clip_rect(rect);
                    actions = TrackTimeline::default()
                        .show(&mut canvas, &mut view, &model)
                        .actions;
                });
                if overlay {
                    egui::Area::new(egui::Id::new("overlay"))
                        .order(egui::Order::Foreground)
                        .fixed_pos(Pos2::ZERO)
                        .show(root.ctx(), |ui| {
                            ui.allocate_exact_size(Vec2::new(100.0, 80.0), Sense::click());
                        });
                }
            });
            output.textures_delta.clear();
            actions
        };
        frame(vec![], false);
        frame(vec![], false);
        let click = |x, pressed| egui::Event::PointerButton {
            pos: Pos2::new(x, 30.0),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        };
        frame(
            vec![
                egui::Event::PointerMoved(Pos2::new(200.0, 30.0)),
                click(200.0, true),
            ],
            false,
        );
        let outside = frame(vec![click(200.0, false)], false);
        assert!(
            !outside
                .iter()
                .any(|a| matches!(a, TimelineAction::Seek { .. }))
        );
        frame(
            vec![
                egui::Event::PointerMoved(Pos2::new(50.0, 30.0)),
                click(50.0, true),
            ],
            false,
        );
        let inside = frame(vec![click(50.0, false)], false);
        assert!(
            inside
                .iter()
                .any(|a| matches!(a, TimelineAction::Seek { .. }))
        );
        frame(vec![], true);
        frame(vec![], true);
        frame(vec![click(50.0, true)], true);
        let covered = frame(vec![click(50.0, false)], true);
        assert!(
            !covered
                .iter()
                .any(|a| matches!(a, TimelineAction::Seek { .. }))
        );
    }

    #[test]
    fn hit_key_grabs_within_padded_zone() {
        let (model, config, view, area, tops) = fixture();
        let (x, cy) = diamond(&view, area, &config);
        // drawn radius (~4.48 at lane_height 16) + key_grab_pad 5 => ~9.48 grab.
        let hit =
            TrackTimeline::hit_key(&model, &tops, Pos2::new(x + 9.0, cy), area, &config, &view);
        assert_eq!(hit, Some((0, 0, 10.0)));
    }

    /// A key between frames is drawn and hit at its EXACT position: with keys at 10 and 10.5
    /// (zoomed so half a frame is 20 points), a click on 10.5's own x grabs 10.5 - not the
    /// whole frame beside it - and reports the key's own time.
    #[test]
    fn a_key_between_frames_is_hit_at_its_exact_position() {
        let (mut model, config, mut view, area, tops) = fixture();
        model.tracks[0].lanes[0].keys.push(Keyframe::new(10.5, 2.0));
        view.zoom = 20.0;
        let cy = area.min.y + config.row_height + config.lane_height / 2.0;
        let at = |frame: f32| view.frame_to_x(frame, area.min.x, &config);
        assert!(
            at(10.5) - at(10.0) >= 20.0,
            "precondition: half a frame is wide"
        );
        let hit =
            |x: f32| TrackTimeline::hit_key(&model, &tops, Pos2::new(x, cy), area, &config, &view);
        assert_eq!(hit(at(10.5)), Some((0, 0, 10.5)));
        assert_eq!(
            hit(at(10.5) - 3.0),
            Some((0, 0, 10.5)),
            "nearest diamond wins"
        );
        assert_eq!(hit(at(10.0)), Some((0, 0, 10.0)));
    }

    #[test]
    fn hit_key_misses_beyond_padded_zone() {
        let (model, config, view, area, tops) = fixture();
        let (x, cy) = diamond(&view, area, &config);
        let hit =
            TrackTimeline::hit_key(&model, &tops, Pos2::new(x + 11.0, cy), area, &config, &view);
        assert_eq!(hit, None);
    }

    /// The `key_grab_pad` token actually resizes the grab zone: a click 6px off
    /// the diamond hits with the default pad but misses when the pad is zeroed.
    #[test]
    fn key_grab_pad_token_resizes_zone() {
        let (model, mut config, view, area, tops) = fixture();
        let (x, cy) = diamond(&view, area, &config);
        assert!(
            TrackTimeline::hit_key(&model, &tops, Pos2::new(x + 6.0, cy), area, &config, &view)
                .is_some()
        );
        config.key_grab_pad = 0.0;
        assert!(
            TrackTimeline::hit_key(&model, &tops, Pos2::new(x + 6.0, cy), area, &config, &view)
                .is_none()
        );
    }
}
