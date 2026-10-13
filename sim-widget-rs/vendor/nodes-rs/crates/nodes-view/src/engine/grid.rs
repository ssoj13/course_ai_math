//! Background grid on the graph working plane.
//!
//! # What
//!
//! Minor/major lines in graph `(u, v)`, drawn with the line pipeline (`line.wgsl`).
//!
//! # Why
//!
//! The grid covers the **scene bounding box + margin** (a FINITE region), not the whole visible
//! frustum∩plane. At grazing angles that visible region runs to the horizon, blew past the
//! per-axis line cap, and the far half of the screen lost its grid ("lines disappear at some
//! angles"). A scene-sized grid is bounded, stable at every angle, and "large enough to cover the
//! nodes" — which is all a node editor needs (no true-infinite shader required). The minor-line
//! LOD still follows zoom; the step only coarsens beyond that if a very large scene would exceed
//! the line cap, so the whole scene is ALWAYS covered.
//!
//! # Where used
//!
//! - [`crate::engine::GraphRenderer::render`] — first pass (clear + grid) in `nodes-view`.
//! - `nodes-editor` / `nodes-egui` viewports via the shared [`crate::GraphRenderer`].

use glam::Vec2;

use super::line_pipeline::LineVertex;
use crate::camera::Camera;
use nodes_core::GRID_STEP;

// Grid line colors come from the shared `ui_theme::Theme` (SSOT): `grid_minor` / `grid_major` /
// `grid_axis`.

/// Target pixel spacing between minor lines.
const TARGET_MINOR_PX: f32 = 40.0;
/// Major line every N minor steps (5 × 20 = 100 when minor is 20).
const MAJOR_SUBDIV: u32 = 5;
/// Cap line count per axis. Lines are cheap (2 verts each), so keep this generous — it only
/// bounds pathological zoom-outs; the step auto-coarsens to stay under it while still covering
/// the whole (4× bbox) grid.
const MAX_LINES_PER_AXIS: usize = 2048;
/// Grid spans this multiple of the scene bounding box (centred on it): plenty of working room
/// around the nodes without being "infinite".
const GRID_BBOX_SCALE: f32 = 4.0;
/// Minimum grid half-extent (world units) around the bounds centre, so an empty or tiny scene
/// still shows a reasonable grid instead of a couple of lines.
const MIN_HALF_EXTENT: f32 = 2000.0;
/// When `true`, the grid cell size adapts to zoom (LOD via [`minor_step_world`] + per-axis
/// line-cap coarsening). When `false` (default), the cell is FIXED at [`GRID_STEP`] and only the
/// BOUNDS track the scene — flip this to restore the adaptive behaviour (kept for that reason).
const GRID_DYNAMIC_STEP: bool = false;

fn lv(pos: [f32; 2], color: [f32; 4]) -> LineVertex {
    LineVertex { pos, color }
}

/// Round `world` to a “nice” step (1, 2, 5 × 10^n).
fn nice_step(world: f32) -> f32 {
    if !world.is_finite() || world <= 0.0 {
        return GRID_STEP;
    }
    let exp = world.log10().floor();
    let scale = 10f32.powf(exp);
    let f = world / scale;
    let mantissa = if f <= 1.0 {
        1.0
    } else if f <= 2.0 {
        2.0
    } else if f <= 5.0 {
        5.0
    } else {
        10.0
    };
    mantissa * scale
}

/// Grid step from zoom only (stable while orbiting — does not depend on tilted AABB span).
///
/// `ppw` is on-plane pixels-per-world ([`Camera::px_per_world_at_target`]) — the perspective
/// replacement for the old global `viewport / ortho_width`.
fn minor_step_world(ppw: f32) -> f32 {
    nice_step((TARGET_MINOR_PX / ppw.max(1e-6)).max(GRID_STEP * 0.25))
}

fn is_major_line(coord: f32, minor: f32) -> bool {
    if coord.abs() < minor * 0.01 {
        return true;
    }
    let major = minor * MAJOR_SUBDIV as f32;
    let q = (coord / major).round();
    (coord - q * major).abs() < minor * 0.05
}

fn line_color(
    coord: f32,
    minor: f32,
    major_line: bool,
    minor_visible: bool,
    theme: &ui_theme::Theme,
) -> Option<[f32; 4]> {
    if coord.abs() < minor * 0.01 && major_line {
        return Some(theme.grid_axis);
    }
    if major_line {
        return Some(theme.grid_major);
    }
    if minor_visible {
        return Some(theme.grid_minor);
    }
    None
}

/// Builds line-list vertices for a grid covering `scene_bounds` (graph-UV min/max) plus a margin.
///
/// `scene_bounds = None` (empty scene) falls back to a default region around the origin. Line
/// spacing follows zoom ([`minor_step_world`]); it only coarsens beyond that if the padded scene
/// is so large it would exceed [`MAX_LINES_PER_AXIS`], guaranteeing the whole scene is covered.
pub fn build_grid_vertices(
    camera: &Camera,
    viewport_w: f32,
    viewport_h: f32,
    scene_bounds: Option<(Vec2, Vec2)>,
    theme: &ui_theme::Theme,
) -> Vec<LineVertex> {
    let w = viewport_w.max(1.0);
    let h = viewport_h.max(1.0);
    // On-plane pixels-per-world — the minor-line visibility cutoff (and, when the dynamic step is
    // enabled, the zoom LOD).
    let pixel_per_world = camera.px_per_world_at_target(Vec2::new(w, h)).max(1e-6);

    // Region to cover: the scene AABB scaled 4× about its centre (generous working room, still
    // finite), floored to a minimum half-extent so a tiny/empty scene still gets a full grid.
    // The BOUNDS stay dynamic (they follow the scene); only the cell SIZE is fixed below.
    let (bmin, bmax) = scene_bounds.unwrap_or((Vec2::ZERO, Vec2::ZERO));
    let center = (bmin + bmax) * 0.5;
    let half = ((bmax - bmin) * 0.5 * GRID_BBOX_SCALE).max(Vec2::splat(MIN_HALF_EXTENT));
    let min_uv = center - half;
    let max_uv = center + half;

    // Cell size. FIXED at `GRID_STEP` by default — the grid keeps a constant world cell no matter
    // the zoom; only its extent (the bounds above) tracks the scene. The zoom-adaptive step (LOD +
    // per-axis line-cap coarsening) is RETAINED behind `GRID_DYNAMIC_STEP`. In the fixed path the
    // `MAX_LINES_PER_AXIS` loop cap bounds extreme zoom-outs by truncating EXTENT (changing bounds)
    // rather than resizing cells.
    let minor = if GRID_DYNAMIC_STEP {
        let zoom_minor = minor_step_world(pixel_per_world);
        let span = (max_uv - min_uv).max_element().max(zoom_minor);
        let cover_minor = span / MAX_LINES_PER_AXIS as f32;
        nice_step(zoom_minor.max(cover_minor))
    } else {
        GRID_STEP
    };

    let show_minor = pixel_per_world * minor >= 3.0;

    let u_start = (min_uv.x / minor).floor() * minor;
    let u_end = (max_uv.x / minor).ceil() * minor;
    let v_start = (min_uv.y / minor).floor() * minor;
    let v_end = (max_uv.y / minor).ceil() * minor;

    let mut out = Vec::new();

    let mut u = u_start;
    let mut u_lines = 0usize;
    while u <= u_end + minor * 0.001 && u_lines < MAX_LINES_PER_AXIS {
        let major = is_major_line(u, minor);
        if let Some(c) = line_color(u, minor, major, show_minor, theme) {
            out.push(lv([u, v_start], c));
            out.push(lv([u, v_end], c));
        }
        u += minor;
        u_lines += 1;
    }

    let mut v = v_start;
    let mut v_lines = 0usize;
    while v <= v_end + minor * 0.001 && v_lines < MAX_LINES_PER_AXIS {
        let major = is_major_line(v, minor);
        if let Some(c) = line_color(v, minor, major, show_minor, theme) {
            out.push(lv([u_start, v], c));
            out.push(lv([u_end, v], c));
        }
        v += minor;
        v_lines += 1;
    }

    out
}
