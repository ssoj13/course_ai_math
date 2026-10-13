use cam_controls::{
    CameraController, CameraIntent, CameraKind, CameraPose, OrbitRig, OrientationMode,
    PerspectiveSettings, yaw_pitch_from_orientation,
};
use cam_viewport::ViewportSize;
use glam::{Mat4, Quat, Vec2, Vec3};

use super::graph_plane::GraphPlane;

/// How Alt+LMB orbit applies rotation around the graph target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OrbitMode {
    /// Trackball-like yaw/pitch without a turntable pole singularity.
    #[default]
    Quaternion,
    /// Houdini-style turntable policy (yaw around world up).
    HoudiniTurntable,
}

impl OrbitMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::HoudiniTurntable => "Houdini",
            Self::Quaternion => "Trackball",
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            Self::HoudiniTurntable => Self::Quaternion,
            Self::Quaternion => Self::HoudiniTurntable,
        }
    }
}

/// Genuine 3D perspective camera for the node graph viewport.
///
/// Pose is owned by [`CameraController`], while [`GraphPlane`] remains the single
/// source of truth for graph UV <-> world mapping (`+u` right, `+v` down on screen).
#[derive(Debug, Clone)]
pub struct Camera {
    /// Working plane for node graph coordinates `(u, v)`.
    pub plane: GraphPlane,
    controller: CameraController,
    settings: PerspectiveSettings,
}

/// Default front distance from the graph board.
const DEFAULT_FRONT_DISTANCE: f32 = 1000.0;

/// Default vertical FOV ≈ 2° (radians).
pub const DEFAULT_FOV_Y: f32 = 0.034_906_585; // 2.0°
/// Minimum FOV ≈ 0.3° (radians).
pub const FOV_Y_MIN: f32 = 0.005_235_988; // 0.3°
/// Maximum FOV = 120° (radians).
pub const FOV_Y_MAX: f32 = 2.094_395_1; // 120°

const NEAR_FACTOR: f32 = 1.0e-4;
const MIN_DIST: f32 = 1.0;
const MAX_DIST: f32 = 5_000_000.0;

impl Default for Camera {
    fn default() -> Self {
        let mut camera = Self {
            plane: GraphPlane::default_graph(),
            controller: CameraController::default(),
            settings: default_perspective_settings(),
        };

        camera.controller = default_controller(camera.settings);
        camera.sync_projection_to_controller();
        camera
    }
}

impl Camera {
    /// Reversed-Z + infinite-far view-projection matrix used by nodes-view.
    ///
    /// This intentionally does not use `cam-controls` `view_proj`, because nodes-view
    /// relies on its own dynamic near plane and infinite far reversed-Z path.
    pub fn view_proj(&self, width: f32, height: f32) -> Mat4 {
        self.view_proj_f64(width, height).as_mat4()
    }

    /// The reversed-Z infinite-far view-projection in **f64** (before the f32 downcast).
    ///
    /// Built in f64 because a near-orthographic FOV pushes the eye tens of thousands of world
    /// units back: an f32 build quantizes the ~distance-magnitude view translation, so both the
    /// rendered image (orbit) AND the screen->plane unproject (pan / zoom-to-cursor) go steppy.
    /// [`Self::view_proj`] downcasts this once for the GPU; unproject paths
    /// ([`Self::try_screen_to_graph`]) invert THIS f64 matrix, not the lossy f32 one.
    pub fn view_proj_f64(&self, width: f32, height: f32) -> glam::DMat4 {
        let aspect = (width.max(1.0) / height.max(1.0)) as f64;
        let pose = self.controller.pose();
        let near = self.near_plane() as f64;
        let fov = self.settings.fov_y as f64;
        let eye = pose.eye.as_dvec3();
        let fwd = pose.forward().as_dvec3();
        let up = pose.up().as_dvec3();
        // glam 0.33: the `DMat4::*_rh` constructors moved to `dcamera::rh::*` (identical math;
        // `directx` = 0..1 depth, our reverse-Z convention).
        let proj =
            glam::dcamera::rh::proj::directx::perspective_infinite_reverse(fov, aspect, near);
        let view = glam::dcamera::rh::view::look_at_mat4(eye, eye + fwd, up);
        proj * view
    }

    /// Vertical FOV in radians (lens parameter source of truth).
    #[inline]
    pub fn fov_y(&self) -> f32 {
        self.settings.fov_y
    }

    /// Distance used by viewport scaling math.
    ///
    /// Orbit backends use eye-to-target distance; fly backends use eye-to-plane distance
    /// along the forward ray.
    #[inline]
    pub fn distance(&self) -> f32 {
        let pose = self.controller.pose();
        if let Some(target) = self.orbit_target_world() {
            (pose.eye - target).length()
        } else {
            distance_to_plane_along_forward(self.plane, pose.eye, pose.forward())
        }
    }

    #[inline]
    fn near_plane(&self) -> f32 {
        (self.distance() * NEAR_FACTOR).max(1.0e-4)
    }

    /// Camera forward direction.
    #[inline]
    pub fn forward(&self) -> Vec3 {
        self.controller.pose().forward()
    }

    /// Exact pixels-per-world at `world` under perspective projection.
    pub fn px_per_world_at(&self, world: Vec3, viewport: Vec2) -> f32 {
        let w = viewport.x.max(1.0);
        let h = viewport.y.max(1.0);
        let aspect = w / h;
        let proj_xx = (1.0 / (self.settings.fov_y * 0.5).tan()) / aspect;
        let clip = self.view_proj(w, h) * world.extend(1.0);
        let clip_w = clip.w;
        if clip_w <= 1.0e-6 {
            return 0.0;
        }
        0.5 * w * proj_xx / clip_w
    }

    /// Pixels-per-world evaluated at the graph target point.
    #[inline]
    pub fn px_per_world_at_target(&self, viewport: Vec2) -> f32 {
        self.px_per_world_at(self.target_world_for_plane_ops(), viewport)
    }

    /// Dolly distance needed for a requested `px_per_world` scale at the plane.
    pub fn distance_for_px_per_world(&self, px_per_world: f32, viewport: Vec2) -> f32 {
        let h = viewport.y.max(1.0);
        let ppw = px_per_world.max(1.0e-6);
        (0.5 * h / ((self.settings.fov_y * 0.5).tan() * ppw)).clamp(MIN_DIST, MAX_DIST)
    }

    /// Reset to 1 world unit ~= 1 px at the graph target.
    pub fn reset_default_zoom(&mut self, viewport: Vec2) {
        self.set_distance(self.distance_for_px_per_world(1.0, viewport));
    }

    /// Set absolute dolly distance (orbit: eye-target, fly: eye-plane along forward).
    pub fn set_distance(&mut self, distance: f32) {
        self.cancel_active_animation();
        let d = distance.clamp(MIN_DIST, MAX_DIST);
        match &mut self.controller {
            CameraController::Houdini(orbit) => {
                orbit.rig.pose.distance = d;
            }
            CameraController::Quat(orbit) => {
                orbit.distance = d;
            }
            CameraController::Fps(fps) => {
                let pose = fps.pose();
                let current = distance_to_plane_along_forward(self.plane, pose.eye, pose.forward());
                let shift = current - d;
                fps.eye += pose.forward() * shift;
            }
            CameraController::Space(space) => {
                let pose = space.pose();
                let current = distance_to_plane_along_forward(self.plane, pose.eye, pose.forward());
                let shift = current - d;
                space.eye += pose.forward() * shift;
            }
        }
    }

    /// Multiplicative dolly (`factor > 1` zooms in).
    pub fn dolly(&mut self, factor: f32) {
        let f = factor.max(1.0e-4);
        self.set_distance(self.distance() / f);
    }

    /// Adjust FOV and compensate distance to preserve on-plane apparent size.
    pub fn adjust_fov(&mut self, delta: f32) {
        let old = self.settings.fov_y;
        let new = (old + delta).clamp(FOV_Y_MIN, FOV_Y_MAX);
        if (new - old).abs() < 1.0e-9 {
            return;
        }

        let scale = (old * 0.5).tan() / (new * 0.5).tan();
        let dist = self.distance();
        self.settings.fov_y = new;
        self.sync_projection_to_controller();
        self.set_distance(dist * scale);
    }

    /// Pan along graph plane UV axes.
    pub fn pan_on_plane(&mut self, delta_uv: Vec2) {
        self.cancel_active_animation();
        // Match GraphPlane convention: world delta = -axis_u*u - axis_v*v.
        let world_delta = -self.plane.axis_u * delta_uv.x - self.plane.axis_v * delta_uv.y;
        self.translate_world(-world_delta);
    }

    #[inline]
    pub fn pan(&mut self, delta_uv: Vec2) {
        self.pan_on_plane(delta_uv);
    }

    /// Orbit around the current target.
    pub fn orbit(&mut self, delta_azimuth: f32, delta_elevation: f32) {
        self.cancel_active_animation();
        self.controller.apply_intent(
            CameraIntent::Orbit {
                yaw_delta: delta_azimuth,
                pitch_delta: delta_elevation,
            },
            0.0,
            ViewportSize::new(1, 1),
        );
    }

    /// Cursor-anchored dolly: keeps graph point under cursor fixed.
    pub fn zoom_toward(&mut self, cursor_screen: Vec2, factor: f32, vp: Vec2) {
        self.cancel_active_animation();
        let before = self.screen_to_graph(cursor_screen, vp.x, vp.y);
        self.dolly(factor);
        let after = self.screen_to_graph(cursor_screen, vp.x, vp.y);
        self.pan_on_plane(before - after);
    }

    /// Project graph `(u, v)` to screen pixels.
    pub fn graph_to_screen(&self, uv: Vec2, width: f32, height: f32) -> Vec2 {
        let world = self.plane.graph_to_world(uv.x, uv.y);
        self.world_point_to_screen(world, width, height)
    }

    /// Axis-aligned visible graph UV bounds.
    pub fn visible_graph_uv_bounds(&self, width: f32, height: f32) -> (Vec2, Vec2) {
        let r = crate::visible_graph_uv_rect(self, Vec2::new(width.max(1.0), height.max(1.0)));
        (r.min, r.max)
    }

    /// WORLD-space point where the cursor ray meets the graph plane, in full f64 precision.
    /// `None` when the ray is parallel to the plane or the plane sits behind the camera.
    ///
    /// This is the single robust screen→plane hit the whole editor keys off: node picking and
    /// (now) graph-plane WIDGET hit-testing both consume it (the latter via
    /// `InputRouter::handle_with_point`), so a field hit-tests identically to a node at every
    /// camera angle. Kept in f64 because the eye sits ~distance (tens of thousands of units) from
    /// the plane under the near-orthographic FOV; an f32 inverse quantizes the hit into visible
    /// steps. [`Self::try_screen_to_graph`] and [`Self::screen_ray_world`] are thin wrappers.
    pub fn screen_plane_hit_world(
        &self,
        screen: Vec2,
        width: f32,
        height: f32,
    ) -> Option<glam::Vec3> {
        let inv = self.view_proj_f64(width, height).inverse();
        let ndc = self.screen_to_ndc(screen, width, height);
        let near_pt = inv.project_point3(glam::DVec3::new(ndc.x as f64, ndc.y as f64, 1.0));
        let eye = self.controller.pose().eye.as_dvec3();
        let dir = near_pt - eye;
        if dir.length_squared() < 1.0e-18 {
            return None;
        }
        let normal = self.plane.normal.as_dvec3();
        let denom = dir.dot(normal);
        if denom.abs() < 1.0e-9 {
            return None;
        }
        let t = (self.plane.origin.as_dvec3() - eye).dot(normal) / denom;
        if t < 0.0 {
            return None;
        }
        Some((eye + dir * t).as_vec3())
    }

    /// Perspective unproject (screen -> graph UV), `None` when ray misses plane.
    pub fn try_screen_to_graph(&self, screen: Vec2, width: f32, height: f32) -> Option<Vec2> {
        self.screen_plane_hit_world(screen, width, height)
            .map(|hit| self.plane.world_to_graph(hit))
    }

    /// Precise WORLD-space pick ray for a screen position: `(origin, dir_normalized)`, or `None`
    /// if the ray is degenerate. Uses the SAME f64 unprojection as [`Self::try_screen_to_graph`]
    /// (the f32 `view_proj⁻¹` quantizes badly once the eye is dollied tens of thousands of units
    /// back). Hosts feed this to `InputRouter::handle_with_ray` so node-plane widgets hit-test
    /// correctly at any dolly distance — the generic f32 raycast otherwise misses entirely.
    pub fn screen_ray_world(
        &self,
        screen: Vec2,
        width: f32,
        height: f32,
    ) -> Option<(glam::Vec3, glam::Vec3)> {
        // Perpendicular pick ray: origin at the graph-plane point under the cursor, direction along
        // the plane normal toward the eye. Retained for callers that still want a ray; graph-plane
        // widget hit-testing now prefers [`Self::screen_plane_hit_world`] +
        // `InputRouter::handle_with_point` (no ray math at all).
        let hit = self
            .screen_plane_hit_world(screen, width, height)?
            .as_dvec3();
        let eye = self.controller.pose().eye.as_dvec3();
        let normal = self.plane.normal.as_dvec3();
        let toward_eye = normal * (eye - hit).dot(normal).signum();
        Some((hit.as_vec3(), toward_eye.as_vec3()))
    }

    /// Perspective unproject with fallback to current graph target.
    pub fn screen_to_graph(&self, screen: Vec2, width: f32, height: f32) -> Vec2 {
        match self.try_screen_to_graph(screen, width, height) {
            Some(uv) => uv,
            None => self.plane.world_to_graph(self.target_world_for_plane_ops()),
        }
    }

    /// Graph UV under the current target point.
    #[inline]
    pub fn target_on_graph(&self) -> Vec2 {
        self.plane.world_to_graph(self.target_world_for_plane_ops())
    }

    /// Restore front-board orientation while preserving current graph target.
    pub fn reset_top_down(&mut self) {
        let target_uv = self.target_on_graph();
        let world_target = self.plane.graph_to_world(target_uv.x, target_uv.y);
        let pose = self.controller.pose();
        let along_n = (pose.eye - world_target).dot(self.plane.normal).abs();
        let dist = if along_n > 50.0 {
            along_n
        } else {
            DEFAULT_FRONT_DISTANCE
        };

        let eye = world_target + self.plane.normal * dist;
        self.controller.set_pose(make_pose(
            eye,
            world_target,
            self.settings.fov_y,
            self.settings.near,
            self.settings.far,
        ));

        if let CameraController::Houdini(orbit) = &mut self.controller {
            orbit.rig.pose.target = world_target;
            orbit.rig.pose.distance = dist.clamp(MIN_DIST, MAX_DIST);
        }
    }

    /// Project world point to screen coordinates.
    pub fn world_point_to_screen(&self, world: Vec3, width: f32, height: f32) -> Vec2 {
        let ndc = self.view_proj(width, height).project_point3(world);
        Vec2::new((ndc.x + 1.0) * 0.5 * width, (1.0 - ndc.y) * 0.5 * height)
    }

    fn screen_to_ndc(&self, screen: Vec2, width: f32, height: f32) -> Vec2 {
        Vec2::new(screen.x / width * 2.0 - 1.0, 1.0 - screen.y / height * 2.0)
    }

    /// Fit graph UV bounds into the viewport.
    pub fn fit_plane_bounds(
        &mut self,
        min_uv: Vec2,
        max_uv: Vec2,
        viewport: Vec2,
        padding: f32,
        preserve_orbit: bool,
    ) {
        if preserve_orbit {
            self.fit_plane_bounds_preserve_orbit(min_uv, max_uv, viewport, padding);
        } else {
            self.fit_plane_bounds_top_down(min_uv, max_uv, viewport, padding);
        }
    }

    /// Animate to the same fitted camera pose as [`Self::fit_plane_bounds`].
    ///
    /// This is orbit-only (`Houdini` backend). For other controller kinds it returns
    /// `false` and leaves the camera unchanged so callers can fall back to instant fit.
    pub fn fit_plane_bounds_animated(
        &mut self,
        min_uv: Vec2,
        max_uv: Vec2,
        viewport: Vec2,
        padding: f32,
        preserve_orbit: bool,
        speed: f32,
    ) -> bool {
        let mut fitted = self.clone();
        fitted.fit_plane_bounds(min_uv, max_uv, viewport, padding, preserve_orbit);

        let Some(goal_pose) = fitted.houdini_pose_for_animation() else {
            return false;
        };
        let Some(rig) = self.houdini_rig_mut() else {
            return false;
        };

        let (orientation, distance, target) = goal_pose;
        let (yaw, pitch) = yaw_pitch_from_orientation(orientation);
        rig.transition_speed = speed.max(0.0);
        rig.animate_to(yaw, pitch, distance, target);
        true
    }

    /// Animate to the same top-down pose as [`Self::reset_top_down`].
    ///
    /// This is orbit-only (`Houdini` backend). For other controller kinds it returns
    /// `false` and leaves the camera unchanged so callers can fall back to instant reset.
    pub fn reset_top_down_animated(&mut self, speed: f32) -> bool {
        let mut reset = self.clone();
        reset.reset_top_down();

        let Some(goal_pose) = reset.houdini_pose_for_animation() else {
            return false;
        };
        let Some(rig) = self.houdini_rig_mut() else {
            return false;
        };

        let (orientation, distance, target) = goal_pose;
        let (yaw, pitch) = yaw_pitch_from_orientation(orientation);
        rig.transition_speed = speed.max(0.0);
        rig.animate_to(yaw, pitch, distance, target);
        true
    }

    fn fit_plane_bounds_top_down(
        &mut self,
        min_uv: Vec2,
        max_uv: Vec2,
        viewport: Vec2,
        padding: f32,
    ) {
        let center_uv = (min_uv + max_uv) * 0.5;
        let world_center = self.plane.graph_to_world(center_uv.x, center_uv.y);

        let dist = self.distance().max(DEFAULT_FRONT_DISTANCE);
        let eye = world_center + self.plane.normal * dist;
        self.controller.set_pose(make_pose(
            eye,
            world_center,
            self.settings.fov_y,
            self.settings.near,
            self.settings.far,
        ));

        if let CameraController::Houdini(orbit) = &mut self.controller {
            orbit.rig.pose.target = world_center;
            orbit.rig.pose.distance = dist.clamp(MIN_DIST, MAX_DIST);
        }

        let mut span = max_uv - min_uv;
        if span.x < 1.0 {
            span.x = 1.0;
        }
        if span.y < 1.0 {
            span.y = 1.0;
        }

        let aspect = viewport.x.max(1.0) / viewport.y.max(1.0);
        let pad = 1.0 + padding.max(0.0);
        let t = (self.settings.fov_y * 0.5).tan().max(1.0e-6);
        let half = span * 0.5 * pad;
        let dist_v = half.y / t;
        let dist_h = half.x / (t * aspect);
        self.set_distance(dist_v.max(dist_h));

        let screen_center = viewport * 0.5;
        let current_uv = self.screen_to_graph(screen_center, viewport.x, viewport.y);
        self.pan_on_plane(center_uv - current_uv);
    }

    fn fit_plane_bounds_preserve_orbit(
        &mut self,
        min_uv: Vec2,
        max_uv: Vec2,
        viewport: Vec2,
        padding: f32,
    ) {
        let center_uv = (min_uv + max_uv) * 0.5;
        let world_center = self.plane.graph_to_world(center_uv.x, center_uv.y);
        // Pivot the orbit on the selection's center: move the ORBIT TARGET (not the forward-ray
        // plane-hit `target_world_for_plane_ops` returns) exactly onto `world_center`, so later
        // orbits rotate about the framed selection. A turntable keeps its target at screen center, so
        // we then only dolly to fit at the current angle — NO screen-centering pan (that pan moved the
        // pivot OFF the selection in perspective, which is the reported "F breaks the pivot" bug).
        let current_pivot = self
            .orbit_target_world()
            .unwrap_or_else(|| self.target_world_for_plane_ops());
        self.translate_world(world_center - current_pivot);

        let w = viewport.x.max(1.0);
        let h = viewport.y.max(1.0);
        let screen_center = viewport * 0.5;

        let corners = [
            Vec2::new(min_uv.x, min_uv.y),
            Vec2::new(max_uv.x, min_uv.y),
            Vec2::new(max_uv.x, max_uv.y),
            Vec2::new(min_uv.x, max_uv.y),
        ];
        let pad = 1.0 + padding.max(0.0);
        let mut max_dx = 0.0f32;
        let mut max_dy = 0.0f32;
        for uv in corners {
            let s = self.graph_to_screen(uv, w, h);
            max_dx = max_dx.max((s.x - screen_center.x).abs());
            max_dy = max_dy.max((s.y - screen_center.y).abs());
        }

        let scale = ((max_dx * 2.0 * pad) / w).max((max_dy * 2.0 * pad) / h);
        if scale > 1.0e-4 {
            self.set_distance(self.distance() * scale);
        }
    }

    /// Active orbit orientation policy label/toggle used by editor HUD.
    pub fn orbit_mode(&self) -> OrbitMode {
        mode_from_controller(&self.controller)
    }

    pub fn set_orbit_mode(&mut self, mode: OrbitMode) {
        if let CameraController::Houdini(orbit) = &mut self.controller {
            orbit.rig.orientation_mode = orientation_mode_from_orbit_mode(mode);
        }
    }

    pub fn cycle_orbit_mode(&mut self) -> OrbitMode {
        let next = self.orbit_mode().cycle();
        self.set_orbit_mode(next);
        next
    }

    /// Camera backend kind (`Houdini`, `Quat`, `Fps`, `Space`).
    pub fn kind(&self) -> CameraKind {
        match self.controller {
            CameraController::Houdini(_) => CameraKind::Houdini,
            CameraController::Quat(_) => CameraKind::Quat,
            CameraController::Fps(_) => CameraKind::Fps,
            CameraController::Space(_) => CameraKind::Space,
        }
    }

    /// Switch backend while preserving the current pose and projection settings.
    pub fn set_kind(&mut self, kind: CameraKind) {
        self.cancel_active_animation();
        let pose = self.controller.pose();
        let orbit_mode = self.orbit_mode();
        self.controller = CameraController::from_pose(kind, pose);
        self.sync_projection_to_controller();
        self.set_orbit_mode(orbit_mode);
    }

    /// Ensure the active backend is the Houdini ORBIT rig, seating the pivot on the graph-plane
    /// point currently under the view centre so a subsequent orbit tumbles around what the user is
    /// looking at.
    ///
    /// **Why:** the fly backends (`Fps`/`Space`) map an `Orbit` intent to free mouse-LOOK (rotate in
    /// place), not a turntable about a pivot — correct for flying, wrong for the board's Alt+LMB
    /// orbit. And releasing RMB-fly deliberately KEEPS the `Space` backend so momentum can coast, so
    /// without this the editor would be stuck free-looking (the "orbit / H don't work after a flight"
    /// trap). The editor calls this before every board orbit/framing gesture, mirroring how
    /// `set_flying` ensures the fly backend — each gesture guarantees the backend it needs.
    ///
    /// The eye is FROZEN across the switch: the plane hit lies along the forward ray at distance
    /// `t`, so re-seating `target = hit`, `distance = t` reproduces `eye = target - forward*t`
    /// exactly (no view jump). `from_pose` otherwise seeds an arbitrary 28-unit-ahead target that
    /// would float off the graph. No-op when already on the orbit backend.
    pub fn adopt_orbit_backend(&mut self) {
        if matches!(self.controller, CameraController::Houdini(_)) {
            return;
        }
        let pivot = self.target_world_for_plane_ops();
        let eye = self.controller.pose().eye;
        self.set_kind(CameraKind::Houdini);
        let dist = (eye - pivot).length().clamp(MIN_DIST, MAX_DIST);
        if let CameraController::Houdini(orbit) = &mut self.controller {
            orbit.rig.pose.target = pivot;
            orbit.rig.pose.distance = dist;
        }
    }

    /// Camera pose snapshot (backend-agnostic).
    pub fn pose(&self) -> CameraPose {
        self.controller.pose()
    }

    /// Current world-space target point used for plane operations.
    pub fn target_world(&self) -> Vec3 {
        self.target_world_for_plane_ops()
    }

    /// Mutable access to the underlying controller (Phase 3 bridge).
    pub fn controller(&mut self) -> &mut CameraController {
        &mut self.controller
    }

    /// Apply raw camera intent to the active backend.
    pub fn apply_intent(&mut self, intent: CameraIntent, dt: f32, viewport: ViewportSize) {
        self.controller.apply_intent(intent, dt, viewport);
    }

    /// Advance backend dynamics/inertia; returns `true` while motion remains.
    pub fn update_dynamics(&mut self, dt: f32) -> bool {
        self.controller.update_dynamics(dt)
    }

    /// Whether an orbit transition animation is currently active.
    ///
    /// Non-orbit backends always return `false`.
    pub fn is_animating(&self) -> bool {
        self.houdini_rig().is_some_and(OrbitRig::is_animating)
    }

    fn sync_projection_to_controller(&mut self) {
        self.controller.set_projection(self.settings);
    }

    fn cancel_active_animation(&mut self) {
        if let Some(rig) = self.houdini_rig_mut() {
            rig.cancel_animation();
        }
    }

    fn houdini_rig(&self) -> Option<&OrbitRig> {
        match &self.controller {
            CameraController::Houdini(orbit) => Some(&orbit.rig),
            CameraController::Quat(_) | CameraController::Fps(_) | CameraController::Space(_) => {
                None
            }
        }
    }

    fn houdini_rig_mut(&mut self) -> Option<&mut OrbitRig> {
        match &mut self.controller {
            CameraController::Houdini(orbit) => Some(&mut orbit.rig),
            CameraController::Quat(_) | CameraController::Fps(_) | CameraController::Space(_) => {
                None
            }
        }
    }

    fn houdini_pose_for_animation(&self) -> Option<(Quat, f32, Vec3)> {
        self.houdini_rig()
            .map(|rig| (rig.pose.orientation, rig.pose.distance, rig.pose.target))
    }

    fn translate_world(&mut self, delta_world: Vec3) {
        match &mut self.controller {
            CameraController::Houdini(orbit) => {
                orbit.rig.pose.target += delta_world;
            }
            CameraController::Quat(orbit) => {
                orbit.target += delta_world;
            }
            CameraController::Fps(fps) => {
                fps.eye += delta_world;
            }
            CameraController::Space(space) => {
                space.eye += delta_world;
            }
        }
    }

    fn orbit_target_world(&self) -> Option<Vec3> {
        match &self.controller {
            CameraController::Houdini(orbit) => Some(orbit.rig.pose.target),
            CameraController::Quat(orbit) => Some(orbit.target),
            CameraController::Fps(_) | CameraController::Space(_) => None,
        }
    }

    fn target_world_for_plane_ops(&self) -> Vec3 {
        let pose = self.controller.pose();
        if let Some(hit) = ray_plane_hit_world(self.plane, pose.eye, pose.forward()) {
            return hit;
        }
        if let Some(target) = self.orbit_target_world() {
            return target;
        }
        pose.eye + pose.forward() * self.distance()
    }
}

fn orientation_mode_from_orbit_mode(mode: OrbitMode) -> OrientationMode {
    match mode {
        OrbitMode::Quaternion => OrientationMode::Quaternion,
        OrbitMode::HoudiniTurntable => OrientationMode::HorizontalPlaneEuler,
    }
}

fn orbit_mode_from_orientation_mode(mode: OrientationMode) -> OrbitMode {
    match mode {
        OrientationMode::Quaternion => OrbitMode::Quaternion,
        OrientationMode::HorizontalPlaneEuler | OrientationMode::Euler => {
            OrbitMode::HoudiniTurntable
        }
    }
}

fn mode_from_controller(controller: &CameraController) -> OrbitMode {
    match controller {
        CameraController::Houdini(orbit) => {
            orbit_mode_from_orientation_mode(orbit.rig.orientation_mode)
        }
        CameraController::Quat(_) | CameraController::Fps(_) | CameraController::Space(_) => {
            OrbitMode::Quaternion
        }
    }
}

fn default_perspective_settings() -> PerspectiveSettings {
    PerspectiveSettings {
        fov_y: DEFAULT_FOV_Y,
        near: 1.0e-4,
        far: 10_000_000.0,
        distance_min: MIN_DIST,
        distance_max: MAX_DIST,
    }
}

fn default_controller(settings: PerspectiveSettings) -> CameraController {
    let orientation = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
    let mut rig = OrbitRig::from_pose(cam_controls::OrbitPose {
        orientation,
        distance: DEFAULT_FRONT_DISTANCE,
        target: Vec3::ZERO,
    });
    rig.projection = settings;
    // Turntable orbit (azimuth about world-up + elevation, NO roll) is the correct feel for a graph
    // plane and avoids the pivot/gimbal artifacts of the free Quaternion mode near the top-down pose
    // (cam-controls docs: HorizontalPlaneEuler = "no gimbal at horizon" vs Quaternion = "legacy, can
    // gimbal near poles"). Runtime-toggle back to Quaternion with `O` (CycleOrbitMode) to compare.
    rig.orientation_mode = orientation_mode_from_orbit_mode(OrbitMode::HoudiniTurntable);
    CameraController::Houdini(cam_controls::HoudiniOrbit::new(rig))
}

fn make_pose(eye: Vec3, target: Vec3, fov_y: f32, znear: f32, zfar: f32) -> CameraPose {
    let forward = (target - eye).normalize_or_zero();
    CameraPose::from_eye_forward_up(eye, forward, Vec3::Y, fov_y, znear, zfar)
}

fn distance_to_plane_along_forward(plane: GraphPlane, eye: Vec3, forward: Vec3) -> f32 {
    let denom = forward.dot(plane.normal);
    if denom.abs() < 1.0e-6 {
        return DEFAULT_FRONT_DISTANCE;
    }
    let t = (plane.origin - eye).dot(plane.normal) / denom;
    if t.is_finite() && t > 1.0e-4 {
        t.clamp(MIN_DIST, MAX_DIST)
    } else {
        DEFAULT_FRONT_DISTANCE
    }
}

fn ray_plane_hit_world(plane: GraphPlane, eye: Vec3, dir: Vec3) -> Option<Vec3> {
    let denom = dir.dot(plane.normal);
    if denom.abs() < 1.0e-6 {
        return None;
    }
    let t = (plane.origin - eye).dot(plane.normal) / denom;
    if t < 0.0 {
        return None;
    }
    Some(eye + dir * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_pose_matches_board_rest() {
        let cam = Camera::default();
        let pose = cam.controller.pose();
        assert!((pose.eye - Vec3::new(0.0, 0.0, -DEFAULT_FRONT_DISTANCE)).length() < 1.0e-3);
        assert!(pose.forward().dot(Vec3::Z) > 0.999);
        assert!(pose.up().dot(Vec3::Y) > 0.999);
    }

    #[test]
    fn default_center_maps_near_origin() {
        let cam = Camera::default();
        let w = 800.0;
        let h = 600.0;
        let uv = cam.screen_to_graph(Vec2::new(w * 0.5, h * 0.5), w, h);
        assert!(
            uv.length() < 1.0,
            "center should be near graph origin, got {uv:?}"
        );
    }

    #[test]
    fn plane_screen_roundtrip() {
        let cam = Camera::default();
        let w = 1280.0;
        let h = 720.0;
        let uv0 = Vec2::new(120.0, -45.0);
        let screen = cam.graph_to_screen(uv0, w, h);
        let uv1 = cam.screen_to_graph(screen, w, h);
        assert!((uv0 - uv1).length() < 0.5, "uv0={uv0:?} uv1={uv1:?}");
    }

    #[test]
    fn pan_on_plane_shifts_target_negatively() {
        let mut cam = Camera::default();
        let before = cam.target_on_graph();
        let d = Vec2::new(100.0, 50.0);
        cam.pan_on_plane(d);
        let after = cam.target_on_graph();
        assert!(
            (after - (before - d)).length() < 0.5,
            "expected target -= d; before={before:?} after={after:?}"
        );
    }

    #[test]
    fn orbit_keeps_target_on_graph() {
        let mut cam = Camera::default();
        let uv0 = cam.target_on_graph();
        cam.orbit(0.6, 0.25);
        let uv1 = cam.target_on_graph();
        assert!((uv0 - uv1).length() < 1.0e-4);
    }

    #[test]
    fn fit_plane_bounds_animated_converges_to_instant_fit_pose() {
        let min_uv = Vec2::new(-320.0, -180.0);
        let max_uv = Vec2::new(440.0, 260.0);
        let viewport = Vec2::new(1280.0, 720.0);

        let mut animated = Camera::default();
        let mut instant = animated.clone();
        instant.fit_plane_bounds(min_uv, max_uv, viewport, 0.15, false);

        assert!(animated.fit_plane_bounds_animated(min_uv, max_uv, viewport, 0.15, false, 8.0));
        assert!(animated.is_animating());

        for _ in 0..1200 {
            animated.update_dynamics(1.0 / 60.0);
        }

        let got = animated.pose();
        let expected = instant.pose();
        assert!((got.eye - expected.eye).length() < 1.0e-2);
        assert!(got.forward().dot(expected.forward()) > 0.999_9);
        assert!((animated.target_world() - instant.target_world()).length() < 1.0e-2);
    }
}
