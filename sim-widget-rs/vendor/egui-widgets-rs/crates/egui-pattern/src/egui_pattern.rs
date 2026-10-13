//! Procedural pattern fills for egui: a rect of any size filled with a pattern is ONE textured
//! quad.
//!
//! **Why:** drawing a pattern as shapes costs one shape per line or dot. A timeline hatching its
//! file clips emitted one line per 6 points across every clip - 177 000 shapes and ~25 ms a frame on
//! a 7000-frame comp zoomed in (Playa `timeline_panel_cost`), almost all of them off screen. A tile
//! repeated by the GPU sampler costs the same at any size and zoom, and any pattern the generator
//! can describe costs the same as a hatch.
//!
//! **How:**
//! ```text
//! Pattern (data: up to 3 periodic Prims)  --tile::rasterize(pattern, pixels_per_point)-->  Tile
//!   seamless (periods in whole pixels, tile = their LCM), 16x16-supersampled coverage
//! fill_pattern(painter, rect, &pattern, color, origin)
//!   texture: the tile as a white mask, TextureWrapMode::Repeat, made once per (pattern, scale)
//!            and kept in the context's temp data
//!   mesh:    one quad over rect ∩ clip, UVs in tiles measured from `origin`, tinted by `color`
//! ```
//! `origin` pins the pattern's phase: pass the item's own corner (a clip's left edge) and the pattern
//! moves with the item instead of crawling while the view pans.
//!
//! **Where used:** `egui-track-timeline` (`Fill::Pattern` clips).

mod tile;

pub use tile::{Dir, MAX_PRIMS, MAX_TILE, Pattern, Prim, Tile, TileError, rasterize};

use egui::{Color32, Context, Id, Mesh, Painter, Pos2, Rect, TextureHandle, TextureOptions};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

/// Fill `rect` with `pattern` in `color`, the pattern's phase pinned to `origin`: one textured quad
/// over the part of `rect` inside the painter's clip rect (nothing when none is). A pattern the
/// generator refuses ([`TileError`]) draws nothing and is logged once.
pub fn fill_pattern(
    painter: &Painter,
    rect: Rect,
    pattern: &Pattern,
    color: Color32,
    origin: Pos2,
) {
    let shown = rect.intersect(painter.clip_rect());
    if !shown.is_positive() {
        return;
    }
    let ctx = painter.ctx();
    let ppp = ctx.pixels_per_point();
    let Some(texture) = texture(ctx, pattern, ppp) else {
        return;
    };
    let [w, h] = texture.size();
    let uv = |p: Pos2| {
        Pos2::new(
            (p.x - origin.x) * ppp / w as f32,
            (p.y - origin.y) * ppp / h as f32,
        )
    };
    let mut mesh = Mesh::with_texture(texture.id());
    mesh.add_rect_with_uv(
        shown,
        Rect::from_min_max(uv(shown.min), uv(shown.max)),
        color,
    );
    painter.add(mesh);
}

/// The tiles made so far, by pattern and scale; `None` for a pattern the generator refused.
#[derive(Clone, Default)]
struct Tiles(Arc<Mutex<HashMap<u64, Option<TextureHandle>>>>);

/// `pattern`'s tile at `ppp` as a repeating texture, made at its first use.
fn texture(ctx: &Context, pattern: &Pattern, ppp: f32) -> Option<TextureHandle> {
    let tiles = ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<Tiles>(Id::new("egui_pattern"))
            .clone()
    });
    let key = key(pattern, ppp);
    // The context's own lock is released here: `load_texture` below takes it again.
    let mut tiles = tiles
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    tiles
        .entry(key)
        .or_insert_with(|| match rasterize(pattern, ppp) {
            Ok(tile) => Some(load(ctx, &tile, key)),
            Err(error) => {
                log::error!(
                    "egui-pattern: {pattern:?} at {ppp} pixels per point has no tile: {error:?}"
                );
                None
            }
        })
        .clone()
}

/// `tile` as a white, alpha-masked texture repeating in both directions. Linear filtering: a quad
/// whose origin falls between pixels samples between texels instead of snapping.
fn load(ctx: &Context, tile: &Tile, key: u64) -> TextureHandle {
    let pixels: Vec<Color32> = tile
        .coverage
        .iter()
        .map(|alpha| Color32::from_white_alpha(*alpha))
        .collect();
    let image = egui::ColorImage::new([tile.width, tile.height], pixels);
    ctx.load_texture(
        format!("egui_pattern_{key:016x}"),
        image,
        TextureOptions {
            magnification: egui::TextureFilter::Linear,
            minification: egui::TextureFilter::Linear,
            wrap_mode: egui::TextureWrapMode::Repeat,
            ..Default::default()
        },
    )
}

/// The cache key of `pattern` at `ppp`: its exact values (f32 bits), not a rounding of them.
fn key(pattern: &Pattern, ppp: f32) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    ppp.to_bits().hash(&mut hasher);
    for prim in pattern.prims() {
        match *prim {
            Prim::Lines { dir, period, width } => {
                (0u8, dir, period.to_bits(), width.to_bits()).hash(&mut hasher)
            }
            Prim::Dots {
                period,
                radius,
                stagger,
            } => (1u8, period.to_bits(), radius.to_bits(), stagger).hash(&mut hasher),
            Prim::Checker { size } => (2u8, size.to_bits()).hash(&mut hasher),
        }
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every mesh `fill_pattern` emits for `rect` under a painter clipped to `clip`.
    fn fill(rect: Rect, clip: Rect, origin: Pos2) -> Vec<Mesh> {
        let ctx = Context::default();
        ctx.begin_pass(egui::RawInput::default());
        let painter = ctx
            .layer_painter(egui::LayerId::background())
            .with_clip_rect(clip);
        fill_pattern(&painter, rect, &Pattern::hatch(), Color32::BLACK, origin);
        let mut out = ctx.end_pass();
        // No GPU here: the uploads are dropped explicitly (epaint asserts otherwise).
        out.textures_delta.clear();
        out.shapes
            .into_iter()
            .map(|clipped| match clipped.shape {
                egui::Shape::Mesh(mesh) => Mesh::clone(&mesh),
                other => panic!("only a mesh is drawn, got {other:?}"),
            })
            .collect()
    }

    /// A bar 60 000 points wide with 300 on screen is one quad over the visible part, its UVs the
    /// visible edges' distance from the bar's origin in tiles (hatch at 1 pixel per point: 6
    /// pixels). The per-line drawing it replaces emitted ~10 000 shapes here.
    #[test]
    fn a_fill_is_one_quad_over_the_visible_part_on_the_items_pattern() {
        let bar = Rect::from_min_max(Pos2::new(-30_000.0, 0.0), Pos2::new(30_000.0, 16.0));
        let clip = Rect::from_min_max(Pos2::new(100.0, 0.0), Pos2::new(400.0, 100.0));
        let meshes = fill(bar, clip, bar.min);
        assert_eq!(meshes.len(), 1, "one shape");
        let mesh = &meshes[0];
        assert_eq!(mesh.vertices.len(), 4, "one quad");
        let xs: Vec<f32> = mesh.vertices.iter().map(|v| v.pos.x).collect();
        let us: Vec<f32> = mesh.vertices.iter().map(|v| v.uv.x).collect();
        assert_eq!(xs.iter().copied().fold(f32::INFINITY, f32::min), 100.0);
        assert_eq!(xs.iter().copied().fold(f32::NEG_INFINITY, f32::max), 400.0);
        let tile = 6.0;
        let u_min = us.iter().copied().fold(f32::INFINITY, f32::min);
        let u_max = us.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(
            (u_min - (100.0 - bar.min.x) / tile).abs() < 1e-3,
            "phase from the bar's origin: {u_min}"
        );
        assert!((u_max - (400.0 - bar.min.x) / tile).abs() < 1e-3);
        // Off screen: nothing.
        let away = Rect::from_min_max(Pos2::new(1_000.0, 0.0), Pos2::new(2_000.0, 16.0));
        assert!(fill(away, clip, away.min).is_empty());
    }

    /// The same pattern at the same scale is one texture, however often it is drawn.
    #[test]
    fn a_pattern_is_rasterised_once_per_scale() {
        let ctx = Context::default();
        let a = texture(&ctx, &Pattern::hatch(), 1.0).expect("a tile");
        let b = texture(&ctx, &Pattern::hatch(), 1.0).expect("a tile");
        assert_eq!(a.id(), b.id());
        let other = texture(&ctx, &Pattern::hatch(), 2.0).expect("a tile");
        assert_ne!(a.id(), other.id(), "another scale, another tile");
        assert_eq!(other.size(), [12, 12]);
    }
}
