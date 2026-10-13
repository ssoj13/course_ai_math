//! egui-gizmo — a hybrid 2D/3D, production-grade transform gizmo for egui.
//!
//! P0 (this increment) is a **fork-vendor** of the upstream `transform-gizmo` 0.9
//! core (see `NOTICE`), with two changes that matter to callers:
//!
//! 1. all math is internalised to **glam f64** (`mint` is gone); and
//! 2. the public API below is **glam f32 only** — `Vec3`/`Quat`/`Mat4` and
//!    `egui` types, nothing else.
//!
//! # Usage
//!
//! ```ignore
//! let mut gizmo = Gizmo::new(GizmoConfig {
//!     view: view_mat,            // glam f32, column-major
//!     projection: proj_mat,      // glam f32, column-major
//!     viewport: ui.clip_rect(),
//!     space: GizmoSpace::ThreeD,
//!     tool: GizmoTool::Move,
//!     orientation: GizmoOrientation::Global,
//!     ..Default::default()
//! });
//!
//! if let Some((result, new_transforms)) = gizmo.interact(ui, &[transform]) {
//!     transform = new_transforms[0];
//!     // result.mode / result.active describe the interaction
//! }
//! ```
//!
//! The host should call [`Gizmo::is_hovered`] and suppress its own camera orbit
//! while it returns `true`.
//!
//! ## Why `view` and `projection` separately (not a single `view_proj`)
//!
//! The gizmo's facing / fade logic reads the **camera basis** from the rows of
//! the view matrix, and its handedness / pixel-scale from the projection matrix.
//! Neither can be recovered from the combined product alone, so the facade takes
//! both — matching how a real host (e.g. a 2D/3D compositor) already has them.

// Some of the vendored core API surface (alternate constructors, query helpers,
// the arcball/plane handles) is not yet wired into the P0 facade. Allow dead
// code crate-wide rather than prune vendored code we will need in P1–P3.
#![allow(dead_code)]

pub mod neutral;
mod config;
mod gizmo;
mod math;
mod shape;
mod subgizmo;

/// NDC -> screen points of a viewport: the gizmo's own viewport transform, for hosts drawing over the same view.
pub use math::viewport_matrix;

use egui::epaint::Vertex;
use egui::{Mesh, PointerButton, Pos2, Rect, Rgba, Sense, Ui, Vec2};
use enumset::{EnumSet, EnumSetType, enum_set};
use glam::{DMat4, DQuat, Mat4, Quat, Vec3};

use config::GizmoMode as InnerMode;

// These public types are identical to their internal counterparts (they only use
// `egui::Color32` + `f32`), so we re-export rather than duplicate them.
pub use config::{
    CenterShape, DEFAULT_SNAP_ANGLE, DEFAULT_SNAP_DISTANCE, DEFAULT_SNAP_SCALE, GizmoOrientation,
    GizmoVisuals,
};

/// A scale / rotation / translation transform in **glam f32** — the public unit
/// the gizmo manipulates. The caller owns the object; the gizmo only reports new
/// transforms (immediate-mode contract).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}

impl Transform {
    /// Convenience constructor mirroring `glam`'s argument order.
    pub fn from_scale_rotation_translation(scale: Vec3, rotation: Quat, translation: Vec3) -> Self {
        Self {
            translation,
            rotation,
            scale,
        }
    }

    /// Widen to the internal glam-f64 transform (boundary conversion).
    fn to_inner(self) -> math::Transform {
        math::Transform {
            scale: self.scale.as_dvec3(),
            rotation: DQuat::from_xyzw(
                self.rotation.x as f64,
                self.rotation.y as f64,
                self.rotation.z as f64,
                self.rotation.w as f64,
            ),
            translation: self.translation.as_dvec3(),
        }
    }

    /// Narrow from the internal glam-f64 transform (boundary conversion).
    fn from_inner(t: math::Transform) -> Self {
        Self {
            translation: t.translation.as_vec3(),
            rotation: Quat::from_xyzw(
                t.rotation.x as f32,
                t.rotation.y as f32,
                t.rotation.z as f32,
                t.rotation.w as f32,
            ),
            scale: t.scale.as_vec3(),
        }
    }
}

/// Which kind of transformation the *active* gizmo (sub)interaction performs.
///
/// This is the OUTCOME channel on [`GizmoResult::mode`], not a tool selector
/// (the tool is [`GizmoTool`]). The `Universal` tool simply reports whichever of
/// `Translate` / `Rotate` / `Scale` the grabbed handle belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum GizmoMode {
    #[default]
    Translate,
    Rotate,
    Scale,
    /// 2D shear (P2b step 3) — an affine that is NOT TRS-representable; the data
    /// rides on [`GizmoResult::kind`], not the returned transforms.
    Shear,
    /// 2D corner-pin (P2b step 4) — a projective homography (NOT TRS); the four
    /// corner positions ride on [`GizmoResult::kind`], not the returned transforms.
    CornerPin,
}

/// Which projection space the gizmo lives in.
///
/// `ThreeD` (perspective/ortho, all handles) vs `TwoD` (ortho; Z-axis handles
/// hidden — the caller passes an orthographic view/projection; pairs with
/// `egui-viewport`). In 2D the gizmo curates its handle set down to what makes
/// sense on a flat plane (see [`GizmoFeatures::to_inner`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GizmoSpace {
    /// Full 3D manipulator: all axes, planes and handle types available.
    #[default]
    ThreeD,
    /// Flat 2D manipulator (caller supplies an orthographic projection): the
    /// Z axis is the screen normal, so Z translate/scale handles and the X/Y
    /// rotation rings are hidden.
    TwoD,
}

/// Which **exclusive** tool is active — the single source of truth for *which*
/// handle family the gizmo shows.
///
/// A gizmo shows exactly ONE tool at a time (Maya/Houdini/Nuke style): the
/// translate / rotate / scale / universal manipulators, the 2D-or-3D transform
/// **box**, the 2D-or-3D **shear** grips, or the 2D-only **corner-pin**. The
/// tool decides the *mode group(s)*; the orthogonal [`GizmoFeatures`]
/// *refinements* (axes / planes / screen / uniform / arcball, per-axis masks)
/// then narrow *within* that group. [`GizmoFeatures::to_inner`] combines the two
/// into the internal handle set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GizmoTool {
    /// Translation arrows / planes / screen-dot.
    #[default]
    Move,
    /// Rotation rings / view-ring / arcball.
    Rotate,
    /// Scale boxes / planes / uniform.
    Scale,
    /// Move ∪ Rotate ∪ Scale (the "universal" manipulator).
    Universal,
    /// The transform **box**: bounding-box scale grips (2D *and* 3D).
    Box,
    /// A full **3D box**: 6 face + 12 edge + 8 corner scale grips on the 3D
    /// bounding box (the volumetric twin of [`Self::Box`]). 3D-meant; in 2D it
    /// collapses to the screen plane.
    Box3D,
    /// **Shear** grips (2D *and* 3D).
    Shear,
    /// Nuke-style **corner-pin**: four independently-draggable corners. 2D only
    /// (a projective homography has no 3D meaning) — see [`Self::supports`].
    CornerPin,
}

impl GizmoTool {
    /// Every tool, in panel / cycle order.
    pub const ALL: [GizmoTool; 8] = [
        Self::Move,
        Self::Rotate,
        Self::Scale,
        Self::Universal,
        Self::Box,
        Self::Box3D,
        Self::Shear,
        Self::CornerPin,
    ];

    /// Whether this tool is usable in `space`. Only `CornerPin` is restricted:
    /// it is a 2D-only projective homography that has no 3D meaning, so it is
    /// disabled in [`GizmoSpace::ThreeD`]. Every other tool works in both spaces.
    pub fn supports(self, space: GizmoSpace) -> bool {
        !(matches!(self, Self::CornerPin) && matches!(space, GizmoSpace::ThreeD))
    }
}

/// The manipulator handle GROUPS a [`GizmoTool::Box3D`] overlays on its bounding box, as a reusable
/// bitfield ([`GizmoFeatures::box_manip`]). Independent of the box's own face/edge/corner grips
/// (always present) and of the central screen-plane picker (the [`GizmoFeatures::screen`] dot). This
/// is what lets ONE Box3D tool be a box-only resize, a classic centre-drag AABB crop, or a full
/// Maya/Houdini combined translate+rotate+scale manipulator — the caller picks the set with
/// `box_manip` (+ `screen` for the central dot).
#[derive(Debug, EnumSetType)]
pub enum GizmoManip {
    /// Per-axis translate arrows (and the translate planes, under the `planes` refinement).
    Translate,
    /// Rotate rings (and the view-ring / arcball, under `screen` / `arcball`).
    Rotate,
    /// Per-axis scale boxes (and the uniform box, under `uniform_scale`).
    Scale,
}

/// Granular *refinements* that narrow the active [`GizmoTool`]'s handle set.
///
/// These are orthogonal to the tool: the tool selects *which* mode group(s) are
/// shown (move / rotate / scale / …), and these flags then narrow *within* a
/// group — which axes/planes, whether the screen-space and uniform/arcball
/// handles appear, and the per-axis masks. They do NOT pick a tool. A handle is
/// shown only when its tool, its handle-type refinement, and (for axis/plane
/// handles) its per-axis flags are all enabled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GizmoFeatures {
    // --- Handle types (gated by the active tool's mode group) ---
    /// Per-axis arrows / boxes / rings.
    pub axes: bool,
    /// Planar (two-axis) quads.
    pub planes: bool,
    /// Screen-space (view) handles: free-translate dot + view-rotation ring.
    pub screen: bool,
    /// Uniform (all-axis) scale box.
    pub uniform_scale: bool,
    /// Arcball (trackball) rotation.
    pub arcball: bool,
    /// For [`GizmoTool::Box3D`]: which combined-manipulator groups (translate / rotate / scale) to
    /// overlay on the bounding box, as a reusable bitfield. Default = all three (a full combined
    /// manip); empty = box grips only. Independent of `screen` (the central picker). Ignored by the
    /// other tools (each fixes its own group).
    pub box_manip: EnumSet<GizmoManip>,
    // --- Per-axis masks (2D forces `axis_z` off) ---
    /// Enable the X axis (and any plane that needs it).
    pub axis_x: bool,
    /// Enable the Y axis (and any plane that needs it).
    pub axis_y: bool,
    /// Enable the Z axis (and any plane that needs it). Masked off in 2D.
    pub axis_z: bool,
}

impl Default for GizmoFeatures {
    fn default() -> Self {
        Self::all()
    }
}

impl GizmoFeatures {
    /// Every refinement enabled (the broadest handle set for whatever tool is
    /// active). The tool, not this, decides *which* mode groups appear.
    pub const fn all() -> Self {
        Self {
            axes: true,
            planes: true,
            screen: true,
            uniform_scale: true,
            arcball: true,
            box_manip: enum_set!(GizmoManip::Translate | GizmoManip::Rotate | GizmoManip::Scale),
            axis_x: true,
            axis_y: true,
            axis_z: true,
        }
    }

    /// Build the internal handle set for the active `tool`, narrowed by these
    /// refinements and curated for the given [`GizmoSpace`].
    ///
    /// 2D curation: the Z axis is the screen normal, so `axis_z` is forced off
    /// (dropping the Z translate/scale arrows and the XZ/YZ planes) and the X/Y
    /// rotation rings collapse to the single screen-plane `RotateZ` ring.
    ///
    /// Tool dispatch:
    /// - `Move` / `Rotate` / `Scale` add only their own group's handles.
    /// - `Universal` adds all three groups, BUT the downstream handle builder
    ///   (`gizmo.rs::add_scale`) de-dups overlapping quads: a planar/uniform SCALE
    ///   handle is suppressed when its translate-plane / view twin is also present.
    ///   So under Universal you get per-axis scale (X/Y/Z) but NOT the planar or
    ///   uniform scale handles — the `planes`/`uniform_scale` refinements are no-ops
    ///   under Universal (they apply to the standalone `Scale` tool).
    /// - `Box` / `Shear` add the single `BoundingBox` / `Shear` handle (both
    ///   spaces — the gizmo-owned `box_model` makes them valid in 3D too).
    /// - `CornerPin` adds the four corner handles, but ONLY in 2D (it is a
    ///   projective homography with no 3D meaning — see [`GizmoTool::supports`]).
    ///
    /// A tool unsupported in `space` (only `CornerPin` in 3D) yields an EMPTY set.
    pub fn to_inner(self, space: GizmoSpace, tool: GizmoTool) -> EnumSet<InnerMode> {
        // Single source of truth for tool/space validity: an unsupported tool
        // (only CornerPin in 3D) shows nothing. Keeps this in lock-step with
        // `GizmoTool::supports` instead of re-deriving the rule inline.
        if !tool.supports(space) {
            return EnumSet::empty();
        }
        let two_d = matches!(space, GizmoSpace::TwoD);

        // Effective per-axis flags. In 2D the Z axis is the screen normal, so
        // it is masked for translate/scale arrows and for any plane needing Z.
        let ax = self.axis_x;
        let ay = self.axis_y;
        let az = self.axis_z && !two_d;

        let mut set = EnumSet::empty();

        match tool {
            GizmoTool::Move => self.add_translate(&mut set, ax, ay, az),
            GizmoTool::Rotate => self.add_rotate(&mut set, ax, ay, az, two_d),
            GizmoTool::Scale => self.add_scale(&mut set, ax, ay, az),
            GizmoTool::Universal => {
                self.add_translate(&mut set, ax, ay, az);
                self.add_rotate(&mut set, ax, ay, az, two_d);
                self.add_scale(&mut set, ax, ay, az);
            }
            // The transform box / shear grips are now valid in BOTH spaces: the
            // gizmo owns the sheared `box_model` frame the handles draw/pick
            // against, so there is no longer a 2D-only guard.
            GizmoTool::Box => set |= InnerMode::BoundingBox,
            // Box3D = the bounding-box grips PLUS a screen-plane CENTRE handle, so the whole box can
            // be dragged from its middle (Maya/Houdini style) without having to grab a face grip.
            // Only the centre `TranslateView` dot is added (not the axis arrows/planes), so the box
            // still reads clean; per-axis resizing stays on the box grips. This makes Box3D a
            // translate+scale manipulator; rotation is intentionally not offered (a rotated box has
            // no meaning for an axis-aligned bounding volume — the common Box3D use).
            // Box3D = the 3D bounding-box grips PLUS a configurable combined manipulator, so ONE
            // tool is a Maya/Houdini-style transform gizmo (translate arrows + rotate rings + scale
            // handles) without tool switching. WHICH groups appear is the reusable `box_manip`
            // bitfield ([`GizmoManip`]); the central screen-plane picker (free-drag dot) is the
            // `screen` refinement. Both independent, so a caller can ask for box-only, box+centre
            // (the classic AABB crop), or the full combined manip.
            GizmoTool::Box3D => {
                set |= InnerMode::BoundingBox3D;
                if self.screen {
                    // Central picker: free-drag the whole box in the screen plane. Added here
                    // (not only via the Translate group) so box+centre is expressible on its own.
                    set |= InnerMode::TranslateView;
                }
                if self.box_manip.contains(GizmoManip::Translate) {
                    self.add_translate(&mut set, ax, ay, az);
                }
                if self.box_manip.contains(GizmoManip::Rotate) {
                    self.add_rotate(&mut set, ax, ay, az, two_d);
                }
                if self.box_manip.contains(GizmoManip::Scale) {
                    self.add_scale(&mut set, ax, ay, az);
                }
            }
            GizmoTool::Shear => set |= InnerMode::Shear,
            // Corner-pin is 2D-only; the early `supports` return already guards 3D.
            GizmoTool::CornerPin => set |= InnerMode::CornerPin,
        }

        set
    }

    /// Add the translate handles (axis arrows, planes, screen-dot) under the
    /// per-axis masks and the `axes`/`planes`/`screen` refinements.
    fn add_translate(self, set: &mut EnumSet<InnerMode>, ax: bool, ay: bool, az: bool) {
        if self.axes {
            if ax {
                *set |= InnerMode::TranslateX;
            }
            if ay {
                *set |= InnerMode::TranslateY;
            }
            if az {
                *set |= InnerMode::TranslateZ;
            }
        }
        if self.planes {
            if ax && ay {
                *set |= InnerMode::TranslateXY;
            }
            if ax && az {
                *set |= InnerMode::TranslateXZ;
            }
            if ay && az {
                *set |= InnerMode::TranslateYZ;
            }
        }
        if self.screen {
            *set |= InnerMode::TranslateView;
        }
    }

    /// Add the rotate handles (rings, view-ring, arcball). In 2D only the
    /// screen-plane `RotateZ` ring is meaningful, so the X/Y rings collapse.
    fn add_rotate(self, set: &mut EnumSet<InnerMode>, ax: bool, ay: bool, az: bool, two_d: bool) {
        if self.axes {
            if two_d {
                *set |= InnerMode::RotateZ;
            } else {
                if ax {
                    *set |= InnerMode::RotateX;
                }
                if ay {
                    *set |= InnerMode::RotateY;
                }
                if az {
                    *set |= InnerMode::RotateZ;
                }
            }
        }
        if self.screen {
            *set |= InnerMode::RotateView;
        }
        if self.arcball {
            *set |= InnerMode::Arcball;
        }
    }

    /// Add the scale handles (axis boxes, planes, uniform box) under the
    /// per-axis masks and the `axes`/`planes`/`uniform_scale` refinements.
    fn add_scale(self, set: &mut EnumSet<InnerMode>, ax: bool, ay: bool, az: bool) {
        if self.axes {
            if ax {
                *set |= InnerMode::ScaleX;
            }
            if ay {
                *set |= InnerMode::ScaleY;
            }
            if az {
                *set |= InnerMode::ScaleZ;
            }
        }
        if self.planes {
            if ax && ay {
                *set |= InnerMode::ScaleXY;
            }
            if ax && az {
                *set |= InnerMode::ScaleXZ;
            }
            if ay && az {
                *set |= InnerMode::ScaleYZ;
            }
        }
        if self.uniform_scale {
            *set |= InnerMode::ScaleUniform;
        }
    }
}

/// Configuration of a gizmo. POD-ish and `Copy` (custom handles will live on
/// [`Gizmo`], not here, so this stays cheap to pass each frame).
#[derive(Clone, Copy, Debug)]
pub struct GizmoConfig {
    /// View matrix (world → view), **glam f32 column-major**.
    pub view: Mat4,
    /// Projection matrix (view → clip), **glam f32 column-major**.
    pub projection: Mat4,
    /// Screen area where the gizmo is displayed.
    pub viewport: Rect,
    /// 3D vs 2D projection space (2D hides the Z-axis handles).
    pub space: GizmoSpace,
    /// Which **exclusive** tool is active (move / rotate / scale / universal /
    /// box / shear / corner-pin). The single source of truth for the handle
    /// family; [`GizmoFeatures`] then refines it.
    pub tool: GizmoTool,
    /// Refinements that narrow the active [`Self::tool`]'s handle set
    /// (axes / planes / screen / uniform / arcball, per-axis masks).
    pub features: GizmoFeatures,
    /// Global (world) or Local (object) axes.
    pub orientation: GizmoOrientation,
    /// Snap to increments while dragging.
    pub snapping: bool,
    /// Rotation snap increment, in RADIANS (applied only while `snapping`).
    /// Default [`DEFAULT_SNAP_ANGLE`] (PI/32 ~= 5.6°).
    pub snap_angle: f32,
    /// Translation snap increment, in the gizmo's world units (only while
    /// `snapping`). Default [`DEFAULT_SNAP_DISTANCE`] (0.1).
    pub snap_distance: f32,
    /// Scale snap increment (only while `snapping`). Default
    /// [`DEFAULT_SNAP_SCALE`] (0.1).
    pub snap_scale: f32,
    /// Visual style (colors, sizes, opacity).
    pub visuals: GizmoVisuals,
    /// Half-extent (local X/Y) of the bounding-box handle set, in the gizmo's
    /// local z=0 plane. Only used when the [`GizmoTool::Box`] tool is active.
    /// `glam::Vec2` (fully qualified to avoid clashing with `egui::Vec2`).
    pub bbox_half_extent: glam::Vec2,
}

impl Default for GizmoConfig {
    fn default() -> Self {
        Self {
            view: Mat4::IDENTITY,
            projection: Mat4::IDENTITY,
            viewport: Rect::NOTHING,
            space: GizmoSpace::default(),
            tool: GizmoTool::default(),
            features: GizmoFeatures::all(),
            orientation: GizmoOrientation::Global,
            snapping: false,
            snap_angle: DEFAULT_SNAP_ANGLE,
            snap_distance: DEFAULT_SNAP_DISTANCE,
            snap_scale: DEFAULT_SNAP_SCALE,
            visuals: GizmoVisuals::default(),
            bbox_half_extent: glam::Vec2::splat(0.5),
        }
    }
}

impl GizmoConfig {
    /// Build the internal (glam f64) config for a given resolved viewport and
    /// pixels-per-point.
    fn to_inner(self, viewport: Rect, pixels_per_point: f32) -> config::GizmoConfig {
        config::GizmoConfig {
            view_matrix: widen(self.view),
            projection_matrix: widen(self.projection),
            viewport,
            modes: self.features.to_inner(self.space, self.tool),
            orientation: self.orientation,
            snapping: self.snapping,
            snap_angle: self.snap_angle,
            snap_distance: self.snap_distance,
            snap_scale: self.snap_scale,
            visuals: self.visuals,
            pixels_per_point,
            bbox_half_extent: self.bbox_half_extent,
            ..Default::default()
        }
    }
}

/// Result of a gizmo interaction this frame.
///
/// Shaped to be additive: the `kind` channel (P2b step 2) reports NON-TRS outcomes
/// (e.g. 2D shear) that cannot ride in the per-target `Vec<Transform>` the gizmo
/// returns. `Eq` is dropped (the new kind carries `f32` payloads which are not
/// `Eq`); the `active` / `mode` fields are unchanged so existing callers (the cube
/// example reads only `new_transforms`) keep compiling.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GizmoResult {
    /// Whether a handle is being actively dragged this frame.
    pub active: bool,
    /// Which kind of transform the active handle performs.
    pub mode: GizmoMode,
    /// The outcome's data channel. `Transform` (the common case) means the result
    /// is fully captured by the returned `Vec<Transform>`; the other variants carry
    /// affine/projective data that has no TRS representation (see [`GizmoResultKind`]).
    pub kind: GizmoResultKind,
}

/// The non-TRS data channel of a [`GizmoResult`] (P2b step 2).
///
/// TRS interactions (translate / rotate / scale / bbox-scale) report `Transform`:
/// the new object transforms ride in the `Vec<Transform>` the gizmo returns, and
/// there is nothing extra to carry here. 2D **shear** is an affine that is NOT
/// TRS-representable, so the target `Transform` is returned unchanged and the shear
/// factor rides here instead — the caller applies it to its own geometry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GizmoResultKind {
    /// A pure TRS interaction; the result is fully described by the returned
    /// transforms. No extra payload.
    Transform,
    /// A 2D shear about the gizmo pivot: `amount` is the shear factor (absolute,
    /// measured from the drag start) along `axis`.
    Shear { axis: ShearAxis, amount: f32 },
    /// A 2D corner-pin (Nuke-style): the four corner positions (frame space, +Y up)
    /// the caller feeds to a homography. Order is SW, SE, NE, NW. NOT TRS — the
    /// returned `Transform`s are unchanged; the caller applies the warp itself.
    /// `glam::Vec2` (fully qualified to avoid clashing with `egui::Vec2`).
    CornerPin { corners: [glam::Vec2; 4] },
}

/// Which axis a 2D [`GizmoResultKind::Shear`] acts along.
///
/// `X` shears the top edge horizontally (x grows with y); `Y` shears the right
/// edge vertically (y grows with x). Mirrors the internal `ShearAxis2`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShearAxis {
    /// Horizontal shear: `x' = x + amount * y`.
    X,
    /// Vertical shear: `y' = y + amount * x`.
    Y,
}

/// A persistent 3D transform gizmo.
#[derive(Clone, Debug, Default)]
pub struct Gizmo {
    inner: gizmo::Gizmo,
    config: GizmoConfig,
    hovered: bool,
}

impl Gizmo {
    /// Create a new gizmo from the given configuration.
    pub fn new(config: GizmoConfig) -> Self {
        let viewport = if config.viewport.is_finite() {
            config.viewport
        } else {
            Rect::NOTHING
        };
        let inner = gizmo::Gizmo::new(config.to_inner(viewport, 1.0));
        Self {
            inner,
            config,
            hovered: false,
        }
    }

    /// Replace the configuration (e.g. when the camera or viewport changes).
    pub fn update_config(&mut self, config: GizmoConfig) {
        self.config = config;
    }

    /// Drive the gizmo for `targets` (their current transforms) and paint it.
    ///
    /// Returns `Some((result, new_transforms))` when a handle is being dragged
    /// this frame; `new_transforms` is in the same order as `targets`
    /// (multi-select preserved). Returns `None` otherwise.
    pub fn interact(
        &mut self,
        ui: &Ui,
        targets: &[Transform],
    ) -> Option<(GizmoResult, Vec<Transform>)> {
        let cursor_pos = ui
            .input(|input| input.pointer.hover_pos())
            .unwrap_or_default();

        // Resolve viewport: fall back to the clip rect when unset/non-finite.
        let mut viewport = self.config.viewport;
        if !viewport.is_finite() {
            viewport = ui.clip_rect();
        }
        let pixels_per_point = ui.ctx().pixels_per_point();

        self.inner
            .update_config(self.config.to_inner(viewport, pixels_per_point));

        // The gizmo only ACQUIRES input while the cursor is inside its `viewport`. This
        // is the systematic fix for the gizmo stealing clicks from sibling widgets that
        // share the same `Ui`/layer (e.g. a control row above the viewport): outside the
        // viewport it allocates NO interaction region, so egui routes the click to those
        // widgets normally. An in-progress drag still continues via `dragging`/`mmb_down`
        // below (button-state, not cursor-gated), so leaving the viewport mid-drag is fine.
        let cursor_in_vp = viewport.contains(cursor_pos);
        // A 1px probe under the cursor, used ONLY to read hover (occlusion by sibling widgets
        // like a toolbar), allocated only while the cursor is over the viewport. It senses
        // `hover()`, NOT `click_and_drag()`: a drag-sensing probe here becomes egui's drag
        // target (same-layer widgets resolve by registration order, and the probe registers
        // after the host viewport), which STOLE the drag from the host's own viewport response
        // — an idle gizmo then killed the host's camera orbit/pan/dolly even with no handle
        // under the cursor. A hover-only probe cannot be `hits.drag`/`hits.click` (see egui
        // `hit_test`), so the host keeps its drag. The gizmo drives its OWN handles from raw
        // pointer state (`drag_started`/`dragging` below), never from this probe, so sensing
        // hover only changes nothing for handle grabs.
        //
        // The probe is read with `contains_pointer()`, NOT `hovered()`, and the difference is
        // load-bearing. egui documents it: `hovered()` is "false whenever some other widget is
        // being dragged", while `contains_pointer()` is "true if the pointer is contained by the
        // response rect, and no other widget is covering it" - and stays true during someone
        // else's drag.
        //
        // The moment the primary button goes down, the host's own viewport widget (or any
        // drag-sensing ancestor) becomes egui's drag target, so `hovered()` flips to false on
        // exactly the frame `drag_started` fires. The handle highlighted while the cursor merely
        // rested on it and then went dark the instant it was clicked, and no axis could ever be
        // picked: the gizmo was told "the cursor is not on you" precisely when it had to decide
        // which subgizmo to grab. Every host with a drag-sensing viewport hit this, which is why
        // only the button-state paths (RMB / MMB shortcuts) appeared to work.
        //
        // `contains_pointer()` is what this probe always meant: occlusion, not interaction
        // ownership. An interactive widget genuinely on top (a toolbar) still covers it and
        // correctly reports false.
        let hovered = cursor_in_vp
            && ui
                .interact(
                    Rect::from_center_size(cursor_pos, Vec2::splat(1.0)),
                    ui.id().with("egui_gizmo_interaction"),
                    Sense::hover(),
                )
                .contains_pointer();

        let inner_targets: Vec<math::Transform> =
            targets.iter().copied().map(Transform::to_inner).collect();

        let result = self.inner.update(
            gizmo::GizmoInteraction {
                cursor_pos: (cursor_pos.x, cursor_pos.y),
                hovered,
                // A drag/grab can only BEGIN inside the viewport (cursor_in_vp); it
                // CONTINUES on raw button-state regardless of where the pointer goes.
                drag_started: cursor_in_vp
                    && ui.input(|input| input.pointer.button_pressed(PointerButton::Primary)),
                dragging: ui.input(|input| input.pointer.button_down(PointerButton::Primary)),
                // Maya middle-mouse translate: plain MMB = free screen move,
                // Shift+MMB = axis-constrained move. These are additive; the
                // Primary fields above are untouched.
                mmb_pressed: cursor_in_vp
                    && ui.input(|input| input.pointer.button_pressed(PointerButton::Middle)),
                mmb_down: ui.input(|input| input.pointer.button_down(PointerButton::Middle)),
                shift: ui.input(|input| input.modifiers.shift),
                // Ctrl = aspect-lock for the 2D bounding-box scale handles.
                ctrl: ui.input(|input| input.modifiers.ctrl),
            },
            &inner_targets,
        );

        // Ease each subgizmo's highlight in/out through egui's frame clock. This
        // crate is otherwise egui+glam-only, so the animation clock lives HERE in
        // the facade (which has the egui `Context`), not in the glam core. Called
        // after `update` (which set the focused bools) and before `draw` (which
        // reads the eased factor): 0 == inactive palette/alpha, 1 == highlighted.
        const GIZMO_FOCUS_FADE_SECS: f32 = 0.12;
        let ctx = ui.ctx();
        self.inner.set_focus_factors(|id, focused| {
            ctx.animate_bool_with_time(
                egui::Id::new(("egui_gizmo_focus", id)),
                focused,
                GIZMO_FOCUS_FADE_SECS,
            )
        });

        let draw_data = self.inner.draw();
        paint(ui, viewport, draw_data);

        self.hovered = self.inner.is_focused();

        result.map(|(inner_result, inner_transforms)| {
            let mode = mode_of(&inner_result);
            let kind = kind_of(&inner_result);
            // Preserve per-target order; `.into_iter()` avoids index panics.
            let transforms = inner_transforms
                .into_iter()
                .map(Transform::from_inner)
                .collect();
            (
                GizmoResult {
                    active: true,
                    mode,
                    kind,
                },
                transforms,
            )
        })
    }

    /// True when a handle is hovered or being dragged — the host should suppress
    /// its own camera orbit while this is true.
    pub fn is_hovered(&self) -> bool {
        self.hovered
    }

    /// Force `is_hovered()` false. `hovered` is written ONLY inside `interact()` (at the
    /// bottom, from `self.inner.is_focused()`), so a host that skips calling `interact()`
    /// this frame (no active tool / no target / nothing selected) would otherwise leave
    /// last frame's hover value latched — `is_hovered()` stuck `true` forever until
    /// `interact()` happens to run again. Call this on every path that returns without
    /// calling `interact()`.
    pub fn clear_hover(&mut self) {
        self.hovered = false;
    }

    /// The current configuration.
    pub fn config(&self) -> &GizmoConfig {
        &self.config
    }

    /// The gizmo-owned 2D shear factors `(kx, ky)`, persisted across frames.
    ///
    /// `kx` shears X over Y (top-edge grip), `ky` shears Y over X (right-edge
    /// grip). Zero until a shear grip is dragged; reset when the 2D tool / mode
    /// changes. The caller composes this into its own geometry (the gizmo cannot
    /// carry a non-TRS affine in the returned `Transform`s).
    pub fn shear_2d(&self) -> glam::Vec2 {
        self.inner.shear_2d()
    }

    /// Seed the gizmo-owned 2D shear from the host's stored value; see
    /// [`crate::gizmo::Gizmo::set_shear_2d`].
    ///
    /// A host that persists shear on its own objects must call this before `interact` on every
    /// frame, for the same reason it re-supplies the `Transform`s: the shear cannot travel on a TRS
    /// `Transform`, and a mode/tool change resets it. Without it, re-selecting a sheared object
    /// shows an un-sheared box and the next grab jumps by the stored amount.
    pub fn set_shear_2d(&mut self, shear: glam::Vec2) {
        self.inner.set_shear_2d(shear);
    }
}

/// Widen an f32 glam `Mat4` (column-major) to an f64 `DMat4` (column-major).
///
/// CRITICAL: this widens **element-wise, preserving columns** — there is NO
/// transpose. The upstream stored row-major `mint` matrices and relied on
/// `DMat4::from(RowMatrix4)` transposing; our public matrix is already
/// column-major, so transposing here would double-flip it.
fn widen(m: Mat4) -> DMat4 {
    DMat4::from_cols(
        m.x_axis.as_dvec4(),
        m.y_axis.as_dvec4(),
        m.z_axis.as_dvec4(),
        m.w_axis.as_dvec4(),
    )
}

/// Map an internal result variant to the public mode.
fn mode_of(result: &gizmo::GizmoResult) -> GizmoMode {
    match result {
        gizmo::GizmoResult::Translation { .. } => GizmoMode::Translate,
        gizmo::GizmoResult::Rotation { .. } | gizmo::GizmoResult::Arcball { .. } => {
            GizmoMode::Rotate
        }
        gizmo::GizmoResult::Scale { .. } | gizmo::GizmoResult::BoxScale { .. } => GizmoMode::Scale,
        gizmo::GizmoResult::Shear { .. } => GizmoMode::Shear,
        gizmo::GizmoResult::CornerPin { .. } => GizmoMode::CornerPin,
    }
}

/// Map an internal result variant to the public, non-TRS data channel.
///
/// Every TRS interaction collapses to [`GizmoResultKind::Transform`] (their result
/// is fully carried by the returned transforms). Only `Shear` carries extra data:
/// the internal `ShearAxis2` + f64 amount narrow to the public `ShearAxis` + f32.
fn kind_of(result: &gizmo::GizmoResult) -> GizmoResultKind {
    match result {
        gizmo::GizmoResult::Shear { axis, amount } => GizmoResultKind::Shear {
            axis: match axis {
                crate::subgizmo::shear::ShearAxis2::X => ShearAxis::X,
                crate::subgizmo::shear::ShearAxis2::Y => ShearAxis::Y,
            },
            amount: *amount as f32,
        },
        // Corner-pin: narrow the four internal f64 world-XY corners to public f32
        // frame-space `glam::Vec2` (the homography channel; +Y up, SW/SE/NE/NW).
        gizmo::GizmoResult::CornerPin { corners } => GizmoResultKind::CornerPin {
            corners: corners.map(|c| glam::Vec2::new(c.x as f32, c.y as f32)),
        },
        _ => GizmoResultKind::Transform,
    }
}

/// Paint the gizmo mesh. All `epaint`/`Mesh`/`Vertex` use is confined here and in
/// `shape.rs` (pitfall #3 — egui-internal tessellation lives in one place).
///
/// The mesh is untextured: `TextureId::default()` with white-UV `Pos2::default()`
/// (the upstream tessellated with feathering, so no extra AA is needed here).
fn paint(ui: &Ui, viewport: Rect, draw: gizmo::GizmoDrawData) {
    // A frame may carry ONLY labels (e.g. a HUD readout with no extra mesh), so
    // gate on both channels being empty — not just the mesh — or the text would
    // be dropped on label-only frames.
    if (draw.vertices.is_empty() || draw.indices.is_empty()) && draw.labels.is_empty() {
        return;
    }

    let labels = draw.labels;

    let mesh = Mesh {
        indices: draw.indices,
        vertices: draw
            .vertices
            .into_iter()
            .zip(draw.colors)
            .map(|(pos, [r, g, b, a])| Vertex {
                pos: pos.into(),
                uv: Pos2::default(),
                color: Rgba::from_rgba_premultiplied(r, g, b, a).into(),
            })
            .collect(),
        ..Default::default()
    };

    // One painter for both channels (mesh first, text on top).
    let painter = egui::Painter::new(ui.ctx().clone(), ui.layer_id(), viewport);
    painter.add(mesh);
    for l in labels {
        let [r, g, b, a] = l.color;
        // Reuse the EXACT premultiplied-linear conversion the mesh path uses
        // (the label colours are linear RGBA, same as `colors`); using a
        // straight `Color32` here would mis-tint the text in sRGB space.
        let color = Rgba::from_rgba_premultiplied(r, g, b, a);
        painter.text(
            l.pos.into(),
            l.anchor,
            l.text,
            egui::FontId::proportional(l.size),
            color.into(),
        );
    }
}

#[cfg(test)]
mod tests {
    //! Locks the `GizmoTool` -> inner handle-set mapping so a refactor can't
    //! silently drop or leak a handle family (this table would have caught the
    //! Universal de-dup / CornerPin-in-3D nuances during review).
    use super::*;

    /// Emitted inner modes for a tool with ALL refinements on.
    fn modes(space: GizmoSpace, tool: GizmoTool) -> enumset::EnumSet<InnerMode> {
        GizmoFeatures::all().to_inner(space, tool)
    }

    #[test]
    fn tool_space_mapping() {
        use GizmoSpace::{ThreeD, TwoD};

        // CornerPin: 2D only, empty (unsupported) in 3D.
        assert!(modes(TwoD, GizmoTool::CornerPin).contains(InnerMode::CornerPin));
        assert!(modes(ThreeD, GizmoTool::CornerPin).is_empty());
        assert!(!GizmoTool::CornerPin.supports(ThreeD));

        // Box & Shear: BOTH spaces.
        for s in [TwoD, ThreeD] {
            assert!(modes(s, GizmoTool::Box).contains(InnerMode::BoundingBox));
            assert!(modes(s, GizmoTool::Shear).contains(InnerMode::Shear));
        }

        // Family isolation: Move emits no rotate/scale/box handle.
        let mv = modes(ThreeD, GizmoTool::Move);
        assert!(mv.contains(InnerMode::TranslateX));
        assert!(!mv.contains(InnerMode::RotateX));
        assert!(!mv.contains(InnerMode::ScaleX));
        assert!(!mv.contains(InnerMode::BoundingBox));

        // 2D masks Z: no TranslateZ; rotation collapses to RotateZ only.
        assert!(!modes(TwoD, GizmoTool::Move).contains(InnerMode::TranslateZ));
        let rot2 = modes(TwoD, GizmoTool::Rotate);
        assert!(rot2.contains(InnerMode::RotateZ));
        assert!(!rot2.contains(InnerMode::RotateX));
        assert!(!rot2.contains(InnerMode::RotateY));

        // Universal carries all three families' per-axis handles.
        let u = modes(ThreeD, GizmoTool::Universal);
        assert!(u.contains(InnerMode::TranslateX));
        assert!(u.contains(InnerMode::RotateX));
        assert!(u.contains(InnerMode::ScaleX));

        // Box3D (all refinements) = the full combined manip: box grips + centre dot + T/R/S groups.
        let b3 = modes(ThreeD, GizmoTool::Box3D);
        assert!(b3.contains(InnerMode::BoundingBox3D));
        assert!(b3.contains(InnerMode::TranslateView));
        assert!(b3.contains(InnerMode::TranslateX));
        assert!(b3.contains(InnerMode::RotateX));
        assert!(b3.contains(InnerMode::ScaleX));
        // With an empty `box_manip` and no `screen`, Box3D is the bare bounding box (no T/R/S dot).
        let bare = GizmoFeatures {
            box_manip: enumset::EnumSet::empty(),
            screen: false,
            ..GizmoFeatures::all()
        }
        .to_inner(ThreeD, GizmoTool::Box3D);
        assert!(bare.contains(InnerMode::BoundingBox3D));
        assert!(!bare.contains(InnerMode::TranslateView));
        assert!(!bare.contains(InnerMode::TranslateX));
        assert!(!bare.contains(InnerMode::RotateX));
        assert!(!bare.contains(InnerMode::ScaleX));
    }

    /// `hovered` is written only inside `interact()`; a host that skips calling it (no
    /// active tool this frame, say) must still be able to force it back to false rather
    /// than read a stale true from whenever `interact()` last ran.
    #[test]
    fn clear_hover_resets_a_stuck_true() {
        // Simulate a hover latched by a prior interact().
        let mut gizmo = Gizmo {
            hovered: true,
            ..Gizmo::default()
        };
        assert!(gizmo.is_hovered());
        gizmo.clear_hover();
        assert!(!gizmo.is_hovered());
    }
}
