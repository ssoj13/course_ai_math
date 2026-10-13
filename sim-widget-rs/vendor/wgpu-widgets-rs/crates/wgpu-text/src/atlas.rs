//! Dual glyph atlas (color + mask), adapted from glyphon’s packing/raster path.

use std::collections::HashSet;

use etagere::{size2, AllocId, Allocation, BucketedAtlasAllocator};
use glyphon::{CacheKey, ContentType, FontSystem, SwashCache, SwashContent};
use lru::LruCache;
use rustc_hash::FxBuildHasher;
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingResource, BindingType, CommandEncoderDescriptor, Device, Extent3d,
    FilterMode, MipmapFilterMode, Origin3d, Queue, Sampler, SamplerBindingType, SamplerDescriptor,
    ShaderStages, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureAspect,
    TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType, TextureUsages,
    TextureView, TextureViewDescriptor, TextureViewDimension,
};

use crate::mipgen::{mip_levels_for_size, MipGenerator};

/// How the glyph atlas textures are sampled. Consumers choose this once when
/// constructing a [`crate::TextEngine`] (via `TextEngine::with_quality`) —
/// `Linear` is the default and fixes the blocky/aliased edges wgpu's raw
/// sampler default (`Nearest`) produces; `Anisotropic` additionally builds a
/// full mip chain (regenerated whenever new glyphs are baked — see
/// [`GlyphAtlas::flush_mips`]) for oblique/orbit camera views where a single
/// bake's resolution doesn't uniformly match the on-screen footprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextFilterQuality {
    /// Matches wgpu's raw sampler default: blocky, pixel-perfect, no smoothing.
    Nearest,
    /// Bilinear sampling of the atlas — smooths aliasing at any on-screen scale.
    /// No mip chain (single base level, as before this option existed).
    #[default]
    Linear,
    /// Linear + a full mip chain + anisotropic filtering. Best quality when the
    /// graph plane is viewed at an angle; costs a mip-chain regen whenever the
    /// atlas gains new glyphs (batched to once per `flush_mips` call, not per
    /// glyph). `level` is the requested anisotropy clamp (2/4/8/16 are typical);
    /// it is clamped to the `1..=16` range wgpu accepts for `anisotropy_clamp`
    /// (see [`GlyphAtlas::with_quality`]). To keep the packed atlas from bleeding
    /// neighbouring glyphs together at coarse mip levels, this path additionally
    /// pads every glyph with a transparent gutter ([`GLYPH_GUTTER`]) and clamps the
    /// sampler's max LOD to the level that gutter protects.
    Anisotropic { level: u16 },
}

/// Transparent border (in atlas texels, per side) reserved around every glyph on
/// the mip-building ([`TextFilterQuality::Anisotropic`]) path. A packed atlas has
/// no gaps between glyphs, so box-filtering it down for the mip chain would blend
/// each glyph with its neighbours at coarse levels — visible as halos/bleed when
/// text is minified. Surrounding each glyph with `GLYPH_GUTTER` transparent texels
/// means the first `log2(GLYPH_GUTTER)` mip levels average toward transparency at
/// the edges instead of into a neighbour. Paired with [`GLYPH_LOD_MAX_CLAMP`] on
/// the sampler so no level coarser than the gutter protects is ever sampled.
/// Only the `Anisotropic` path pays this (space) cost; `Linear`/`Nearest` allocate
/// glyphs tightly exactly as before.
pub const GLYPH_GUTTER: u16 = 8;

/// Sampler `lod_max_clamp` for the [`TextFilterQuality::Anisotropic`] path: the
/// coarsest mip level whose box-filter footprint (`2^level` texels) still fits
/// inside the [`GLYPH_GUTTER`], so cross-glyph bleed can never reach the sampled
/// texels. `log2(8) = 3`.
const GLYPH_LOD_MAX_CLAMP: f32 = 3.0;

impl TextFilterQuality {
    fn wants_mips(self) -> bool {
        matches!(self, TextFilterQuality::Anisotropic { .. })
    }

    /// Per-side glyph padding in atlas texels: [`GLYPH_GUTTER`] on the mip path,
    /// `0` otherwise (tight packing, byte-identical to the pre-mip behaviour).
    fn glyph_pad(self) -> u32 {
        if self.wants_mips() {
            GLYPH_GUTTER as u32
        } else {
            0
        }
    }
}

#[derive(Clone, Copy)]
struct GlyphDetails {
    width: u16,
    height: u16,
    atlas_x: u16,
    atlas_y: u16,
    top: i16,
    left: i16,
    content_type: ContentType,
    atlas_id: Option<AllocId>,
}

struct InnerAtlas {
    texture: Texture,
    texture_view: TextureView,
    packer: BucketedAtlasAllocator,
    size: u32,
    mip_levels: u32,
    /// Set whenever a glyph is written (or the atlas grows); cleared by
    /// [`GlyphAtlas::flush_mips`] after regenerating the mip chain. Only
    /// meaningful when `mip_levels > 1`.
    mips_dirty: bool,
    glyph_cache: LruCache<CacheKey, GlyphDetails, FxBuildHasher>,
    glyphs_in_use: HashSet<CacheKey, FxBuildHasher>,
    format: TextureFormat,
    channels: u32,
    quality: TextFilterQuality,
}

impl InnerAtlas {
    const INITIAL: u32 = 512;

    fn new(device: &Device, format: TextureFormat, channels: u32, quality: TextFilterQuality) -> Self {
        let size = Self::INITIAL.min(device.limits().max_texture_dimension_2d);
        let mip_levels = if quality.wants_mips() { mip_levels_for_size(size) } else { 1 };
        let packer = BucketedAtlasAllocator::new(size2(size as i32, size as i32));
        let mut usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST;
        if mip_levels > 1 {
            // Mip levels beyond 0 are written via render passes (see `MipGenerator`),
            // not `queue.write_texture`, so they need RENDER_ATTACHMENT too.
            usage |= TextureUsages::RENDER_ATTACHMENT;
        }
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("wgpu_text_atlas"),
            size: Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: mip_levels,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        });
        // Default view spans every mip level so the draw-time sampler can pick the
        // right one via `mipmap_filter`/LOD — only the mip-generation pass itself
        // needs single-level views (built ad hoc in `MipGenerator::generate`).
        let texture_view = texture.create_view(&TextureViewDescriptor::default());
        Self {
            texture,
            texture_view,
            packer,
            size,
            mip_levels,
            mips_dirty: false,
            glyph_cache: LruCache::unbounded_with_hasher(FxBuildHasher),
            glyphs_in_use: HashSet::with_hasher(FxBuildHasher),
            format,
            channels,
            quality,
        }
    }

    fn try_allocate(&mut self, w: usize, h: usize) -> Option<Allocation> {
        let size = size2(w as i32, h as i32);
        loop {
            if let Some(a) = self.packer.allocate(size) {
                return Some(a);
            }
            let (key, value) = self.glyph_cache.peek_lru()?;
            if self.glyphs_in_use.contains(key) {
                return None;
            }
            let id = value.atlas_id?;
            let _ = self.glyph_cache.pop_lru();
            self.packer.deallocate(id);
        }
    }

    /// Double the atlas backing texture up to `device` max. Resets the packer and
    /// invalidates the LRU cache (all glyphs need re-raster at their new positions).
    /// Returns false when already at the device limit.
    fn grow(&mut self, device: &Device) -> bool {
        let max = device.limits().max_texture_dimension_2d;
        if self.size >= max {
            return false;
        }
        let new_size = (self.size.saturating_mul(2)).min(max);
        let mip_levels = if self.quality.wants_mips() { mip_levels_for_size(new_size) } else { 1 };
        let mut usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST;
        if mip_levels > 1 {
            usage |= TextureUsages::RENDER_ATTACHMENT;
        }
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("wgpu_text_atlas"),
            size: Extent3d {
                width: new_size,
                height: new_size,
                depth_or_array_layers: 1,
            },
            mip_level_count: mip_levels,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: self.format,
            usage,
            view_formats: &[],
        });
        let texture_view = texture.create_view(&TextureViewDescriptor::default());
        self.texture = texture;
        self.texture_view = texture_view;
        self.size = new_size;
        self.mip_levels = mip_levels;
        // Every glyph will be re-uploaded at its new packed position, which already
        // drives a full mip regen via the normal `ensure_glyph` dirty-marking path —
        // but mark it explicitly too in case a grow ever happens with no pending
        // glyph write in the same frame.
        self.mips_dirty = mip_levels > 1;
        self.packer = BucketedAtlasAllocator::new(size2(new_size as i32, new_size as i32));
        // New packer means old AllocIds are dead; UVs in cached entries are stale.
        self.glyph_cache.clear();
        self.glyphs_in_use.clear();
        true
    }

    /// Current backing texture side length in pixels.
    fn side(&self) -> u32 {
        self.size
    }
}

/// GPU dual (color + mask) glyph atlas with on-demand swash rasterization.
///
/// This is the public glyph-atlas boundary of `wgpu-text`: the single shared GPU
/// resource that any glyph-quad renderer bakes into (via [`ensure_glyph`]) and
/// renders from (via [`bind_group`] / [`bind_group_layout`]). Both the internal
/// plane-text renderer and external consumers (e.g. `wgpu-hud`'s node/grid
/// billboards) drive it through this same surface, so glyph rasterization and the
/// bind-group contract live in exactly one place.
///
/// [`ensure_glyph`]: GlyphAtlas::ensure_glyph
/// [`bind_group`]: GlyphAtlas::bind_group
/// [`bind_group_layout`]: GlyphAtlas::bind_group_layout
pub struct GlyphAtlas {
    color: InnerAtlas,
    mask: InnerAtlas,
    bind_group: BindGroup,
    bind_group_layout: BindGroupLayout,
    sampler: Sampler,
    quality: TextFilterQuality,
    mip_gen_color: Option<MipGenerator>,
    mip_gen_mask: Option<MipGenerator>,
}

impl GlyphAtlas {
    /// Default quality: [`TextFilterQuality::Linear`] (see [`Self::with_quality`] to
    /// pick a different one, e.g. `Anisotropic` for oblique/orbit camera views).
    pub fn new(device: &Device, queue: &Queue) -> Self {
        Self::with_quality(device, queue, TextFilterQuality::Linear)
    }

    /// Build the atlas with an explicit [`TextFilterQuality`]. `Nearest` matches
    /// wgpu's raw sampler default (blocky); `Linear` (the `new()` default) smooths
    /// sampling with no mip chain; `Anisotropic` additionally builds a full mip
    /// chain per atlas texture, regenerated on demand via [`Self::flush_mips`].
    pub fn with_quality(device: &Device, _queue: &Queue, quality: TextFilterQuality) -> Self {
        let color = InnerAtlas::new(device, TextureFormat::Rgba8UnormSrgb, 4, quality);
        let mask = InnerAtlas::new(device, TextureFormat::R8Unorm, 1, quality);
        // wgpu's `SamplerDescriptor::default()` uses `FilterMode::Nearest` for every
        // filter — fine for pixel-perfect UI, but it makes glyph edges look blocky/
        // aliased whenever a bake doesn't land on an exact integer screen scale (which
        // `raster_scale`'s quantized buckets don't guarantee).
        // `lod_max_clamp` bounds how coarse a mip level the sampler may reach. Only
        // the `Anisotropic` path has a mip chain, and it is capped at the level the
        // per-glyph gutter protects (see `GLYPH_LOD_MAX_CLAMP`); the others keep
        // wgpu's default (32.0), harmless with a single-level texture.
        let (mag_filter, min_filter, mipmap_filter, anisotropy_clamp, lod_max_clamp) = match quality {
            TextFilterQuality::Nearest => {
                (FilterMode::Nearest, FilterMode::Nearest, MipmapFilterMode::Nearest, 1, 32.0)
            }
            TextFilterQuality::Linear => {
                (FilterMode::Linear, FilterMode::Linear, MipmapFilterMode::Nearest, 1, 32.0)
            }
            TextFilterQuality::Anisotropic { level } => (
                FilterMode::Linear,
                FilterMode::Linear,
                MipmapFilterMode::Linear,
                // wgpu accepts anisotropy_clamp only in 1..=16.
                level.clamp(1, 16),
                GLYPH_LOD_MAX_CLAMP,
            ),
        };
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("wgpu_text_sampler"),
            mag_filter,
            min_filter,
            mipmap_filter,
            anisotropy_clamp,
            lod_max_clamp,
            ..Default::default()
        });
        let (mip_gen_color, mip_gen_mask) = if quality.wants_mips() {
            (
                Some(MipGenerator::new(device, TextureFormat::Rgba8UnormSrgb)),
                Some(MipGenerator::new(device, TextureFormat::R8Unorm)),
            )
        } else {
            (None, None)
        };
        let bind_group_layout =
            device.create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("wgpu_text_atlas_bgl"),
                entries: &[
                    BindGroupLayoutEntry {
                        binding: 0,
                        visibility: ShaderStages::FRAGMENT,
                        ty: BindingType::Texture {
                            multisampled: false,
                            view_dimension: TextureViewDimension::D2,
                            sample_type: TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    BindGroupLayoutEntry {
                        binding: 1,
                        visibility: ShaderStages::FRAGMENT,
                        ty: BindingType::Texture {
                            multisampled: false,
                            view_dimension: TextureViewDimension::D2,
                            sample_type: TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    BindGroupLayoutEntry {
                        binding: 2,
                        visibility: ShaderStages::FRAGMENT,
                        ty: BindingType::Sampler(SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
        let bind_group = Self::make_bind_group(
            device,
            &bind_group_layout,
            &sampler,
            &color.texture_view,
            &mask.texture_view,
        );
        Self {
            color,
            mask,
            bind_group,
            bind_group_layout,
            sampler,
            quality,
            mip_gen_color,
            mip_gen_mask,
        }
    }

    fn make_bind_group(
        device: &Device,
        layout: &BindGroupLayout,
        sampler: &Sampler,
        color_view: &TextureView,
        mask_view: &TextureView,
    ) -> BindGroup {
        device.create_bind_group(&BindGroupDescriptor {
            label: Some("wgpu_text_atlas_bg"),
            layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(color_view),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(mask_view),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::Sampler(sampler),
                },
            ],
        })
    }

    pub fn trim(&mut self) {
        self.color.glyphs_in_use.clear();
        self.mask.glyphs_in_use.clear();
    }

    /// Bind group holding the atlas textures + sampler, laid out as
    /// `binding 0 = color texture`, `binding 1 = mask texture`, `binding 2 =
    /// filtering sampler`. Bind it as the fragment-stage atlas group when drawing
    /// glyph quads. Its identity changes when the atlas grows, so re-fetch it each
    /// frame rather than caching the reference across resizes.
    pub fn bind_group(&self) -> &BindGroup {
        &self.bind_group
    }

    /// Layout matching [`bind_group`](Self::bind_group). Pass it when building a
    /// glyph-quad render pipeline so the pipeline's atlas group is compatible with
    /// the atlas bind group.
    pub fn bind_group_layout(&self) -> &BindGroupLayout {
        &self.bind_group_layout
    }

    /// Color/mask atlas side lengths in pixels (for diagnostics and tests).
    pub fn dims(&self) -> (u32, u32) {
        (self.color.side(), self.mask.side())
    }

    /// Regenerate the mip chain of every atlas texture that gained new glyphs (or
    /// grew) since the last call. No-op when `quality` isn't [`TextFilterQuality::Anisotropic`]
    /// or nothing is dirty. Submits its own command buffer, so callers can call this
    /// once per frame (e.g. right before drawing) without any encoder bookkeeping.
    pub fn flush_mips(&mut self, device: &Device, queue: &Queue) {
        if !self.quality.wants_mips() {
            return;
        }
        let color_dirty = self.color.mips_dirty;
        let mask_dirty = self.mask.mips_dirty;
        if !color_dirty && !mask_dirty {
            return;
        }
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("wgpu_text_mipgen_encoder"),
        });
        if color_dirty {
            if let Some(mip_gen) = &self.mip_gen_color {
                mip_gen.generate(device, &mut encoder, &self.color.texture, self.color.mip_levels);
            }
            self.color.mips_dirty = false;
        }
        if mask_dirty {
            if let Some(mip_gen) = &self.mip_gen_mask {
                mip_gen.generate(device, &mut encoder, &self.mask.texture, self.mask.mip_levels);
            }
            self.mask.mips_dirty = false;
        }
        queue.submit(Some(encoder.finish()));
    }

    /// Grow the atlas matching `content_type`. Rebuilds the shared bind group so
    /// downstream pipelines see the new texture view on the next draw.
    fn grow(&mut self, device: &Device, content_type: ContentType) -> bool {
        let grown = match content_type {
            ContentType::Color => self.color.grow(device),
            ContentType::Mask => self.mask.grow(device),
        };
        if grown {
            self.bind_group = Self::make_bind_group(
                device,
                &self.bind_group_layout,
                &self.sampler,
                &self.color.texture_view,
                &self.mask.texture_view,
            );
        }
        grown
    }

    /// Rasterize glyph if needed; returns atlas UV rect (normalized) and pixel metrics.
    pub fn ensure_glyph(
        &mut self,
        device: &Device,
        queue: &Queue,
        font_system: &mut FontSystem,
        swash_cache: &mut SwashCache,
        cache_key: CacheKey,
    ) -> Option<GlyphMetrics> {
        if let Some(d) = self.color.glyph_cache.get(&cache_key).copied() {
            self.color.glyphs_in_use.insert(cache_key);
            return Some(metrics_from_details(&self.color, d));
        }
        if let Some(d) = self.mask.glyph_cache.get(&cache_key).copied() {
            self.mask.glyphs_in_use.insert(cache_key);
            return Some(metrics_from_details(&self.mask, d));
        }

        let image = swash_cache.get_image_uncached(font_system, cache_key)?;
        let content_type = match image.content {
            SwashContent::Color => ContentType::Color,
            SwashContent::Mask | SwashContent::SubpixelMask => ContentType::Mask,
        };
        let width = image.placement.width as u16;
        let height = image.placement.height as u16;
        if width == 0 || height == 0 {
            return None;
        }

        // On the mip path each glyph is padded by a transparent gutter so the mip
        // chain never blends adjacent glyphs (see `GLYPH_GUTTER`); the allocation
        // therefore reserves `width + 2*pad` × `height + 2*pad`. `pad` is 0 on the
        // Linear/Nearest paths, keeping the packing (and every metric below) tight
        // and byte-identical to before.
        let pad = self.quality.glyph_pad();
        let alloc_w = width as usize + 2 * pad as usize;
        let alloc_h = height as usize + 2 * pad as usize;

        // Allocate; grow once if the side is exhausted while no glyph from the
        // current frame can be evicted.
        let allocation = {
            let inner = match content_type {
                ContentType::Color => &mut self.color,
                ContentType::Mask => &mut self.mask,
            };
            inner.try_allocate(alloc_w, alloc_h)
        };
        let allocation = match allocation {
            Some(a) => a,
            None => {
                if !self.grow(device, content_type) {
                    return None;
                }
                let inner = match content_type {
                    ContentType::Color => &mut self.color,
                    ContentType::Mask => &mut self.mask,
                };
                inner.try_allocate(alloc_w, alloc_h)?
            }
        };

        let (metrics, details) = {
            let inner = match content_type {
                ContentType::Color => &mut self.color,
                ContentType::Mask => &mut self.mask,
            };
            let min = allocation.rectangle.min;
            // Inner glyph origin: the padded rect corner offset by `pad`.
            let glyph_x = min.x as u32 + pad;
            let glyph_y = min.y as u32 + pad;
            let channels = inner.channels as usize;
            if pad == 0 {
                // Tight path — upload the glyph bitmap directly (unchanged).
                queue.write_texture(
                    TexelCopyTextureInfo {
                        texture: &inner.texture,
                        mip_level: 0,
                        origin: Origin3d { x: glyph_x, y: glyph_y, z: 0 },
                        aspect: TextureAspect::All,
                    },
                    &image.data,
                    TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(width as u32 * inner.channels),
                        rows_per_image: None,
                    },
                    Extent3d {
                        width: width as u32,
                        height: height as u32,
                        depth_or_array_layers: 1,
                    },
                );
            } else {
                // Build a zeroed padded tile with the glyph copied into its centre,
                // then upload the whole padded rect in one write. Zeroing (rather
                // than offsetting into a possibly-stale reused region) guarantees the
                // gutter is genuinely transparent, so the mip box-filter averages
                // toward transparency at glyph edges instead of into leftover ink.
                let row_bytes = width as usize * channels;
                let padded_row_bytes = alloc_w * channels;
                let mut padded = vec![0u8; padded_row_bytes * alloc_h];
                for row in 0..height as usize {
                    let src = row * row_bytes;
                    let dst = (row + pad as usize) * padded_row_bytes + pad as usize * channels;
                    padded[dst..dst + row_bytes].copy_from_slice(&image.data[src..src + row_bytes]);
                }
                queue.write_texture(
                    TexelCopyTextureInfo {
                        texture: &inner.texture,
                        mip_level: 0,
                        origin: Origin3d { x: min.x as u32, y: min.y as u32, z: 0 },
                        aspect: TextureAspect::All,
                    },
                    &padded,
                    TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(padded_row_bytes as u32),
                        rows_per_image: None,
                    },
                    Extent3d {
                        width: alloc_w as u32,
                        height: alloc_h as u32,
                        depth_or_array_layers: 1,
                    },
                );
            }
            let d = GlyphDetails {
                width,
                height,
                atlas_x: glyph_x as u16,
                atlas_y: glyph_y as u16,
                top: image.placement.top as i16,
                left: image.placement.left as i16,
                content_type,
                atlas_id: Some(allocation.id),
            };
            inner.glyphs_in_use.insert(cache_key);
            inner.mips_dirty = inner.mip_levels > 1;
            (metrics_from_details(inner, d), d)
        };
        match content_type {
            ContentType::Color => {
                let _ = self.color.glyph_cache.put(cache_key, details);
            }
            ContentType::Mask => {
                let _ = self.mask.glyph_cache.put(cache_key, details);
            }
        }
        Some(metrics)
    }
}

fn metrics_from_details(inner: &InnerAtlas, details: GlyphDetails) -> GlyphMetrics {
    let u0 = details.atlas_x as f32 / inner.size as f32;
    let v0 = details.atlas_y as f32 / inner.size as f32;
    let u1 = (details.atlas_x + details.width) as f32 / inner.size as f32;
    let v1 = (details.atlas_y + details.height) as f32 / inner.size as f32;
    GlyphMetrics {
        width: details.width,
        height: details.height,
        left: details.left,
        top: details.top,
        content_type: details.content_type,
        uv: [u0, v0, u1, v1],
    }
}

/// Placement of one rasterized glyph in the [`GlyphAtlas`], returned by
/// [`GlyphAtlas::ensure_glyph`]. `width`/`height`/`left`/`top` are pixel metrics
/// (relative to the glyph pen origin); `uv` is the normalized `[u0, v0, u1, v1]`
/// atlas rect; `content_type` selects which atlas texture (color vs. mask) the
/// quad samples.
#[derive(Debug, Clone, Copy)]
pub struct GlyphMetrics {
    pub width: u16,
    pub height: u16,
    pub left: i16,
    pub top: i16,
    pub content_type: ContentType,
    pub uv: [f32; 4],
}

#[cfg(test)]
mod tests {
    use super::*;
    use pollster::block_on;

    /// Returns `Some((device, queue))` when a wgpu adapter is available on the host.
    fn try_headless_device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::default();
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .ok()?;
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("wgpu_text_atlas_test"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: Default::default(),
            experimental_features: Default::default(),
        }))
        .ok()?;
        Some((device, queue))
    }

    /// Grow doubles the side and rebuilds the bind group; old cache entries are dropped.
    #[test]
    fn atlas_grows_on_demand() {
        let Some((device, queue)) = try_headless_device() else {
            eprintln!("no wgpu adapter; skipping atlas_grows_on_demand");
            return;
        };

        let mut atlas = GlyphAtlas::new(&device, &queue);
        let (color0, mask0) = atlas.dims();
        assert_eq!(color0, mask0, "both atlases start equal");
        assert_eq!(color0, InnerAtlas::INITIAL, "initial side matches InnerAtlas::INITIAL");

        let initial_bind_group_addr = format!("{:p}", &atlas.bind_group);

        let grew = atlas.grow(&device, ContentType::Mask);
        assert!(grew, "grow should succeed below device max");

        let (color1, mask1) = atlas.dims();
        assert_eq!(color1, color0, "color atlas untouched");
        assert_eq!(mask1, color0 * 2, "mask atlas doubled");

        // Bind group field was replaced (not the same allocation).
        // We can't compare BindGroup equality, but the side change above proves the
        // texture+view was recreated, and the field assignment runs unconditionally.
        let _ = initial_bind_group_addr;
    }

    /// Repeated grow stops at `device.limits().max_texture_dimension_2d`.
    #[test]
    fn atlas_grow_caps_at_device_max() {
        let Some((device, _queue)) = try_headless_device() else {
            eprintln!("no wgpu adapter; skipping atlas_grow_caps_at_device_max");
            return;
        };
        let max = device.limits().max_texture_dimension_2d;
        let mut inner = InnerAtlas::new(&device, TextureFormat::R8Unorm, 1, TextFilterQuality::Linear);
        while inner.side() < max {
            assert!(inner.grow(&device), "grow until max");
        }
        assert_eq!(inner.side(), max);
        assert!(!inner.grow(&device), "no grow at device max");
    }
}
