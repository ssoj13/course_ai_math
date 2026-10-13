// Vendored and adapted from `transform-gizmo` 0.9.0.
// Copyright (c) Urho Laukkarinen. Licensed under MIT OR Apache-2.0. See NOTICE.
// Adaptations (glam-f64 internalisation, egui-only deps) are part of egui-gizmo
// and licensed BSD-3-Clause.
//
//! The internal 3D transformation gizmo (state machine + result types).
//!
//! All transforms and results here are **glam f64**. The public facade
//! ([`crate::Gizmo`]) wraps this and converts to/from glam f32 at the boundary.

use egui::Rgba;
use egui::epaint::Mesh;
use egui::{Pos2, Vec2};
use enumset::EnumSet;
use std::ops::{Add, AddAssign, Sub};

use crate::config::{
    GizmoConfig, GizmoDirection, GizmoMode, GizmoOrientation, PreparedGizmoConfig,
    TransformPivotPoint,
};
use crate::math::{Transform, screen_to_world, world_to_screen};
use glam::{DMat4, DQuat, DVec2, DVec3};

use crate::subgizmo::bbox::{BboxHandle, BboxParams};
use crate::subgizmo::bbox3d::{Bbox3DHandle, Bbox3DParams};
use crate::subgizmo::corner_pin::CornerPinParams;
use crate::subgizmo::rotation::RotationParams;
use crate::subgizmo::scale::ScaleParams;
use crate::subgizmo::shear::{ShearAxis2, ShearParams};
use crate::subgizmo::translation::TranslationParams;
use crate::subgizmo::{
    ArcballSubGizmo, Bbox3DSubGizmo, BboxSubGizmo, CornerPinSubGizmo, RotationSubGizmo,
    ScaleSubGizmo, ShearSubGizmo, SubGizmo, TranslationSubGizmo,
    common::{TransformKind, gizmo_normal},
};

/// A 3D transformation gizmo.
#[derive(Clone, Debug, Default)]
pub struct Gizmo {
    /// Prepared configuration of the gizmo.
    /// Includes the original [`GizmoConfig`] as well as
    /// various other values calculated from it, used for
    /// interaction and drawing the gizmo.
    config: PreparedGizmoConfig,
    /// Subgizmos used in the gizmo.
    subgizmos: Vec<SubGizmo>,
    active_subgizmo_id: Option<u64>,

    /// Active Maya middle-mouse translate gesture, if any (see [`MmbState`]).
    /// `None` means MMB is not currently driving the gizmo.
    mmb: Option<MmbState>,

    target_start_transforms: Vec<Transform>,

    gizmo_start_transform: Transform,

    /// Persistent 2D corner-pin state: the four WORLD-space corner positions
    /// (SW, SE, NE, NW). `None` means "not yet seeded" — on such a frame the
    /// prepared config is seeded from the target's box instead (see
    /// [`Gizmo::update_config`]); after the first corner-pin drag this becomes
    /// `Some(..)` and is re-fed every frame so the corners persist (the un-warped
    /// TRS transform would otherwise snap them back). Cleared whenever the handle
    /// set rebuilds (mode/target change) so leaving corner-pin mode resets the pin.
    corner_pin: Option<[DVec3; 4]>,

    /// Persistent 2D shear factors `(kx, ky)`. `None` means "no shear" — on such a
    /// frame the prepared config is fed `Vec2::ZERO` (un-sheared box). After the
    /// first shear-grip drag this becomes `Some(..)` and is re-fed every frame so
    /// the shear persists and the grips track the sheared box; a fresh grab then
    /// CONTINUES from the current value instead of snapping to zero. Cleared
    /// whenever the handle set rebuilds (mode/tool/target change), exactly like
    /// `corner_pin`. (`glam::Vec2`, fully qualified to avoid clashing with the
    /// `egui::Vec2` imported above.)
    shear: Option<glam::Vec2>,
}

/// State of an in-progress Maya middle-mouse-button (MMB) translate gesture.
///
/// MMB reuses the EXISTING translation subgizmos (no ad-hoc delta math): plain
/// MMB grabs the screen-space `View` handle (free move); `Shift`+MMB defers until
/// the 2D drag picks a world axis, then locks to that axis' arrow for the rest of
/// the gesture.
#[derive(Clone, Copy, Debug)]
enum MmbState {
    /// Plain MMB: the `View` (screen-plane) translate handle is force-active.
    Free,
    /// `Shift`+MMB before a direction is established — waiting for the drag to
    /// exceed the pixel threshold so a world axis can be chosen.
    AwaitingAxis {
        /// Screen position where MMB was pressed (drag origin for the threshold).
        press_pos: Pos2,
    },
    /// `Shift`+MMB after the axis lock — the chosen axis arrow stays force-active
    /// until MMB is released.
    AxisLocked,
}

impl Gizmo {
    /// Creates a new gizmo from given configuration
    pub fn new(config: GizmoConfig) -> Self {
        let mut gizmo = Self::default();
        gizmo.update_config(config);
        gizmo
    }

    /// Current configuration used by the gizmo.
    pub fn config(&self) -> &GizmoConfig {
        &self.config
    }

    /// The persisted 2D shear factors `(kx, ky)`; zero when not yet sheared.
    pub fn shear_2d(&self) -> glam::Vec2 {
        self.shear.unwrap_or(glam::Vec2::ZERO)
    }

    /// SEED the persistent 2D shear from the host's own stored value.
    ///
    /// The gizmo owns the shear because it is not TRS-representable and so cannot ride on the
    /// `Transform`s the host passes in. That makes the state one-way by default: the host reads it
    /// out with [`Self::shear_2d`], but on the next selection - or after any mode/tool change, which
    /// resets it - the grips would show an UN-sheared box over an already-sheared layer, and the
    /// first grab would snap the layer by the whole stored amount, because the handle reports an
    /// absolute factor measured from the box it thinks it is on.
    ///
    /// Call this before `update` with whatever the host has persisted (zero if none). Passing zero
    /// is equivalent to the un-seeded state, so hosts that do not store shear need not call it.
    pub fn set_shear_2d(&mut self, shear: glam::Vec2) {
        self.shear = Some(shear);
    }

    /// Updates the configuration used by the gizmo.
    pub fn update_config(&mut self, config: GizmoConfig) {
        if config.modes_changed(&self.config) {
            self.subgizmos.clear();
            self.active_subgizmo_id = None;
            // RESET the persistent corner-pin when the handle set rebuilds
            // (mode toggled / corner-pin disabled): otherwise stale corners would
            // re-seed the next time the handle is re-enabled. With `None` the pin
            // re-seeds from the current target box on its first frame.
            self.corner_pin = None;
            // RESET the persistent shear too (same lifecycle as corner-pin):
            // switching tool/mode must not leak a stale shear into the rebuilt
            // handle set. `None` => the box re-feeds zero shear next frame.
            self.shear = None;
        }

        self.config.update_for_config(config);

        if self.subgizmos.is_empty() {
            self.add_rotation();
            self.add_translation();
            self.add_scale();
            self.add_bbox();
            self.add_bbox3d();
            self.add_shear();
            self.add_corner_pin();
        }
    }

    /// Was this gizmo focused after the latest [`Gizmo::update`] call.
    pub fn is_focused(&self) -> bool {
        self.subgizmos.iter().any(|subgizmo| subgizmo.is_focused())
    }

    /// Ease each subgizmo's focus factor toward its current focused state. The
    /// caller supplies `factor(id, focused)` (the egui facade drives it through
    /// `Context::animate_bool_with_time`, keeping this crate egui-clock-free); the
    /// draw path lerps highlight vs inactive colour/alpha by the returned factor so
    /// the highlight fades instead of snapping. Call once per frame AFTER
    /// [`Self::update`] (which sets the focused bools) and BEFORE [`Self::draw`].
    pub(crate) fn set_focus_factors(&mut self, mut factor: impl FnMut(u64, bool) -> f32) {
        for subgizmo in &mut self.subgizmos {
            let f = factor(subgizmo.id(), subgizmo.is_focused());
            subgizmo.set_focus_factor(f);
        }
    }

    /// Updates the gizmo based on given interaction information.
    ///
    /// Returns the result of the interaction with the updated transformation.
    /// [`Some`] is returned when any of the subgizmos is being dragged, [`None`] otherwise.
    pub fn update(
        &mut self,
        interaction: GizmoInteraction,
        targets: &[Transform],
    ) -> Option<(GizmoResult, Vec<Transform>)> {
        if !self.config.viewport.is_finite() {
            return None;
        }

        // Update the gizmo based on the given target transforms,
        // unless the gizmo is currently being interacted with.
        if self.active_subgizmo_id.is_none() {
            self.config.update_for_targets(targets);
        }

        // Aspect-lock (Ctrl) is per-frame interaction state, not part of the
        // immutable config; stash it on the prepared config so it reaches each
        // subgizmo's `update` (the bbox handles read it). Must be set BEFORE the
        // config is copied into the subgizmos below.
        self.config.aspect_lock = interaction.ctrl;

        // Feed the 2D corner-pin corners into the prepared config so every corner
        // handle can read all four. When the gizmo already owns a corner set
        // (`Some`, after a drag), use THAT so the warp persists across frames;
        // otherwise SEED from the current target box (the model matrix is up to
        // date here — `update_for_targets` ran above when not dragging). Must be
        // set BEFORE the config is copied into the subgizmos below.
        self.config.corner_pin = self
            .corner_pin
            .unwrap_or_else(|| seed_corner_pin(&self.config));

        // Feed the owned 2D shear into the prepared config so the box frame
        // (`box_model`) and the bbox/shear grips read it. Zero when not yet
        // sheared (= un-sheared box, byte-identical to before). Must be set
        // BEFORE the config is copied into the subgizmos below.
        self.config.shear = self.shear.unwrap_or(glam::Vec2::ZERO);

        for subgizmo in &mut self.subgizmos {
            // Update current configuration to each subgizmo.
            subgizmo.update_config(self.config);
            // All subgizmos are initially considered unfocused.
            subgizmo.set_focused(false);
        }

        let force_active = self.config.mode_override.is_some();

        let pointer_ray = self.pointer_ray(Pos2::from(interaction.cursor_pos));

        // === Maya middle-mouse-button (MMB) translate ===========================
        // Drive the MMB gesture before the Primary pick. It reuses the existing
        // translation subgizmos and is mutually exclusive with the Primary (LMB)
        // drag (see `update_mmb`). A no-op when no translate handles exist.
        self.update_mmb(&interaction, pointer_ray, targets);

        // If there is no active subgizmo, find which one of them
        // is under the mouse pointer, if any. Skipped while MMB owns the gizmo so
        // the two buttons never fight over the active handle.
        if self.mmb.is_none()
            && self.active_subgizmo_id.is_none()
            && interaction.hovered
            && let Some(subgizmo) = self.pick_subgizmo(pointer_ray)
        {
            subgizmo.set_focused(true);

            // If we started dragging from one of the subgizmos, mark it as active.
            if interaction.drag_started || force_active {
                self.active_subgizmo_id = Some(subgizmo.id());
                self.target_start_transforms = targets.to_vec();
                self.gizmo_start_transform = self.config.as_transform();
            }
        }

        let mut result = None;

        // An MMB-owned handle is force-active for the whole gesture; its drag is
        // governed by `mmb_down` (tracked in `update_mmb`), not the Primary flags.
        let mmb_owns = self.mmb.is_some();

        if let Some(subgizmo) = self.active_subgizmo_mut() {
            if interaction.dragging || force_active || mmb_owns {
                subgizmo.set_active(true);
                subgizmo.set_focused(true);
                result = subgizmo.update(pointer_ray);
            } else {
                subgizmo.set_active(false);
                subgizmo.set_focused(false);
                self.active_subgizmo_id = None;
            }
        }

        let Some(result) = result else {
            // No interaction, no result.

            self.config.update_for_targets(targets);

            for subgizmo in &mut self.subgizmos {
                subgizmo.update_config(self.config);
            }

            return None;
        };

        // PERSIST the corner-pin across frames. The result carries the new corner
        // set in the gizmo's z=0 WORLD plane (X/Y only); store it back so the next
        // frame re-feeds it via `self.config.corner_pin` above instead of
        // re-seeding from the (un-warped) target box, which would snap the corners
        // back. Z is 0 (the corners live in the flat plane).
        if let GizmoResult::CornerPin { corners } = result {
            self.corner_pin = Some(corners.map(|c| DVec3::new(c.x, c.y, 0.0)));
        }

        // PERSIST the 2D shear across frames (same reasoning as corner-pin above):
        // the shear handle reports the ABSOLUTE factor (base + drift), so storing
        // it back lets the next frame re-feed it via `self.config.shear` and a
        // re-grab continue from the current value instead of snapping to zero.
        if let GizmoResult::Shear { axis, amount } = result {
            let mut s = self.shear.unwrap_or(glam::Vec2::ZERO);
            match axis {
                ShearAxis2::X => s.x = amount as f32,
                ShearAxis2::Y => s.y = amount as f32,
            }
            self.shear = Some(s);
        }

        self.update_config_with_result(result);

        let updated_targets =
            self.update_transforms_with_result(result, targets, &self.target_start_transforms);

        Some((result, updated_targets))
    }

    /// Return all the necessary data to draw the latest gizmo interaction.
    ///
    /// The gizmo draw data consists of vertices in viewport coordinates.
    pub fn draw(&self) -> GizmoDrawData {
        if !self.config.viewport.is_finite() {
            return GizmoDrawData::default();
        }

        let mut draw_data = GizmoDrawData::default();
        // Draw ALL subgizmos every frame, including mid-drag. The upstream hid non-active handles
        // during a drag to declutter, but the Box3D wireframe is emitted by a single handle (the
        // +++ corner); hiding the others made the whole bounding box vanish while dragging any other
        // grip. Keeping all visible is the wanted behavior (box stays on screen during the drag).
        for subgizmo in &self.subgizmos {
            draw_data += subgizmo.draw();
        }

        draw_data
    }

    /// Checks all sub-gizmos for intersections with the cursor. If there is one, return true.
    pub fn pick_preview(&self, cursor_pos: (f32, f32)) -> bool {
        let pointer_ray = self.pointer_ray(Pos2::from(cursor_pos));
        self.subgizmos.iter().any(|x| x.pick_preview(pointer_ray))
    }

    fn active_subgizmo_mut(&mut self) -> Option<&mut SubGizmo> {
        self.active_subgizmo_id.and_then(|id| {
            self.subgizmos
                .iter_mut()
                .find(|subgizmo| subgizmo.id() == id)
        })
    }

    /// Drive the Maya middle-mouse translate gesture (called from [`Gizmo::update`]
    /// before the Primary pick). Mutates `self.mmb` and `self.active_subgizmo_id`.
    ///
    /// Mutual exclusion with the Primary (LMB) drag: a gesture only BEGINS when no
    /// subgizmo is already active (so we never hijack a Primary drag), and the
    /// Primary pick at the call site is skipped while `self.mmb` is `Some`.
    fn update_mmb(&mut self, interaction: &GizmoInteraction, ray: Ray, targets: &[Transform]) {
        // End the gesture on release, deactivating any force-activated handle.
        if self.mmb.is_some() && !interaction.mmb_down {
            self.active_subgizmo_id = None;
            self.mmb = None;
            return;
        }

        // Begin a gesture on MMB press, provided nothing else owns the gizmo.
        if interaction.mmb_pressed && self.mmb.is_none() && self.active_subgizmo_id.is_none() {
            if interaction.shift {
                // Defer: the axis is chosen once the drag establishes a direction,
                // but only when at least one per-axis translate arrow exists.
                if self.has_axis_translate() {
                    self.mmb = Some(MmbState::AwaitingAxis {
                        press_pos: Pos2::from(interaction.cursor_pos),
                    });
                }
            } else if self
                .force_activate(GizmoDirection::View, ray, targets)
                .is_some()
            {
                // Free screen-space move: grab the `View` handle immediately. If the
                // `View` (screen) handle is disabled, plain MMB simply does nothing.
                self.mmb = Some(MmbState::Free);
            }
        }

        // Resolve the deferred axis once the drag exceeds the pixel threshold.
        if let Some(MmbState::AwaitingAxis { press_pos }) = self.mmb
            && interaction.mmb_down
        {
            // Distance (in points) the 2D drag must travel before it commits to a
            // world axis — small enough to feel immediate, large enough to be a
            // deliberate direction.
            const MMB_AXIS_THRESHOLD: f32 = 4.0;
            let drag = Pos2::from(interaction.cursor_pos) - press_pos;
            if drag.length() >= MMB_AXIS_THRESHOLD
                && let Some(dir) = self.pick_mmb_axis(drag)
                && self.force_activate(dir, ray, targets).is_some()
            {
                self.mmb = Some(MmbState::AxisLocked);
            }
        }
    }

    /// Force-activate the translate subgizmo driving `direction`, seeding its
    /// drag-start state from `ray` so the first frame produces a zero delta (no
    /// jump). Returns its id, or `None` when that handle is absent (e.g. translate
    /// disabled, or the `View`/axis handle not enabled).
    fn force_activate(
        &mut self,
        direction: GizmoDirection,
        ray: Ray,
        targets: &[Transform],
    ) -> Option<u64> {
        let id = self.find_translation_id(direction)?;
        if let Some(subgizmo) = self.subgizmos.iter_mut().find(|s| s.id() == id) {
            // Seed `start_point`/`last_point` with the SAME projection `update`
            // uses (see `translation::seed_drag_start`) — the normal pick path only
            // works jump-free because the cursor is ON the handle, which MMB is not.
            subgizmo.seed_drag_start(ray);
        }
        self.active_subgizmo_id = Some(id);
        self.target_start_transforms = targets.to_vec();
        self.gizmo_start_transform = self.config.as_transform();
        Some(id)
    }

    /// Id of the translation subgizmo driving `direction` (`View` = screen handle,
    /// `X`/`Y`/`Z` = the per-axis arrows), if present.
    fn find_translation_id(&self, direction: GizmoDirection) -> Option<u64> {
        self.subgizmos
            .iter()
            .find(|s| s.mmb_translate_dir() == Some(direction))
            .map(|s| s.id())
    }

    /// Whether any per-axis translate arrow exists (gate for the Shift+MMB axis
    /// lock).
    fn has_axis_translate(&self) -> bool {
        [GizmoDirection::X, GizmoDirection::Y, GizmoDirection::Z]
            .into_iter()
            .any(|d| self.find_translation_id(d).is_some())
    }

    /// Pick the enabled world translate axis whose screen-projected direction is
    /// most parallel to the 2D `drag` vector (max `|dot|` of the normalised screen
    /// directions) — the Maya `Shift`+MMB axis choice. Reuses `world_to_screen` /
    /// `gizmo_normal` (no new math path).
    fn pick_mmb_axis(&self, drag: Vec2) -> Option<GizmoDirection> {
        if drag.length() < 1e-3 {
            return None;
        }
        let drag = drag.normalized();

        let viewport = self.config.viewport;
        let vp = self.config.view_projection;
        let origin_w = self.config.translation;
        // Project along the actual on-screen arrow length so the screen direction
        // matches what the user sees; magnitude is irrelevant after normalising.
        let len = (self.config.scale_factor * self.config.visuals.gizmo_size) as f64;

        let mut best: Option<(GizmoDirection, f32)> = None;
        for dir in [GizmoDirection::X, GizmoDirection::Y, GizmoDirection::Z] {
            if self.find_translation_id(dir).is_none() {
                continue; // this axis arrow is disabled — not a candidate
            }
            // World-space axis direction (honours local/global orientation).
            let axis_w = gizmo_normal(&self.config, dir);
            let tip_w = origin_w + axis_w * len;
            let (Some(p0), Some(p1)) = (
                world_to_screen(viewport, vp, origin_w),
                world_to_screen(viewport, vp, tip_w),
            ) else {
                continue; // behind the camera — skip
            };
            let sdir = p1 - p0;
            if sdir.length() < 1e-3 {
                continue; // axis points straight at/away from the camera
            }
            let sdir = sdir.normalized();
            // 2D parallelism (sign-agnostic: dragging either way along an axis
            // selects it).
            let parallelism = (sdir.x * drag.x + sdir.y * drag.y).abs();
            if best.is_none_or(|(_, b)| parallelism > b) {
                best = Some((dir, parallelism));
            }
        }
        best.map(|(dir, _)| dir)
    }

    fn update_transforms_with_result(
        &self,
        result: GizmoResult,
        transforms: &[Transform],
        start_transforms: &[Transform],
    ) -> Vec<Transform> {
        transforms
            .iter()
            .zip(start_transforms)
            .map(|(transform, start_transform)| match result {
                GizmoResult::Rotation {
                    axis,
                    delta,
                    total: _,
                    is_view_axis,
                } => self.update_rotation(transform, axis, delta, is_view_axis),
                GizmoResult::Translation { delta, total: _ } => {
                    self.update_translation(delta, transform, start_transform)
                }
                GizmoResult::Scale { total } => {
                    self.update_scale(transform, start_transform, total)
                }
                GizmoResult::Arcball { delta, total: _ } => {
                    self.update_rotation_quat(transform, delta)
                }
                GizmoResult::BoxScale { pivot, scale } => {
                    // Scale about the world pivot, applied to the START transform:
                    //   T(pivot) · S(scale) · T(-pivot) · SRT(start)
                    // then decomposed back to TRS. World-axis scale of a rotated
                    // object is a shear (not TRS-representable) — accepted here as
                    // with the existing Global scale path; 2D bbox runs identity-
                    // rotation so it is exact.
                    let m = DMat4::from_translation(pivot)
                        * DMat4::from_scale(scale)
                        * DMat4::from_translation(-pivot)
                        * DMat4::from_scale_rotation_translation(
                            start_transform.scale,
                            start_transform.rotation,
                            start_transform.translation,
                        );
                    let (s, r, t) = m.to_scale_rotation_translation();
                    Transform {
                        scale: s,
                        rotation: r,
                        translation: t,
                    }
                }
                // Shear is NOT TRS-representable: leave the target transform alone.
                // The shear factor travels on the public `GizmoResult::kind`, where
                // the caller reads it and applies it to its own geometry.
                GizmoResult::Shear { .. } => *transform,
                // Corner-pin is a homography (NOT TRS): leave the target transform
                // alone. The four corner positions travel on the public
                // `GizmoResult::kind`, where the caller applies the homography.
                GizmoResult::CornerPin { .. } => *transform,
            })
            .collect()
    }

    fn update_rotation(
        &self,
        transform: &Transform,
        axis: DVec3,
        delta: f64,
        is_view_axis: bool,
    ) -> Transform {
        let axis = match self.config.orientation() {
            GizmoOrientation::Local if !is_view_axis => (transform.rotation * axis).normalize(),
            _ => axis,
        };

        let delta = DQuat::from_axis_angle(axis, delta);

        self.update_rotation_quat(transform, delta)
    }

    fn update_rotation_quat(&self, transform: &Transform, delta: DQuat) -> Transform {
        let translation = match self.config.pivot_point {
            TransformPivotPoint::MedianPoint => {
                self.config.translation + delta * (transform.translation - self.config.translation)
            }
            TransformPivotPoint::IndividualOrigins => transform.translation,
        };

        let new_rotation = (delta * transform.rotation).normalize();

        Transform {
            scale: transform.scale,
            rotation: new_rotation,
            translation,
        }
    }

    fn update_translation(
        &self,
        delta: DVec3,
        transform: &Transform,
        start_transform: &Transform,
    ) -> Transform {
        let delta = match self.config.orientation() {
            GizmoOrientation::Global => delta,
            GizmoOrientation::Local => start_transform.rotation * delta,
        };

        Transform {
            scale: start_transform.scale,
            rotation: start_transform.rotation,
            translation: delta + transform.translation,
        }
    }

    fn update_scale(
        &self,
        transform: &Transform,
        start_transform: &Transform,
        scale: DVec3,
    ) -> Transform {
        let new_scale = match self.config.orientation() {
            GizmoOrientation::Global => {
                let scaled_transform_mat = DMat4::from_scale(scale)
                    * DMat4::from_scale_rotation_translation(
                        start_transform.scale,
                        start_transform.rotation,
                        start_transform.translation,
                    );
                let (scale, _, _) = scaled_transform_mat.to_scale_rotation_translation();
                scale
            }
            GizmoOrientation::Local => start_transform.scale * scale,
        };

        Transform {
            scale: new_scale,
            ..*transform
        }
    }

    fn update_config_with_result(&mut self, result: GizmoResult) {
        // Apply the result to the gizmo's own (median) transform. Use
        // `.into_iter().next()` instead of `[0]` indexing — the vector is always
        // length 1 here, but indexing would panic on the empty case.
        if let Some(new_config_transform) = self
            .update_transforms_with_result(
                result,
                &[self.config.as_transform()],
                &[self.gizmo_start_transform],
            )
            .into_iter()
            .next()
        {
            self.config.update_transform(new_config_transform);
        }
    }

    /// Picks the subgizmo that is closest to the given world space ray.
    #[allow(clippy::manual_inspect)]
    fn pick_subgizmo(&mut self, ray: Ray) -> Option<&mut SubGizmo> {
        // If mode is overridden, assume we only have that mode, and choose it.
        if self.config.mode_override.is_some() {
            return self.subgizmos.first_mut().map(|subgizmo| {
                subgizmo.pick(ray);

                subgizmo
            });
        }

        self.subgizmos
            .iter_mut()
            .filter_map(|subgizmo| subgizmo.pick(ray).map(|t| (t, subgizmo)))
            .min_by(|(first, _), (second, _)| {
                first
                    .partial_cmp(second)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(_, subgizmo)| subgizmo)
    }

    /// Get all modes that are currently enabled
    fn enabled_modes(&self) -> EnumSet<GizmoMode> {
        self.config
            .mode_override
            .map_or(self.config.modes, EnumSet::only)
    }

    /// Adds rotation subgizmos
    fn add_rotation(&mut self) {
        let modes = self.enabled_modes();

        if modes.contains(GizmoMode::RotateX) {
            self.subgizmos.push(
                RotationSubGizmo::new(
                    self.config,
                    RotationParams {
                        direction: GizmoDirection::X,
                    },
                )
                .into(),
            );
        }

        if modes.contains(GizmoMode::RotateY) {
            self.subgizmos.push(
                RotationSubGizmo::new(
                    self.config,
                    RotationParams {
                        direction: GizmoDirection::Y,
                    },
                )
                .into(),
            );
        }

        if modes.contains(GizmoMode::RotateZ) {
            self.subgizmos.push(
                RotationSubGizmo::new(
                    self.config,
                    RotationParams {
                        direction: GizmoDirection::Z,
                    },
                )
                .into(),
            );
        }

        if modes.contains(GizmoMode::RotateView) {
            self.subgizmos.push(
                RotationSubGizmo::new(
                    self.config,
                    RotationParams {
                        direction: GizmoDirection::View,
                    },
                )
                .into(),
            );
        }

        if modes.contains(GizmoMode::Arcball) {
            self.subgizmos
                .push(ArcballSubGizmo::new(self.config, ()).into());
        }
    }

    /// Adds translation subgizmos
    fn add_translation(&mut self) {
        let modes = self.enabled_modes();

        if modes.contains(GizmoMode::TranslateX) {
            self.subgizmos.push(
                TranslationSubGizmo::new(
                    self.config,
                    TranslationParams {
                        mode: GizmoMode::TranslateX,
                        direction: GizmoDirection::X,
                        transform_kind: TransformKind::Axis,
                    },
                )
                .into(),
            );
        }

        if modes.contains(GizmoMode::TranslateY) {
            self.subgizmos.push(
                TranslationSubGizmo::new(
                    self.config,
                    TranslationParams {
                        mode: GizmoMode::TranslateY,
                        direction: GizmoDirection::Y,
                        transform_kind: TransformKind::Axis,
                    },
                )
                .into(),
            );
        }

        if modes.contains(GizmoMode::TranslateZ) {
            self.subgizmos.push(
                TranslationSubGizmo::new(
                    self.config,
                    TranslationParams {
                        mode: GizmoMode::TranslateZ,
                        direction: GizmoDirection::Z,
                        transform_kind: TransformKind::Axis,
                    },
                )
                .into(),
            );
        }

        if modes.contains(GizmoMode::TranslateView) {
            self.subgizmos.push(
                TranslationSubGizmo::new(
                    self.config,
                    TranslationParams {
                        mode: GizmoMode::TranslateView,
                        direction: GizmoDirection::View,
                        transform_kind: TransformKind::Plane,
                    },
                )
                .into(),
            );
        }

        if modes.contains(GizmoMode::TranslateXY) {
            self.subgizmos.push(
                TranslationSubGizmo::new(
                    self.config,
                    TranslationParams {
                        mode: GizmoMode::TranslateXY,
                        direction: GizmoDirection::X,
                        transform_kind: TransformKind::Plane,
                    },
                )
                .into(),
            );
        }

        if modes.contains(GizmoMode::TranslateXZ) {
            self.subgizmos.push(
                TranslationSubGizmo::new(
                    self.config,
                    TranslationParams {
                        mode: GizmoMode::TranslateXZ,
                        direction: GizmoDirection::Y,
                        transform_kind: TransformKind::Plane,
                    },
                )
                .into(),
            );
        }

        if modes.contains(GizmoMode::TranslateYZ) {
            self.subgizmos.push(
                TranslationSubGizmo::new(
                    self.config,
                    TranslationParams {
                        mode: GizmoMode::TranslateYZ,
                        direction: GizmoDirection::Z,
                        transform_kind: TransformKind::Plane,
                    },
                )
                .into(),
            );
        }
    }

    /// Adds scale subgizmos
    fn add_scale(&mut self) {
        let modes = self.enabled_modes();

        if modes.contains(GizmoMode::ScaleX) {
            self.subgizmos.push(
                ScaleSubGizmo::new(
                    self.config,
                    ScaleParams {
                        mode: GizmoMode::ScaleX,
                        direction: GizmoDirection::X,
                        transform_kind: TransformKind::Axis,
                    },
                )
                .into(),
            );
        }

        if modes.contains(GizmoMode::ScaleY) {
            self.subgizmos.push(
                ScaleSubGizmo::new(
                    self.config,
                    ScaleParams {
                        mode: GizmoMode::ScaleY,
                        direction: GizmoDirection::Y,
                        transform_kind: TransformKind::Axis,
                    },
                )
                .into(),
            );
        }

        if modes.contains(GizmoMode::ScaleZ) {
            self.subgizmos.push(
                ScaleSubGizmo::new(
                    self.config,
                    ScaleParams {
                        mode: GizmoMode::ScaleZ,
                        direction: GizmoDirection::Z,
                        transform_kind: TransformKind::Axis,
                    },
                )
                .into(),
            );
        }

        if modes.contains(GizmoMode::ScaleUniform) && !modes.contains(GizmoMode::RotateView) {
            self.subgizmos.push(
                ScaleSubGizmo::new(
                    self.config,
                    ScaleParams {
                        mode: GizmoMode::ScaleUniform,
                        direction: GizmoDirection::View,
                        transform_kind: TransformKind::Plane,
                    },
                )
                .into(),
            );
        }

        if modes.contains(GizmoMode::ScaleXY) && !modes.contains(GizmoMode::TranslateXY) {
            self.subgizmos.push(
                ScaleSubGizmo::new(
                    self.config,
                    ScaleParams {
                        mode: GizmoMode::ScaleXY,
                        direction: GizmoDirection::X,
                        transform_kind: TransformKind::Plane,
                    },
                )
                .into(),
            );
        }

        if modes.contains(GizmoMode::ScaleXZ) && !modes.contains(GizmoMode::TranslateXZ) {
            self.subgizmos.push(
                ScaleSubGizmo::new(
                    self.config,
                    ScaleParams {
                        mode: GizmoMode::ScaleXZ,
                        direction: GizmoDirection::Y,
                        transform_kind: TransformKind::Plane,
                    },
                )
                .into(),
            );
        }

        if modes.contains(GizmoMode::ScaleYZ) && !modes.contains(GizmoMode::TranslateYZ) {
            self.subgizmos.push(
                ScaleSubGizmo::new(
                    self.config,
                    ScaleParams {
                        mode: GizmoMode::ScaleYZ,
                        direction: GizmoDirection::Z,
                        transform_kind: TransformKind::Plane,
                    },
                )
                .into(),
            );
        }
    }

    /// Adds the 2D bounding-box subgizmos (8 grips: 4 corners + 4 edge mids).
    /// Gated on the `BoundingBox` mode (set 2D-only by the facade); each handle's
    /// distinct `BboxParams` hashes to a distinct id (see `SubGizmoConfig::new`).
    fn add_bbox(&mut self) {
        let modes = self.enabled_modes();

        if modes.contains(GizmoMode::BoundingBox) {
            for handle in BboxHandle::ALL {
                self.subgizmos
                    .push(BboxSubGizmo::new(self.config, BboxParams { handle }).into());
            }
        }
    }

    /// Adds the 3D bounding-box subgizmos (26 grips: 6 faces + 12 edges + 8
    /// corners). Gated on the `BoundingBox3D` mode; each handle's distinct
    /// `Bbox3DParams` hashes to a distinct id (see `SubGizmoConfig::new`).
    fn add_bbox3d(&mut self) {
        let modes = self.enabled_modes();

        if modes.contains(GizmoMode::BoundingBox3D) {
            for handle in Bbox3DHandle::all() {
                self.subgizmos
                    .push(Bbox3DSubGizmo::new(self.config, Bbox3DParams { handle }).into());
            }
        }
    }

    /// Adds the 2D shear subgizmos (2 grips: top-edge mid = X shear, right-edge
    /// mid = Y shear). Gated on the `Shear` mode (set 2D-only by the facade); each
    /// grip's distinct `ShearParams` hashes to a distinct id (see `SubGizmoConfig`).
    fn add_shear(&mut self) {
        let modes = self.enabled_modes();

        if modes.contains(GizmoMode::Shear) {
            for axis in ShearAxis2::ALL {
                self.subgizmos
                    .push(ShearSubGizmo::new(self.config, ShearParams { axis }).into());
            }
        }
    }

    /// Adds the 2D corner-pin subgizmos (4 independently-draggable corners, fixed
    /// order SW, SE, NE, NW). Gated on the `CornerPin` mode (set 2D-only by the
    /// facade); each corner's distinct `CornerPinParams` hashes to a distinct id
    /// (see `SubGizmoConfig::new`).
    fn add_corner_pin(&mut self) {
        let modes = self.enabled_modes();

        if modes.contains(GizmoMode::CornerPin) {
            for corner in 0..4 {
                self.subgizmos
                    .push(CornerPinSubGizmo::new(self.config, CornerPinParams { corner }).into());
            }
        }
    }

    /// Calculate a world space ray from given screen space position
    fn pointer_ray(&self, screen_pos: Pos2) -> Ray {
        let mat = self.config.view_projection.inverse();
        let origin = screen_to_world(self.config.viewport, mat, screen_pos, -1.0);
        let target = screen_to_world(self.config.viewport, mat, screen_pos, 1.0);

        let direction = target.sub(origin).normalize();

        Ray {
            screen_pos,
            origin,
            direction,
        }
    }
}

/// Seed the four corner-pin corners from the target box: the local corners
/// `(±hx, ±hy, 0)` (half-extent from `bbox_half_extent`) transformed to WORLD by
/// the model matrix, in the fixed order SW, SE, NE, NW. Used by
/// [`Gizmo::update`] on frames where the persistent pin is not yet set (`None`).
fn seed_corner_pin(config: &PreparedGizmoConfig) -> [DVec3; 4] {
    let hx = config.bbox_half_extent.x as f64;
    let hy = config.bbox_half_extent.y as f64;
    let m = config.model_matrix;
    [
        m.transform_point3(DVec3::new(-hx, -hy, 0.0)), // SW
        m.transform_point3(DVec3::new(hx, -hy, 0.0)),  // SE
        m.transform_point3(DVec3::new(hx, hy, 0.0)),   // NE
        m.transform_point3(DVec3::new(-hx, hy, 0.0)),  // NW
    ]
}

/// Information needed for interacting with the gizmo.
#[derive(Default, Clone, Copy, Debug)]
pub struct GizmoInteraction {
    /// Current cursor position in window coordinates.
    pub cursor_pos: (f32, f32),
    /// Whether the gizmo is hovered this frame.
    /// Some other UI element might be covering the gizmo,
    /// and in such case you may not want the gizmo to be
    /// interactable.
    pub hovered: bool,
    /// Whether dragging was started this frame.
    /// Usually this is set to true if the primary mouse
    /// button was just pressed.
    pub drag_started: bool,
    /// Whether the user is currently dragging.
    /// Usually this is set to true whenever the primary mouse
    /// button is being pressed.
    pub dragging: bool,
    /// Whether the MIDDLE mouse button was pressed this frame (Maya MMB — begins a
    /// screen-space translate, or a `Shift`-axis translate). Defaults to `false`,
    /// so existing callers that only set the Primary fields are unaffected.
    pub mmb_pressed: bool,
    /// Whether the MIDDLE mouse button is currently held (continues the MMB
    /// translate). Defaults to `false`.
    pub mmb_down: bool,
    /// Whether `Shift` is held — selects the axis-constrained MMB translate over
    /// the free screen-space one. Defaults to `false`.
    pub shift: bool,
    /// Whether `Ctrl` is held — the aspect-lock modifier for the 2D bounding-box
    /// scale handles (forces a uniform factor). Additive; defaults to `false`.
    pub ctrl: bool,
}

/// Result of a gizmo transformation. All quantities are **glam f64**.
#[derive(Debug, Copy, Clone)]
pub enum GizmoResult {
    Rotation {
        /// The rotation axis
        axis: DVec3,
        /// The latest rotation angle delta
        delta: f64,
        /// Total rotation angle of the gizmo interaction
        total: f64,
        /// Whether we are rotating along the view axis
        is_view_axis: bool,
    },
    Translation {
        /// The latest translation delta
        delta: DVec3,
        /// Total translation of the gizmo interaction
        total: DVec3,
    },
    Scale {
        /// Total scale of the gizmo interaction
        total: DVec3,
    },
    Arcball {
        /// The latest rotation delta
        delta: DQuat,
        /// Total rotation of the gizmo interaction
        total: DQuat,
    },
    /// 2D bounding-box scale: scale `scale` about the world-space `pivot`
    /// (the opposite anchor of the grabbed grip). Emitted by the `bbox` handles.
    BoxScale {
        /// World-space pivot the scale is applied about.
        pivot: DVec3,
        /// Scale factors (local X/Y; Z left at 1.0).
        scale: DVec3,
    },
    /// 2D shear: an affine that is NOT TRS-representable, so it does NOT modify the
    /// target transforms (the `update_transforms_with_result` arm returns them
    /// unchanged). The `amount` (shear factor, absolute from drag start) rides on
    /// the public [`crate::GizmoResult::kind`] instead. Emitted by the `shear`
    /// handles (see `subgizmo/shear.rs`).
    Shear {
        /// Which local axis the shear acts along.
        axis: ShearAxis2,
        /// Shear factor, absolute relative to the drag-start frame.
        amount: f64,
    },
    /// 2D corner-pin: a projective homography that is NOT TRS-representable, so it
    /// does NOT modify the target transforms (the `update_transforms_with_result`
    /// arm returns them unchanged). The four corner positions (in the gizmo's z=0
    /// WORLD plane, X/Y only) ride on the public [`crate::GizmoResult::kind`]
    /// instead. Emitted by the `corner_pin` handles (see `subgizmo/corner_pin.rs`).
    CornerPin {
        /// The four corner positions (SW, SE, NE, NW) in the z=0 world plane.
        corners: [DVec2; 4],
    },
}

/// A single text label to draw on top of the gizmo mesh. This is the gizmo's
/// text channel: meshes carry no glyphs, so the drag-time HUD readouts (angle /
/// distance / factor) and dial tick numbers ride here instead. Positions are
/// ALREADY projected to viewport/screen space (same convention as `vertices`),
/// so the painter can emit them verbatim without re-projecting.
#[derive(Clone, Debug)]
pub struct GizmoLabel {
    /// Anchor position in VIEWPORT/screen space (already projected, like the
    /// mesh `vertices`).
    pub pos: [f32; 2],
    /// The text to draw.
    pub text: String,
    /// Linear RGBA (premultiplied-on-paint, same convention as `colors`).
    pub color: [f32; 4],
    /// Font size in points.
    pub size: f32,
    /// How `pos` anchors the text box (e.g. centre, top-left…).
    pub anchor: egui::Align2,
}

/// Data used to draw [`Gizmo`].
#[derive(Default, Clone, Debug)]
pub struct GizmoDrawData {
    /// Vertices in viewport space.
    pub vertices: Vec<[f32; 2]>,
    /// Linear RGBA colors.
    pub colors: Vec<[f32; 4]>,
    /// Indices to the vertex data.
    pub indices: Vec<u32>,
    /// Text labels (gizmo text channel — drag HUD readouts + dial tick numbers).
    /// Decoupled from the index buffer: labels carry pre-projected screen
    /// positions, so concatenation needs no index offset (see `AddAssign`).
    pub labels: Vec<GizmoLabel>,
}

impl From<Mesh> for GizmoDrawData {
    fn from(mesh: Mesh) -> Self {
        let (vertices, colors): (Vec<_>, Vec<_>) = mesh
            .vertices
            .iter()
            .map(|vertex| {
                (
                    [vertex.pos.x, vertex.pos.y],
                    Rgba::from(vertex.color).to_array(),
                )
            })
            .unzip();

        Self {
            vertices,
            colors,
            indices: mesh.indices,
            // A mesh has no text; the label channel starts empty.
            labels: Vec::new(),
        }
    }
}

impl AddAssign for GizmoDrawData {
    fn add_assign(&mut self, rhs: Self) {
        let index_offset = self.vertices.len() as u32;
        self.vertices.extend(rhs.vertices);
        self.colors.extend(rhs.colors);
        self.indices
            .extend(rhs.indices.into_iter().map(|idx| index_offset + idx));
        // Labels carry pre-projected screen positions (no vertex indices), so
        // they just concatenate — no index-offset fix-up needed.
        self.labels.extend(rhs.labels);
    }
}

impl Add for GizmoDrawData {
    type Output = Self;

    fn add(mut self, rhs: Self) -> Self::Output {
        self += rhs;
        self
    }
}

#[derive(Debug, Copy, Clone)]
pub(crate) struct Ray {
    pub(crate) screen_pos: Pos2,
    pub(crate) origin: DVec3,
    pub(crate) direction: DVec3,
}
