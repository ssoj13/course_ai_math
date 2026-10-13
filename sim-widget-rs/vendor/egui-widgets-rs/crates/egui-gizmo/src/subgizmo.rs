// Vendored and adapted from `transform-gizmo` 0.9.0.
// Copyright (c) Urho Laukkarinen. Licensed under MIT OR Apache-2.0. See NOTICE.
// Adaptations (glam-f64 internalisation, egui-only deps) are part of egui-gizmo
// and licensed BSD-3-Clause.
//
//! Subgizmo dispatch. P1: the closed `enum_dispatch` enum was replaced by an open
//! `Box<dyn SubGizmoControl>` so new handle kinds (shear, corner-pin, custom) can
//! be added without editing a fixed variant set. The `ahash` dependency is dropped
//! in favour of the std `DefaultHasher` for the per-subgizmo id.

use std::any::Any;
use std::collections::hash_map::DefaultHasher;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};
use std::ops::Deref;

use crate::config::{GizmoDirection, PreparedGizmoConfig};
use crate::gizmo::{GizmoDrawData, GizmoResult, Ray};

pub(crate) use arcball::ArcballSubGizmo;
pub(crate) use bbox::BboxSubGizmo;
pub(crate) use bbox3d::Bbox3DSubGizmo;
pub(crate) use corner_pin::CornerPinSubGizmo;
pub(crate) use rotation::RotationSubGizmo;
pub(crate) use scale::ScaleSubGizmo;
pub(crate) use shear::ShearSubGizmo;
pub(crate) use translation::TranslationSubGizmo;

pub(crate) mod arcball;
pub(crate) mod bbox;
pub(crate) mod bbox3d;
pub(crate) mod common;
pub(crate) mod corner_pin;
pub(crate) mod rotation;
pub(crate) mod scale;
pub(crate) mod shear;
pub(crate) mod translation;

/// Open handle dispatch (P1): a boxed `dyn SubGizmoControl`. Replaces the closed
/// `enum_dispatch` enum so shear / corner-pin / custom handle kinds can be added
/// without a fixed variant set. `Vec<SubGizmo>` and `&mut SubGizmo` keep working
/// via `Box`'s auto-deref to `dyn SubGizmoControl`; construction stays `.into()`
/// thanks to the `From` impl below. `Clone`/`Debug` are preserved (the owning
/// `Gizmo` derives both) via the `clone_box` shim + the `Debug` supertrait.
pub(crate) type SubGizmo = Box<dyn SubGizmoControl>;

pub(crate) trait SubGizmoControl: Debug {
    /// Unique identifier for this subgizmo.
    fn id(&self) -> u64;
    /// Update the configuration used by the gizmo.
    fn update_config(&mut self, config: PreparedGizmoConfig);
    /// Sets whether this subgizmo is currently focused.
    fn set_focused(&mut self, focused: bool);
    /// Set the animated focus factor (0 = inactive, 1 = fully highlighted). Eased
    /// by the egui facade toward `is_focused()` so the highlight fades in/out
    /// instead of hard-switching.
    fn set_focus_factor(&mut self, factor: f32);
    /// The current eased focus factor (0..=1); the draw path lerps highlight vs
    /// inactive colour/alpha by it.
    fn focus_factor(&self) -> f32;
    /// Sets whether this subgizmo is currently active.
    fn set_active(&mut self, active: bool);
    /// Returns true if this subgizmo is currently focused.
    fn is_focused(&self) -> bool;
    /// Returns true if this subgizmo is currently active.
    fn is_active(&self) -> bool;
    /// Pick the subgizmo based on pointer ray. If it is close enough to
    /// the mouse pointer, distance from camera to the subgizmo is returned.
    fn pick(&mut self, ray: Ray) -> Option<f64>;
    /// Update the subgizmo based on pointer ray and interaction.
    fn update(&mut self, ray: Ray) -> Option<GizmoResult>;
    /// Draw the subgizmo.
    fn draw(&self) -> GizmoDrawData;
    fn pick_preview(&self, ray: Ray) -> bool;
    /// Clone into a box so `Box<dyn SubGizmoControl>` can be `Clone` (trait objects
    /// aren't `Clone` without this shim; the owning `Gizmo` derives `Clone`).
    fn clone_box(&self) -> Box<dyn SubGizmoControl>;
    /// World translate axis/handle this subgizmo drives for the Maya MMB gesture
    /// (`View` = free screen move, `X`/`Y`/`Z` = axis arrow), or `None` for any
    /// handle MMB does not reuse (rotation, scale, planes…). Used by the gizmo to
    /// FIND the handle to force-activate.
    fn mmb_translate_dir(&self) -> Option<GizmoDirection>;
    /// Seed this handle's drag-start state for a FORCE-activation (Maya MMB), using
    /// the same point projection its `update` uses so the first frame has no jump.
    fn seed_drag_start(&mut self, ray: Ray);
}

pub(crate) trait SubGizmoKind: 'static {
    type Params: Debug + Copy + Hash;
    type State: Debug + Copy + Clone + Send + Sync + Default + 'static;
    type PickPreview: Picked + 'static;

    fn pick(subgizmo: &mut SubGizmoConfig<Self>, ray: Ray) -> Option<f64>
    where
        Self: Sized;
    fn update(subgizmo: &mut SubGizmoConfig<Self>, ray: Ray) -> Option<GizmoResult>
    where
        Self: Sized;
    fn draw(subgizmo: &SubGizmoConfig<Self>) -> GizmoDrawData
    where
        Self: Sized;

    fn pick_preview(subgizmo: &SubGizmoConfig<Self>, ray: Ray) -> Self::PickPreview
    where
        Self: Sized;

    /// MMB translate direction for this kind (see
    /// [`SubGizmoControl::mmb_translate_dir`]). Defaults to `None`; only the
    /// translation kind overrides it.
    fn mmb_translate_dir(_subgizmo: &SubGizmoConfig<Self>) -> Option<GizmoDirection>
    where
        Self: Sized,
    {
        None
    }

    /// Seed drag-start state for a force-activation (see
    /// [`SubGizmoControl::seed_drag_start`]). Defaults to `pick` (correct for
    /// handles whose `update` reuses the pick point); the translation kind
    /// overrides it to seed from the axis/plane projection instead.
    fn seed_drag_start(subgizmo: &mut SubGizmoConfig<Self>, ray: Ray)
    where
        Self: Sized,
    {
        Self::pick(subgizmo, ray);
    }
}

pub(crate) trait Picked {
    fn picked(&self) -> bool;
}

#[derive(Clone, Debug)]
pub(crate) struct SubGizmoConfig<T: SubGizmoKind> {
    id: u64,
    /// Additional parameters depending on the subgizmo kind.
    params: T::Params,

    /// Configuration of the full gizmo
    pub(crate) config: PreparedGizmoConfig,
    /// Whether this subgizmo is focused this frame
    pub(crate) focused: bool,
    /// Eased focus factor (0 = inactive, 1 = highlighted), driven by the egui
    /// facade toward `focused` so the highlight fades rather than snaps.
    pub(crate) focus_factor: f32,
    /// Whether this subgizmo is active this frame
    pub(crate) active: bool,
    /// Implementation-specific state of the subgizmo.
    pub(crate) state: T::State,
}

impl<T: SubGizmoKind> Deref for SubGizmoConfig<T> {
    type Target = T::Params;

    fn deref(&self) -> &Self::Target {
        &self.params
    }
}

impl<T> SubGizmoConfig<T>
where
    T: SubGizmoKind,
{
    pub(crate) fn new(config: PreparedGizmoConfig, params: T::Params) -> Self {
        // `DefaultHasher::new()` is deterministic (fixed SipHasher keys), so the
        // id is stable across frames within a run — which is all the gizmo needs
        // to track the active subgizmo. This replaces the dropped `ahash`.
        let mut hasher = DefaultHasher::new();
        params.type_id().hash(&mut hasher);
        params.hash(&mut hasher);
        let id = hasher.finish();

        Self {
            id,
            params,
            config,
            focused: false,
            focus_factor: 0.0,
            active: false,
            state: Default::default(),
        }
    }
}

impl<T> SubGizmoControl for SubGizmoConfig<T>
where
    // `Clone`/`Debug` come from the supertrait + `clone_box`; the derive on
    // `SubGizmoConfig<T>` bounds them on `T`. Every concrete kind already is both.
    T: SubGizmoKind + Clone + Debug,
{
    fn id(&self) -> u64 {
        self.id
    }
    fn update_config(&mut self, config: PreparedGizmoConfig) {
        self.config = config;
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn set_focus_factor(&mut self, factor: f32) {
        self.focus_factor = factor;
    }

    fn focus_factor(&self) -> f32 {
        self.focus_factor
    }

    fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    fn is_focused(&self) -> bool {
        self.focused
    }

    fn is_active(&self) -> bool {
        self.active
    }

    fn pick(&mut self, ray: Ray) -> Option<f64> {
        T::pick(self, ray)
    }

    fn update(&mut self, ray: Ray) -> Option<GizmoResult> {
        T::update(self, ray)
    }

    fn draw(&self) -> GizmoDrawData {
        T::draw(self)
    }

    fn pick_preview(&self, ray: Ray) -> bool {
        T::pick_preview(self, ray).picked()
    }

    fn clone_box(&self) -> Box<dyn SubGizmoControl> {
        Box::new(self.clone())
    }

    fn mmb_translate_dir(&self) -> Option<GizmoDirection> {
        T::mmb_translate_dir(self)
    }

    fn seed_drag_start(&mut self, ray: Ray) {
        T::seed_drag_start(self, ray);
    }
}

/// `.into()` at the construction sites builds the boxed handle (replaces the
/// `enum_dispatch`-generated `From<…> for SubGizmo`).
impl<T: SubGizmoKind + Clone + Debug> From<SubGizmoConfig<T>> for Box<dyn SubGizmoControl> {
    fn from(subgizmo: SubGizmoConfig<T>) -> Self {
        Box::new(subgizmo)
    }
}

/// Lets the owning `Gizmo` keep `#[derive(Clone)]` over its `Vec<SubGizmo>`.
impl Clone for Box<dyn SubGizmoControl> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}
