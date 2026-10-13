//! GPU **object-id picking** for wgpu — geometry-agnostic.
//!
//! # What
//!
//! Pixel-exact picking by rendering each object's integer id into an `R32Uint`
//! "object-id" render target, then reading back the single pixel under the cursor. This is
//! the robust alternative to CPU ray/shape hit-testing: it picks against the *actually
//! rasterized* pixels (SDF edges, overlaps, occlusion, any projection incl. reversed-Z
//! perspective) with zero geometry math on the CPU.
//!
//! # Why a crate (not per-app)
//!
//! The mechanism is identical for every wgpu renderer — only the geometry differs. This crate
//! owns the reusable parts (id texture, async readback, id<->handle map); the consumer owns its
//! shaders and simply outputs a `u32` id to [`IdTarget::FORMAT`] and calls
//! [`IdPicker::request`]/[`IdPicker::submit`]/[`IdPicker::poll`] each frame.
//!
//! # How (per frame)
//!
//! ```text
//! on cursor move/click:  picker.request(px, py)
//! build render pass:     add id_target.view() as a second color attachment (blend = None),
//!                        clear to ID_NONE; the fragment shader writes the instance's id
//! after encoding pass:   picker.submit(device, &mut encoder, id_target.texture(), size)
//! after queue.submit:    let id = picker.poll(device);   // Some(id) with 1-frame latency
//! map id -> your handle: registry.get(id)
//! ```
//!
//! The readback is one row copy (256-byte aligned) + a blocking map; it carries the usual
//! **1-frame latency** (the id read belongs to the previously submitted frame), which is
//! imperceptible for hover/selection.

use std::collections::HashMap;

/// Background / "nothing picked" id. Never allocate this to an object.
pub const ID_NONE: u32 = 0;

/// Optional high bit a consumer may OR into the id texture to mark *selected* objects, so a
/// single id pass can carry both identity and selection state. Strip it with [`canonical_id`]
/// before looking an id up in an [`IdRegistry`].
pub const SELECTED_BIT: u32 = 0x8000_0000;

/// Strip [`SELECTED_BIT`] so an id read from the texture matches the value handed out by
/// [`IdRegistry::alloc`] (which never sets the bit).
#[inline]
pub fn canonical_id(id: u32) -> u32 {
    id & !SELECTED_BIT
}

/// 256-byte-aligned bytes-per-row for a `width`-pixel `R32Uint` row (wgpu copy requirement).
#[inline]
fn aligned_bpr(width: u32) -> u32 {
    (width * 4 + 255) & !255
}

/// The `R32Uint` object-id render target, sized to the viewport.
///
/// Attach [`Self::view`] as a **second** color attachment on the pass that draws pickable
/// geometry, cleared to [`ID_NONE`]. The matching pipeline target uses
/// [`Self::color_target_state`] (no blend — ids must not be averaged). The texture carries
/// `COPY_SRC` so [`IdPicker`] can read a row back.
pub struct IdTarget {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
}

impl IdTarget {
    /// Texture format for the id target and the pipeline's id color-target.
    pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;

    /// Create an id target sized `width` x `height` (clamped to >= 1).
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        let texture = Self::make_texture(device, width, height);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self { texture, view, width, height }
    }

    /// Reallocate only when the size actually changed (cheap to call every frame).
    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        if width == self.width && height == self.height {
            return;
        }
        self.texture = Self::make_texture(device, width, height);
        self.view = self.texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.width = width;
        self.height = height;
    }

    fn make_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("wgpu_pick_object_id"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::FORMAT,
            // RENDER_ATTACHMENT: drawn into; COPY_SRC: read a row back for the pick.
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    }

    /// View to attach as the id color attachment (clear to [`ID_NONE`]).
    #[inline]
    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// Texture handle to hand to [`IdPicker::submit`].
    #[inline]
    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    /// Current `(width, height)`.
    #[inline]
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The pipeline color-target for the id output: [`Self::FORMAT`], **no blend**, write all.
    /// Ids are identities, not colors - blending them would fabricate nonexistent ids.
    pub fn color_target_state() -> wgpu::ColorTargetState {
        wgpu::ColorTargetState {
            format: Self::FORMAT,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        }
    }

    /// Clear-op value for the id attachment (background id).
    #[inline]
    pub fn clear_value() -> wgpu::Color {
        // R32Uint clears take a Color; only `.r` is used, reinterpreted as the uint bits.
        wgpu::Color { r: ID_NONE as f64, g: 0.0, b: 0.0, a: 0.0 }
    }
}

/// Async object-id readback with 1-frame latency.
///
/// Convention: an id of [`ID_NONE`] means the cursor is over background. Lifecycle per frame:
/// [`Self::request`] (on input) -> [`Self::submit`] (after encoding the id pass) ->
/// [`Self::poll`] (after `queue.submit`).
#[derive(Default)]
pub struct IdPicker {
    /// MAP_READ + COPY_DST staging buffer sized to one aligned texture row.
    buffer: Option<wgpu::Buffer>,
    buffer_bpr: u32,
    /// Requested pixel (set by [`Self::request`], consumed by [`Self::submit`]).
    pending: Option<(u32, u32)>,
    /// Pixel X whose row was copied (consumed by [`Self::poll`]).
    pending_px: Option<u32>,
    /// Last successfully read canonical id (persists between polls).
    last: u32,
}

impl IdPicker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a pick at viewport pixel `(x, y)` (top-left origin). Overwrites any pending
    /// request - only the latest cursor position matters for hover.
    pub fn request(&mut self, x: u32, y: u32) {
        self.pending = Some((x, y));
    }

    /// Is a pick queued but not yet read back?
    #[inline]
    pub fn is_pending(&self) -> bool {
        self.pending.is_some() || self.pending_px.is_some()
    }

    /// Encode the row copy for the pending pick, if any. Call **after** the id pass is encoded
    /// into `encoder` and **before** `queue.submit(encoder)`. No-op without a pending request
    /// or when the pixel is out of bounds.
    pub fn submit(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        id_texture: &wgpu::Texture,
        size: (u32, u32),
    ) {
        let Some((px, py)) = self.pending.take() else {
            return;
        };
        if px >= size.0 || py >= size.1 {
            log::trace!("wgpu-pick: request ({px},{py}) out of bounds {size:?}");
            return;
        }
        let bpr = aligned_bpr(size.0);
        self.ensure_buffer(device, bpr);
        let Some(buf) = self.buffer.as_ref() else {
            return;
        };
        // Copy the whole row containing the pixel (row-copy honors the 256-byte alignment; the
        // exact pixel is picked out by byte offset in `poll`).
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: id_texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: py, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bpr),
                    rows_per_image: Some(1),
                },
            },
            wgpu::Extent3d { width: size.0, height: 1, depth_or_array_layers: 1 },
        );
        self.pending_px = Some(px);
    }

    /// Read back the copied pixel, if one was submitted. Call **after** `queue.submit`.
    /// Returns the canonical id (with [`SELECTED_BIT`] stripped); `Some(`[`ID_NONE`]`)` means
    /// background. Blocks briefly to map the staging buffer.
    pub fn poll(&mut self, device: &wgpu::Device) -> Option<u32> {
        let px = self.pending_px.take()?;
        let buf = self.buffer.as_ref()?;
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        // Drive the map callback to completion (the copy was already submitted).
        let _ = device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
        let offset = (px as usize) * 4;
        let id = {
            let data = slice.get_mapped_range().ok()?; // wgpu 30: fallible; None on map failure
            if offset + 4 <= data.len() {
                Some(u32::from_le_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                ]))
            } else {
                None
            }
        };
        buf.unmap();
        let raw = id?;
        self.last = canonical_id(raw);
        Some(self.last)
    }

    /// Last id read by [`Self::poll`] (canonical; [`ID_NONE`] until the first successful read).
    #[inline]
    pub fn last(&self) -> u32 {
        self.last
    }

    fn ensure_buffer(&mut self, device: &wgpu::Device, bpr: u32) {
        if self.buffer.is_none() || self.buffer_bpr < bpr {
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("wgpu_pick_readback"),
                size: bpr as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }));
            self.buffer_bpr = bpr;
        }
    }
}

/// Generic `u32 id <-> handle` map with per-frame stable reuse.
///
/// The consumer walks its scene each frame and calls [`Self::alloc`] per pickable object in a
/// **deterministic order**, writing the returned id into the object's instance data. Ids start
/// at `1` ([`ID_NONE`] = 0 is reserved). [`Self::begin_frame`] rewinds the counter but keeps
/// the map, so a stable traversal reuses the same ids.
pub struct IdRegistry<H> {
    next: u32,
    map: HashMap<u32, H>,
}

impl<H> Default for IdRegistry<H> {
    fn default() -> Self {
        Self { next: 1, map: HashMap::new() }
    }
}

impl<H> IdRegistry<H> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Rewind id allocation for a new frame (keeps existing entries for reuse).
    pub fn begin_frame(&mut self) {
        self.next = 1;
    }

    /// Allocate the next id and associate `handle` with it. Returns the id (never [`ID_NONE`]).
    pub fn alloc(&mut self, handle: H) -> u32 {
        let id = self.next;
        self.next = self.next.wrapping_add(1).max(1);
        self.map.insert(id, handle);
        id
    }

    /// Look up the handle for an id (stripping [`SELECTED_BIT`]); `None` for [`ID_NONE`]/unknown.
    pub fn get(&self, id: u32) -> Option<&H> {
        let id = canonical_id(id);
        if id == ID_NONE {
            return None;
        }
        self.map.get(&id)
    }

    /// Drop all entries (e.g. on scope change).
    pub fn clear(&mut self) {
        self.map.clear();
        self.next = 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_strips_selected_bit() {
        assert_eq!(canonical_id(7), 7);
        assert_eq!(canonical_id(7 | SELECTED_BIT), 7);
        assert_eq!(canonical_id(ID_NONE), ID_NONE);
    }

    #[test]
    fn aligned_bpr_is_256_multiple_and_fits() {
        for w in [1u32, 63, 64, 100, 255, 256, 257, 1920] {
            let bpr = aligned_bpr(w);
            assert_eq!(bpr % 256, 0, "bpr {bpr} not 256-aligned for w={w}");
            assert!(bpr >= w * 4, "bpr {bpr} too small for w={w}");
        }
    }

    #[test]
    fn registry_allocs_from_one_and_looks_up() {
        let mut reg: IdRegistry<&'static str> = IdRegistry::new();
        let a = reg.alloc("node_a");
        let b = reg.alloc("node_b");
        assert_eq!(a, 1);
        assert_eq!(b, 2);
        assert_eq!(reg.get(a), Some(&"node_a"));
        assert_eq!(reg.get(b), Some(&"node_b"));
        // SELECTED_BIT is transparent to lookup.
        assert_eq!(reg.get(b | SELECTED_BIT), Some(&"node_b"));
        // Background / unknown.
        assert_eq!(reg.get(ID_NONE), None);
        assert_eq!(reg.get(999), None);
    }

    #[test]
    fn begin_frame_reuses_ids_for_stable_order() {
        let mut reg: IdRegistry<u32> = IdRegistry::new();
        assert_eq!(reg.alloc(10), 1);
        assert_eq!(reg.alloc(20), 2);
        reg.begin_frame();
        // Same traversal order -> same ids, values refreshed.
        assert_eq!(reg.alloc(11), 1);
        assert_eq!(reg.alloc(21), 2);
        assert_eq!(reg.get(1), Some(&11));
        assert_eq!(reg.get(2), Some(&21));
    }

    #[test]
    fn picker_starts_idle() {
        let p = IdPicker::new();
        assert!(!p.is_pending());
        assert_eq!(p.last(), ID_NONE);
    }

    #[test]
    fn picker_request_sets_pending() {
        let mut p = IdPicker::new();
        p.request(10, 20);
        assert!(p.is_pending());
    }
}
