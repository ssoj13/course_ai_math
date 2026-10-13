// Vendored and adapted from `transform-gizmo` 0.9.0.
// Copyright (c) Urho Laukkarinen. Licensed under MIT OR Apache-2.0. See NOTICE.
// Adaptations (glam-f64 internalisation, egui-only deps) are part of egui-gizmo
// and licensed BSD-3-Clause.
//
//! Internal gizmo configuration and its prepared (derived) form.
//!
//! NOTE: this is the gizmo's INTERNAL config. The public facade
//! ([`crate::GizmoConfig`]) builds this from glam-f32 inputs. Here the view and
//! projection matrices are already **glam f64, column-major** (`DMat4`) — they
//! were widened element-wise from the f32 public matrices with NO transpose.

use std::ops::{Deref, DerefMut};

pub use egui::Color32;

use egui::Rect;
use enumset::{EnumSet, EnumSetType, enum_set};

use crate::math::{DMat4, DQuat, DVec3, Transform, screen_to_world, world_to_screen};
use glam::Vec2;

/// The default snapping distance for rotation in radians
pub const DEFAULT_SNAP_ANGLE: f32 = std::f32::consts::PI / 32.0;
/// The default snapping distance for translation
pub const DEFAULT_SNAP_DISTANCE: f32 = 0.1;
/// The default snapping distance for scale
pub const DEFAULT_SNAP_SCALE: f32 = 0.1;

/// Configuration of a gizmo.
///
/// Defines how the gizmo is drawn to the screen and
/// how it can be interacted with.
#[derive(Debug, Copy, Clone)]
pub struct GizmoConfig {
    /// View matrix for the gizmo, aligning it with the camera's viewpoint.
    /// Glam f64, column-major (widened from the f32 public matrix, no transpose).
    pub view_matrix: DMat4,
    /// Projection matrix for the gizmo, determining how it is projected onto the screen.
    /// Glam f64, column-major (widened from the f32 public matrix, no transpose).
    pub projection_matrix: DMat4,
    /// Screen area where the gizmo is displayed.
    pub viewport: Rect,
    /// The gizmo's operation modes.
    pub modes: EnumSet<GizmoMode>,
    /// If set, this mode is forced active and other modes are disabled
    pub mode_override: Option<GizmoMode>,
    /// Determines the gizmo's orientation relative to global or local axes.
    pub orientation: GizmoOrientation,
    /// Pivot point for transformations
    pub pivot_point: TransformPivotPoint,
    /// Toggles snapping to predefined increments during transformations for precision.
    pub snapping: bool,
    /// Angle increment for snapping rotations, in radians.
    pub snap_angle: f32,
    /// Distance increment for snapping translations.
    pub snap_distance: f32,
    /// Scale increment for snapping scalings.
    pub snap_scale: f32,
    /// Visual settings for the gizmo, affecting appearance and visibility.
    pub visuals: GizmoVisuals,
    /// Ratio of window's physical size to logical size.
    pub pixels_per_point: f32,
    /// Half-extent (local X/Y) of the 2D bounding-box handle set, in the gizmo's
    /// LOCAL z=0 plane. Read by the `bbox` subgizmos; ignored when no bbox handle
    /// is enabled. Threaded from the public facade's `bbox_half_extent`.
    pub bbox_half_extent: Vec2,
}

impl Default for GizmoConfig {
    fn default() -> Self {
        Self {
            view_matrix: DMat4::IDENTITY,
            projection_matrix: DMat4::IDENTITY,
            viewport: Rect::NOTHING,
            modes: GizmoMode::all(),
            mode_override: None,
            orientation: GizmoOrientation::default(),
            pivot_point: TransformPivotPoint::default(),
            snapping: false,
            snap_angle: DEFAULT_SNAP_ANGLE,
            snap_distance: DEFAULT_SNAP_DISTANCE,
            snap_scale: DEFAULT_SNAP_SCALE,
            visuals: GizmoVisuals::default(),
            pixels_per_point: 1.0,
            bbox_half_extent: Vec2::splat(0.5),
        }
    }
}

impl GizmoConfig {
    /// Forward vector of the view camera.
    ///
    /// The camera basis lives in the **rows** of the (column-major) view matrix,
    /// so we read `row(2)` here — NOT `z_axis` (which is a column). Using the
    /// columns would silently flip the gizmo's facing/fade logic.
    pub(crate) fn view_forward(&self) -> DVec3 {
        self.view_matrix.row(2).truncate()
    }

    /// Up vector of the view camera (row 1 of the view matrix).
    pub(crate) fn view_up(&self) -> DVec3 {
        self.view_matrix.row(1).truncate()
    }

    /// Right vector of the view camera (row 0 of the view matrix).
    pub(crate) fn view_right(&self) -> DVec3 {
        self.view_matrix.row(0).truncate()
    }

    /// Whether local orientation is used
    pub(crate) fn local_space(&self) -> bool {
        self.orientation() == GizmoOrientation::Local
    }

    /// Transform orientation of the gizmo
    pub(crate) fn orientation(&self) -> GizmoOrientation {
        self.orientation
    }

    /// Whether the modes have changed, compared to given other config
    pub(crate) fn modes_changed(&self, other: &Self) -> bool {
        (self.modes != other.modes && self.mode_override.is_none())
            || (self.mode_override != other.mode_override)
    }
}

#[derive(Debug, Copy, Clone, Default)]
pub(crate) struct PreparedGizmoConfig {
    config: GizmoConfig,
    /// Rotation of the gizmo
    pub(crate) rotation: DQuat,
    /// Translation of the gizmo
    pub(crate) translation: DVec3,
    /// Scale of the gizmo
    pub(crate) scale: DVec3,
    /// Combined view-projection matrix
    pub(crate) view_projection: DMat4,
    /// Model matrix from targets
    pub(crate) model_matrix: DMat4,
    /// Combined model-view-projection matrix
    pub(crate) mvp: DMat4,
    /// Scale factor for the gizmo rendering
    pub(crate) scale_factor: f32,
    /// How close the mouse pointer needs to be to a subgizmo before it is focused
    pub(crate) focus_distance: f32,
    /// Whether left-handed projection is used
    pub(crate) left_handed: bool,
    /// Direction from the camera to the gizmo in world space
    pub(crate) eye_to_model_dir: DVec3,
    /// Aspect-lock modifier (Ctrl) state for THIS frame. This is per-frame
    /// interaction state, not part of the immutable [`GizmoConfig`], so it lives
    /// on the prepared config and is set by [`crate::gizmo::Gizmo::update`] before
    /// the subgizmos read it (used by the `bbox` handles). Defaults to `false`.
    pub(crate) aspect_lock: bool,
    /// The four WORLD-space corner positions of the 2D corner-pin (Nuke-style)
    /// handle, in a fixed order (SW, SE, NE, NW). Unlike every other handle, the
    /// corner-pin's outcome is a homography (NOT a TRS `Transform`), so the gizmo
    /// must OWN these positions across frames — they are seeded from the target's
    /// box on the first frame, then re-fed from [`crate::gizmo::Gizmo::corner_pin`]
    /// (the persistent store) every subsequent frame. Each corner handle reads all
    /// four here so it can report the full updated set. Defaults to zeros (unused
    /// until a corner-pin handle is enabled). See `subgizmo/corner_pin.rs`.
    pub(crate) corner_pin: [DVec3; 4],
    /// The gizmo-owned 2D shear factors `(kx, ky)`, fed in by
    /// [`crate::gizmo::Gizmo`] every frame BEFORE the config is copied into the
    /// subgizmos. Like `corner_pin`, shear is an affine the gizmo must OWN across
    /// frames (it is NOT TRS, so it cannot ride in the target `Transform`): the
    /// box-frame ([`Self::box_model`]) and the shear handles read it here so the
    /// grips track the sheared box and a re-grab continues from the current value.
    /// Defaults to zero (no shear = byte-identical to the un-sheared frame).
    pub(crate) shear: glam::Vec2,
}

impl Deref for PreparedGizmoConfig {
    type Target = GizmoConfig;

    fn deref(&self) -> &Self::Target {
        &self.config
    }
}

impl DerefMut for PreparedGizmoConfig {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.config
    }
}

impl PreparedGizmoConfig {
    pub(crate) fn update_for_config(&mut self, config: GizmoConfig) {
        // Matrices are already glam f64 column-major — no `mint` round-trip and
        // no transpose. `view_projection = projection * view`.
        let projection_matrix = config.projection_matrix;
        let view_matrix = config.view_matrix;

        let view_projection = projection_matrix * view_matrix;

        let left_handed = if projection_matrix.z_axis.w == 0.0 {
            projection_matrix.z_axis.z > 0.0
        } else {
            projection_matrix.z_axis.w > 0.0
        };

        self.config = config;
        self.view_projection = view_projection;
        self.left_handed = left_handed;

        self.update_transform(Transform {
            scale: self.scale,
            rotation: self.rotation,
            translation: self.translation,
        });
    }

    pub(crate) fn update_for_targets(&mut self, targets: &[Transform]) {
        let mut scale = DVec3::ZERO;
        let mut translation = DVec3::ZERO;
        let mut rotation = DQuat::IDENTITY;

        for target in targets {
            scale += target.scale;
            translation += target.translation;
            rotation = target.rotation;
        }

        let target_count = targets.len();
        if target_count == 0 {
            scale = DVec3::ONE;
        } else {
            translation /= target_count as f64;
            scale /= target_count as f64;
        }

        self.update_transform(Transform {
            scale,
            rotation,
            translation,
        });
    }

    pub(crate) fn update_transform(&mut self, transform: Transform) {
        self.translation = transform.translation;
        self.rotation = transform.rotation;
        self.scale = transform.scale;
        self.model_matrix =
            DMat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation);
        self.mvp = self.view_projection * self.model_matrix;

        self.scale_factor = self.mvp.as_ref()[15] as f32
            / self.projection_matrix.x_axis.x as f32
            / self.config.viewport.width()
            * 2.0;

        let gizmo_screen_pos =
            world_to_screen(self.config.viewport, self.mvp, self.translation).unwrap_or_default();

        let gizmo_view_near = screen_to_world(
            self.config.viewport,
            self.view_projection.inverse(),
            gizmo_screen_pos,
            -1.0,
        );

        self.focus_distance = self.scale_factor * (self.config.visuals.stroke_width / 2.0 + 5.0);

        self.eye_to_model_dir = (gizmo_view_near - self.translation).normalize_or_zero();
    }

    /// Build the 2D shear affine (column-major `DMat4`) for factors `(kx, ky)`.
    ///
    /// Column-major glam: `M * (x,y,z,1)` = `x*col0 + y*col1 + z*col2 + col3`, so
    /// with col0 = `(1, ky, 0, 0)` and col1 = `(kx, 1, 0, 0)` the result is
    /// `(x + kx*y, y + ky*x, z)` — exactly the demo's 2×2 shear about the local
    /// origin (`x' = x + kx*y; y' = y + ky*x`). Identity when shear is zero.
    pub(crate) fn shear_matrix(shear: glam::Vec2) -> DMat4 {
        let (kx, ky) = (shear.x as f64, shear.y as f64);
        DMat4::from_cols_array(&[
            1.0, ky, 0.0, 0.0, // col 0
            kx, 1.0, 0.0, 0.0, // col 1
            0.0, 0.0, 1.0, 0.0, // col 2
            0.0, 0.0, 0.0, 1.0, // col 3
        ])
    }

    /// The sheared box frame = `model * shear`. The 2D bbox / shear grips draw and
    /// pick against THIS frame so a non-zero owned shear moves them onto the
    /// sheared box. Equals `model_matrix` exactly when `shear` is zero (so the
    /// un-sheared 2D and all of 3D are byte-identical to before — `shear` defaults
    /// to zero and is only ever non-zero in 2D after a shear-grip drag).
    pub(crate) fn box_model(&self) -> DMat4 {
        self.model_matrix * Self::shear_matrix(self.shear)
    }

    pub(crate) fn as_transform(&self) -> Transform {
        Transform {
            scale: self.scale,
            rotation: self.rotation,
            translation: self.translation,
        }
    }
}

/// Operation mode of a gizmo.
#[derive(Debug, EnumSetType, Hash)]
pub enum GizmoMode {
    /// Rotate around the view forward axis
    RotateView,
    /// Rotate around the X axis
    RotateX,
    /// Rotate around the Y axis
    RotateY,
    /// Rotate around the Z axis
    RotateZ,
    /// Translate along the view forward axis
    TranslateView,
    /// Translate along the X axis
    TranslateX,
    /// Translate along the Y axis
    TranslateY,
    /// Translate along the Z axis
    TranslateZ,
    /// Translate along the XY plane
    TranslateXY,
    /// Translate along the XZ plane
    TranslateXZ,
    /// Translate along the YZ plane
    TranslateYZ,
    /// Scale uniformly in all directions
    ScaleUniform,
    /// Scale along the X axis
    ScaleX,
    /// Scale along the Y axis
    ScaleY,
    /// Scale along the Z axis
    ScaleZ,
    /// Scale along the XY plane
    ScaleXY,
    /// Scale along the XZ plane
    ScaleXZ,
    /// Scale along the YZ plane
    ScaleYZ,
    /// Rotate using an arcball (trackball)
    Arcball,
    /// 2D bounding-box scale (corner + edge grips on the local z=0 plane).
    /// Scale-family: [`GizmoMode::kind`] reports `Scale`.
    BoundingBox,
    /// 3D bounding-box scale (6 face + 12 edge + 8 corner grips on the local box).
    /// Scale-family ([`GizmoMode::kind`] reports `Scale`); acts on all three axes.
    BoundingBox3D,
    /// 2D shear (top/right edge grips on the local z=0 plane). NOT TRS — the shear
    /// factor rides on the result `kind`, not the target transform. Reported as
    /// scale-family by [`GizmoMode::kind`] (it shares the scale group's alpha/UX).
    Shear,
    /// 2D corner-pin (Nuke-style): four independently-draggable corners whose
    /// outcome is a homography (NOT TRS). The four corner positions ride on the
    /// result `kind`, not the target transform. Reported as translate-family by
    /// [`GizmoMode::kind`] (the corners are positional).
    CornerPin,
}

impl GizmoMode {
    /// All modes
    pub fn all() -> EnumSet<Self> {
        EnumSet::all()
    }

    /// All rotation modes
    pub const fn all_rotate() -> EnumSet<Self> {
        enum_set!(Self::RotateX | Self::RotateY | Self::RotateZ | Self::RotateView)
    }

    /// All translation modes
    pub const fn all_translate() -> EnumSet<Self> {
        enum_set!(
            Self::TranslateX
                | Self::TranslateY
                | Self::TranslateZ
                | Self::TranslateXY
                | Self::TranslateXZ
                | Self::TranslateYZ
                | Self::TranslateView
        )
    }

    /// All scaling modes
    pub const fn all_scale() -> EnumSet<Self> {
        enum_set!(
            Self::ScaleX
                | Self::ScaleY
                | Self::ScaleZ
                | Self::ScaleXY
                | Self::ScaleXZ
                | Self::ScaleYZ
                | Self::ScaleUniform
        )
    }

    /// Is this mode for rotation
    pub fn is_rotate(&self) -> bool {
        self.kind() == GizmoModeKind::Rotate
    }

    /// Is this mode for translation
    pub fn is_translate(&self) -> bool {
        self.kind() == GizmoModeKind::Translate
    }

    /// Is this mode for scaling
    pub fn is_scale(&self) -> bool {
        self.kind() == GizmoModeKind::Scale
    }

    /// Axes this mode acts on
    pub fn axes(&self) -> EnumSet<GizmoDirection> {
        match self {
            Self::RotateX | Self::TranslateX | Self::ScaleX => {
                enum_set!(GizmoDirection::X)
            }
            Self::RotateY | Self::TranslateY | Self::ScaleY => {
                enum_set!(GizmoDirection::Y)
            }
            Self::RotateZ | Self::TranslateZ | Self::ScaleZ => {
                enum_set!(GizmoDirection::Z)
            }
            Self::RotateView | Self::TranslateView => {
                enum_set!(GizmoDirection::View)
            }
            Self::ScaleUniform | Self::Arcball => {
                enum_set!(GizmoDirection::X | GizmoDirection::Y | GizmoDirection::Z)
            }
            Self::TranslateXY | Self::ScaleXY => {
                enum_set!(GizmoDirection::X | GizmoDirection::Y)
            }
            Self::TranslateXZ | Self::ScaleXZ => {
                enum_set!(GizmoDirection::X | GizmoDirection::Z)
            }
            Self::TranslateYZ | Self::ScaleYZ => {
                enum_set!(GizmoDirection::Y | GizmoDirection::Z)
            }
            // 3D bounding box acts on all three axes.
            Self::BoundingBox3D => {
                enum_set!(GizmoDirection::X | GizmoDirection::Y | GizmoDirection::Z)
            }
            // 2D bounding box / shear / corner-pin all act in the local X/Y
            // (screen) plane.
            Self::BoundingBox | Self::Shear | Self::CornerPin => {
                enum_set!(GizmoDirection::X | GizmoDirection::Y)
            }
        }
    }

    /// Returns the modes that match to given axes exactly
    pub fn all_from_axes(axes: EnumSet<GizmoDirection>) -> EnumSet<Self> {
        EnumSet::<Self>::all()
            .iter()
            .filter(|mode| mode.axes() == axes)
            .collect()
    }

    pub fn kind(&self) -> GizmoModeKind {
        match self {
            Self::RotateX | Self::RotateY | Self::RotateZ | Self::RotateView => {
                GizmoModeKind::Rotate
            }
            Self::TranslateX
            | Self::TranslateY
            | Self::TranslateZ
            | Self::TranslateXY
            | Self::TranslateXZ
            | Self::TranslateYZ
            | Self::TranslateView
            // Corner-pin corners are positional (a homography of points), so the
            // mode is translate-family even though it carries non-TRS data.
            | Self::CornerPin => GizmoModeKind::Translate,
            Self::ScaleX
            | Self::ScaleY
            | Self::ScaleZ
            | Self::ScaleXY
            | Self::ScaleXZ
            | Self::ScaleYZ
            | Self::ScaleUniform
            | Self::BoundingBox
            | Self::BoundingBox3D
            | Self::Shear => GizmoModeKind::Scale,
            Self::Arcball => GizmoModeKind::Arcball,
        }
    }

    pub fn all_from_kind(kind: GizmoModeKind) -> EnumSet<Self> {
        EnumSet::<Self>::all()
            .iter()
            .filter(|mode| mode.kind() == kind)
            .collect()
    }

    pub fn from_kind_and_axes(kind: GizmoModeKind, axes: EnumSet<GizmoDirection>) -> Option<Self> {
        EnumSet::<Self>::all()
            .iter()
            .find(|mode| mode.kind() == kind && mode.axes() == axes)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd)]
pub enum GizmoModeKind {
    Rotate,
    Translate,
    Scale,
    Arcball,
}

/// The point in space around which all rotations are centered.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub enum TransformPivotPoint {
    /// Pivot around the median point of targets
    #[default]
    MedianPoint,
    /// Pivot around each target's own origin
    IndividualOrigins,
}

/// Orientation of a gizmo.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub enum GizmoOrientation {
    /// Transformation axes are aligned to world space.
    #[default]
    Global,
    /// Transformation axes are aligned to the last target's orientation.
    Local,
}

#[derive(Debug, EnumSetType, Hash)]
pub enum GizmoDirection {
    /// Gizmo points in the X-direction
    X,
    /// Gizmo points in the Y-direction
    Y,
    /// Gizmo points in the Z-direction
    Z,
    /// Gizmo points in the view direction
    View,
}

/// Shape drawn at the gizmo's screen-center (free-translate / uniform-scale dot).
///
/// Both variants sit in the screen plane at `inner_circle_radius`.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum CenterShape {
    /// Unfilled ring outline (the original look).
    #[default]
    Ring,
    /// Square outline in the view plane — Maya-style box center handle.
    Box,
}

/// Controls the visual style of the gizmo
#[derive(Debug, Copy, Clone)]
pub struct GizmoVisuals {
    /// Color of the x axis
    pub x_color: Color32,
    /// Color of the y axis
    pub y_color: Color32,
    /// Color of the z axis
    pub z_color: Color32,
    /// Color of the forward axis
    pub s_color: Color32,
    /// Alpha of the gizmo color when inactive
    pub inactive_alpha: f32,
    /// Alpha of the gizmo color when highlighted/active
    pub highlight_alpha: f32,
    /// Color to use for highlighted and active axes. By default, the axis color is used with `highlight_alpha`
    pub highlight_color: Option<Color32>,
    /// Width (thickness) of the gizmo strokes
    pub stroke_width: f32,
    /// Gizmo size in pixels
    pub gizmo_size: f32,
    /// Per-element size multipliers (default `1.0` = unchanged look). These let
    /// the caller tune each handle family independently without touching the
    /// shared `gizmo_size`; each is applied at the single geometry site that
    /// derives the element's length/radius from `gizmo_size` (see
    /// `subgizmo::common`).
    ///
    /// Translate-arrow length multiplier.
    pub arrow_scale: f32,
    /// Planar-handle size + offset multiplier.
    pub plane_scale: f32,
    /// Rotation-ring radius multiplier.
    pub ring_scale: f32,
    /// Scale-handle length multiplier.
    pub scale_box_scale: f32,
    /// Shape of the screen-center handle (Ring or Box).
    pub center_shape: CenterShape,
    /// Color of the screen-center handle (free-translate dot / uniform-scale ring).
    pub center_color: Color32,
    /// Stroke width for the center handle, independent of `stroke_width`.
    pub center_stroke_width: f32,
    /// Size multiplier for the center handle (ring/box) relative to its base
    /// (`inner_circle_radius`). Default `0.7` = 30% smaller than the inner circle.
    pub center_scale: f32,
    /// On-screen side length (in points) of the 2D bounding-box grips. Constant
    /// screen size (independent of the object's scale). Default `10.0`.
    pub bbox_handle_size: f32,
    /// Per-group transparency multipliers — applied on top of the existing
    /// `inactive_alpha`/`highlight_alpha`. Default `1.0` = no change.
    pub translate_alpha: f32,
    pub rotate_alpha: f32,
    pub scale_alpha: f32,
    pub center_alpha: f32,

    // --- Drag-time HUD ("P-HUD") -------------------------------------------
    // These drive the on-drag heads-up display drawn ONLY while a handle is
    // actively dragged (see each subgizmo's active `draw` branch). Defaults are
    // on-but-subtle so the readouts help without dominating the gizmo.
    /// Master switch for the entire drag HUD (readout + dial ticks + guides).
    pub show_drag_hud: bool,
    /// Show the numeric readout (angle / distance / factor) near the cursor.
    pub hud_readout: bool,
    /// Show the rotate-dial tick ring + degree labels.
    pub dial_ticks: bool,
    /// Show the move/scale axis guide lines + trail.
    pub axis_guides: bool,
    /// Colour of HUD tick marks / start-radius lines.
    pub hud_tick_color: Color32,
    /// Colour of HUD guide lines (axis lines, trails, start ghosts).
    pub hud_guide_color: Color32,
    /// Fill alpha applied to the rotate sweep wedge while the HUD is on.
    pub hud_sweep_alpha: f32,
    /// Angular spacing (degrees) between major rotate-dial ticks.
    pub dial_tick_step_deg: f32,
    /// Font size (points) for HUD text (readout + dial numbers).
    pub hud_font_size: f32,
    /// Colour (with alpha) of HUD TEXT — the numeric readout + dial degree labels.
    /// Alpha controls how translucent the overlay text is.
    pub hud_text_color: Color32,
}

impl Default for GizmoVisuals {
    fn default() -> Self {
        Self {
            x_color: Color32::from_rgb(255, 0, 125),
            y_color: Color32::from_rgb(0, 255, 125),
            z_color: Color32::from_rgb(0, 125, 255),
            s_color: Color32::from_rgb(255, 255, 255),
            inactive_alpha: 0.7,
            highlight_alpha: 1.0,
            highlight_color: None,
            stroke_width: 1.5,
            gizmo_size: 75.0,
            // All multipliers default to 1.0 so the out-of-the-box look is
            // identical to the unscaled geometry.
            arrow_scale: 1.0,
            plane_scale: 1.0,
            ring_scale: 1.0,
            scale_box_scale: 1.0,
            // Center handle defaults: turquoise ring, slightly thicker stroke.
            center_shape: CenterShape::Box,
            center_color: Color32::from_rgb(124, 197, 247),
            center_stroke_width: 1.4,
            center_scale: 0.5,
            bbox_handle_size: 10.0,
            // Per-group alphas default to 1.0 (no change to existing look).
            translate_alpha: 1.0,
            rotate_alpha: 1.0,
            scale_alpha: 1.0,
            center_alpha: 1.0,
            // Drag HUD: on by default, but with restrained greys so it reads as
            // an overlay rather than competing with the coloured handles.
            show_drag_hud: true,
            hud_readout: true,
            dial_ticks: true,
            axis_guides: true,
            hud_tick_color: Color32::from_rgb(200, 200, 200),
            hud_guide_color: Color32::from_rgb(110, 110, 110),
            hud_sweep_alpha: 0.25,
            dial_tick_step_deg: 10.0,
            hud_font_size: 10.0,
            hud_text_color: Color32::from_rgba_unmultiplied(235, 235, 235, 220),
        }
    }
}
