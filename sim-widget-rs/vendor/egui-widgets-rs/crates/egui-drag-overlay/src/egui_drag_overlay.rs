//! Cross-panel drag-and-drop with a foreground "ghost" overlay.
//!
//! egui clips each panel to its own rect, so a normal drag vanishes the moment
//! the cursor leaves the source panel. This crate keeps the dragged payload in
//! egui's ephemeral data and paints a ghost on the **foreground** layer (above
//! every panel) so the drag stays visible all the way to the drop target.
//!
//! Flow:
//! 1. Source panel: on drag start, call [`DragGhost::begin`] with a payload `T`.
//! 2. Drop target(s): each frame, read [`DragGhost::current`]; when the cursor is
//!    over a valid drop, publish a snapped preview with [`DragGhost::set_snap`].
//! 3. Once per frame, AFTER all panels, call
//!    [`DragGhost::paint_and_take_drop`] — it paints the ghost and, on primary
//!    release, returns the payload (the "drop") and clears the drag.
//!
//! Generic over the payload `T: Clone + Send + Sync + 'static`. No app types.

use egui::{Color32, Context, Id, LayerId, Order, Rect, Stroke, Vec2, epaint::StrokeKind};
use egui_widgets_config::state;

/// A snapped drop-preview rect a drop target publishes so the ghost is accurate.
/// `invalid` tints it red (e.g. an illegal drop, like a cyclic reference).
#[derive(Clone, Copy, Debug)]
pub struct DragSnap {
    pub rect: Rect,
    pub invalid: bool,
}

/// Handle to one cross-panel drag channel, keyed by a stable caller [`Id`].
/// Cheap to construct; carries no state itself (everything lives in egui data).
#[derive(Clone, Copy)]
pub struct DragGhost {
    id: Id,
}

impl DragGhost {
    /// New channel keyed by `id`. Use the same `id` in the source, the drop
    /// target, and the per-frame paint call.
    pub fn new(id: impl Into<Id>) -> Self {
        Self { id: id.into() }
    }

    #[inline]
    fn snap_id(&self) -> Id {
        self.id.with("__snap")
    }

    /// Begin a drag carrying `payload`.
    pub fn begin<T: Clone + Send + Sync + 'static>(&self, ctx: &Context, payload: T) {
        state::set(ctx, self.id, payload);
    }

    /// The payload of the in-flight drag, if any (and of type `T`).
    pub fn current<T: Clone + Send + Sync + 'static>(&self, ctx: &Context) -> Option<T> {
        state::get::<T>(ctx, self.id)
    }

    /// Cancel the drag without a drop.
    pub fn cancel<T: Clone + Send + Sync + 'static>(&self, ctx: &Context) {
        state::clear::<T>(ctx, self.id);
        state::clear::<DragSnap>(ctx, self.snap_id());
    }

    /// A drop target publishes the snapped preview rect for THIS frame. It is
    /// consumed by the next [`Self::paint_and_take_drop`] (and re-published each
    /// frame the cursor stays over the target).
    pub fn set_snap(&self, ctx: &Context, snap: DragSnap) {
        state::set(ctx, self.snap_id(), snap);
    }

    /// Paint the ghost and, on primary-button release, return the dropped payload
    /// and clear the drag. Call once per frame after all panels have laid out
    /// (so drop targets had a chance to `set_snap`).
    ///
    /// `default_size` is the ghost box drawn at the cursor when no target has
    /// published a snapped rect. Returns `Some(payload)` only on the release
    /// frame; `None` otherwise.
    pub fn paint_and_take_drop<T: Clone + Send + Sync + 'static>(
        &self,
        ctx: &Context,
        default_size: Vec2,
    ) -> Option<T> {
        let Some(payload) = self.current::<T>(ctx) else {
            // No active drag — make sure no stale snap lingers.
            state::clear::<DragSnap>(ctx, self.snap_id());
            return None;
        };

        let released = ctx.input(|i| i.pointer.primary_released());
        let snap = state::get::<DragSnap>(ctx, self.snap_id());

        let mut painter =
            ctx.layer_painter(LayerId::new(Order::Foreground, self.id.with("__ghost")));
        painter.set_clip_rect(ctx.viewport_rect());

        if let Some(snap) = snap {
            let color = if snap.invalid {
                Color32::from_rgba_unmultiplied(255, 80, 80, 200)
            } else {
                Color32::from_rgba_unmultiplied(100, 220, 255, 180)
            };
            painter.rect_stroke(snap.rect, 4.0, Stroke::new(2.0, color), StrokeKind::Middle);
        } else if let Some(pos) =
            ctx.input(|i| i.pointer.hover_pos().or_else(|| i.pointer.latest_pos()))
        {
            let rect = Rect::from_center_size(pos, default_size);
            let color = Color32::from_rgba_unmultiplied(100, 220, 255, 180);
            painter.rect_stroke(rect, 4.0, Stroke::new(2.0, color), StrokeKind::Middle);
        }

        if released {
            state::clear::<T>(ctx, self.id);
            state::clear::<DragSnap>(ctx, self.snap_id());
            Some(payload)
        } else {
            None
        }
    }
}
