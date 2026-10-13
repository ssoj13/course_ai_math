//! Frustum-vs-AABB node culling + screen-size LOD — the SINGLE visibility policy.
//!
//! ## What
//!
//! [`compute_visible_nodes`] answers, for every node in a [`LayoutCache`], two questions at once:
//!
//! 1. **Is it on screen?** — a conservative view-frustum test of the node's world AABB.
//! 2. **How much detail does it deserve?** — a [`NodeLod`] tier from its projected pixel size.
//!
//! The result is a [`VisibleNodes`] set that EVERY graph layer consults through the same accessors
//! ([`VisibleNodes::is_visible`] / [`VisibleNodes::lod`]): the renderer's shapes, tube wires and
//! plane text, AND the editor's node-widget layer. One policy, one algorithm — so a node can never
//! be drawn "half" (e.g. body culled but inline fields kept), which was exactly why the previous,
//! layer-local uv-rect cull was disabled.
//!
//! ## Why this design (and not the old `visible_graph_uv_rect`)
//!
//! The retired approach unprojected the viewport perimeter onto the plane to get a uv rectangle,
//! then tested node uv-AABBs against it. At grazing camera angles the perimeter rays miss the plane
//! and the rect collapses, culling on-screen nodes. Here we do the opposite and robust direction:
//! PROJECT each node's world AABB into clip space and cull only when it is provably outside one clip
//! boundary. Projection is always well-defined, so grazing angles are handled correctly, and the
//! test is conservative (a straddling / behind-camera box is never culled).
//!
//! ## Where used
//!
//! - `engine::GraphRenderer::render_scene` — computes it once per frame, drives shapes/wires/text.
//! - `nodes-editor::node_widgets` — same function, so inline widgets follow the same visible set.

use std::collections::HashMap;

use glam::{Mat4, Vec2, Vec3};
use nodes_core::{LayoutCache, LayoutNode, NodeId};

use crate::camera::{Camera, GraphPlane};

/// Level of detail for a node, chosen from its projected on-screen size. Tiers are cumulative
/// (each drops detail the previous kept): tiny nodes stop being worth their glyphs / sockets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeLod {
    /// Full detail: body, title, pins, inline fields and text labels.
    Full,
    /// Body + pins + fields, but NO text (glyph shaping/upload is the priciest per-node cost, and
    /// the label is unreadable at this size anyway).
    NoText,
    /// Body/title quad only — skip pins, inline fields and text. For nodes a few pixels tall.
    BodyOnly,
}

impl NodeLod {
    /// Whether plane text labels should be built for this node.
    #[inline]
    pub fn draws_text(self) -> bool {
        matches!(self, NodeLod::Full)
    }

    /// Whether per-port detail (pins, inline fields/widgets, chrome flags) should be built.
    #[inline]
    pub fn draws_detail(self) -> bool {
        !matches!(self, NodeLod::BodyOnly)
    }
}

/// Tunables for [`compute_visible_nodes`]. Defaults suit the node editor; hosts may override.
#[derive(Clone, Copy, Debug)]
pub struct VisibilityConfig {
    /// Master switch. `false` → [`VisibleNodes`] reports every node visible at [`NodeLod::Full`]
    /// (the "draw everything, let the GPU viewport clip" fallback).
    pub cull: bool,
    /// Half-thickness (world units) added along the plane normal when building a node's AABB, so a
    /// `Mesh3D` extruded card isn't clipped a frame early at grazing angles. `0` for the flat SDF
    /// backend.
    pub z_extrude: f32,
    /// Below this projected node HEIGHT (pixels) the node drops to [`NodeLod::BodyOnly`].
    pub lod_body_px: f32,
    /// Below this projected node HEIGHT (pixels) the node drops to [`NodeLod::NoText`].
    /// Must be `>= lod_body_px`.
    pub lod_no_text_px: f32,
}

impl Default for VisibilityConfig {
    fn default() -> Self {
        Self {
            cull: true,
            z_extrude: 0.0,
            // A node under ~8px tall is a sliver: only its body quad reads → BodyOnly.
            lod_body_px: 8.0,
            // Title/port text is ~13px; under ~18px node height it's unreadable → skip glyphs.
            lod_no_text_px: 18.0,
        }
    }
}

/// Conservative half-thickness (world units) for a `Mesh3D` extruded card's cull AABB — the SSOT
/// value shared by the renderer and the editor's widget layer via [`VisibilityConfig::for_backend`],
/// so both compute the SAME visible-set. Generous on purpose (culling errs toward keeping a node).
pub const MESH3D_Z_EXTRUDE: f32 = 24.0;

impl VisibilityConfig {
    /// Config for the active node-body backend: SDF is flat on the plane (`z_extrude = 0`); the
    /// Mesh3D card extrudes toward the camera, so its AABB gets [`MESH3D_Z_EXTRUDE`]. `cull` is the
    /// master on/off. LOD thresholds keep their defaults. This is the ONE place the backend →
    /// extrusion mapping lives, so the render pass and the widget layer never disagree.
    pub fn for_backend(cull: bool, mesh3d: bool) -> Self {
        Self {
            cull,
            z_extrude: if mesh3d { MESH3D_Z_EXTRUDE } else { 0.0 },
            ..Self::default()
        }
    }
}

/// The per-frame visible set: which nodes to draw and at what [`NodeLod`].
///
/// When culling is off, `nodes` is empty and the accessors report "everything visible, Full".
#[derive(Clone, Debug, Default)]
pub struct VisibleNodes {
    /// Only VISIBLE nodes are present; the value is their chosen LOD. Absent ⇒ culled.
    nodes: HashMap<NodeId, NodeLod>,
    /// `false` ⇒ culling disabled: [`Self::is_visible`] is always `true`, [`Self::lod`] is `Full`.
    culling: bool,
}

impl VisibleNodes {
    /// Whether `id` should be drawn this frame.
    #[inline]
    pub fn is_visible(&self, id: NodeId) -> bool {
        !self.culling || self.nodes.contains_key(&id)
    }

    /// LOD tier for `id` (`Full` when culling is off or the node isn't tracked).
    #[inline]
    pub fn lod(&self, id: NodeId) -> NodeLod {
        self.nodes.get(&id).copied().unwrap_or(NodeLod::Full)
    }

    /// `true` when this set actually culls (culling enabled).
    #[inline]
    pub fn culling(&self) -> bool {
        self.culling
    }

    /// Number of visible nodes (only meaningful when [`Self::culling`]).
    #[inline]
    pub fn visible_count(&self) -> usize {
        self.nodes.len()
    }
}

/// Compute the visible set + per-node LOD for one frame. Pure and cheap (O(nodes)); call it wherever
/// a layer needs the shared policy — it is the SSOT of the culling/LOD algorithm, not a cached state.
pub fn compute_visible_nodes(
    layout: &LayoutCache,
    camera: &Camera,
    viewport: Vec2,
    cfg: VisibilityConfig,
) -> VisibleNodes {
    if !cfg.cull {
        return VisibleNodes {
            nodes: HashMap::new(),
            culling: false,
        };
    }

    let w = viewport.x.max(1.0);
    let h = viewport.y.max(1.0);
    let frustum = Frustum::from_view_proj(camera.view_proj(w, h));
    let plane = &camera.plane;

    let mut nodes = HashMap::with_capacity(layout.nodes.len());
    for (id, ln) in &layout.nodes {
        let (lo, hi) = node_world_aabb(ln, plane, cfg.z_extrude);
        if !frustum.intersects_world_aabb(lo, hi) {
            continue;
        }
        nodes.insert(*id, node_lod(ln, camera, Vec2::new(w, h), &cfg));
    }
    VisibleNodes {
        nodes,
        culling: true,
    }
}

/// A view frustum stored as its combined `view_proj` matrix. AABB tests run in CLIP space
/// (per-corner), which is robust across projection conventions — including the renderer's reverse-Z
/// infinite-far perspective — with no fragile plane extraction.
#[derive(Clone, Copy, Debug)]
pub struct Frustum {
    view_proj: Mat4,
}

impl Frustum {
    /// Wrap a `view_proj` matrix (world → clip).
    #[inline]
    pub fn from_view_proj(view_proj: Mat4) -> Self {
        Self { view_proj }
    }

    /// Conservative visibility of a world-space AABB.
    ///
    /// Returns `true` unless the box is provably fully outside ONE clip boundary (all 8 corners past
    /// the same left/right/top/bottom edge) or fully behind the camera (all corners with `w <= 0`).
    /// A corner behind the near plane (`w <= 0`) disables the side tests, so a box straddling the
    /// camera plane is kept — never a false "culled" for something on screen.
    pub fn intersects_world_aabb(&self, min: Vec3, max: Vec3) -> bool {
        let (mut all_left, mut all_right) = (true, true);
        let (mut all_below, mut all_above) = (true, true);
        let mut all_behind = true;

        for i in 0..8u8 {
            let corner = Vec3::new(
                if i & 1 == 0 { min.x } else { max.x },
                if i & 2 == 0 { min.y } else { max.y },
                if i & 4 == 0 { min.z } else { max.z },
            );
            let c = self.view_proj * corner.extend(1.0);
            let front = c.w > 0.0;
            if front {
                all_behind = false;
            }
            // Only a corner in FRONT can sit outside a side; a behind/near corner (`!front`) makes
            // that side's flag false, disabling the cull for this box (conservative).
            all_left &= front && c.x < -c.w;
            all_right &= front && c.x > c.w;
            all_below &= front && c.y < -c.w;
            all_above &= front && c.y > c.w;
        }

        !(all_behind || all_left || all_right || all_below || all_above)
    }
}

/// World-space AABB of a node card: its four ROTATED corners on the plane (via [`LayoutNode::xform`]),
/// optionally extruded `±z_extrude` along the plane normal for the 3D-mesh backend.
fn node_world_aabb(ln: &LayoutNode, plane: &GraphPlane, z_extrude: f32) -> (Vec3, Vec3) {
    let hw = ln.size[0] * 0.5;
    let hh = ln.size[1] * 0.5;
    let xf = ln.xform();
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        let uv = xf.point([ln.center[0] + sx * hw, ln.center[1] + sy * hh]);
        let w = plane.graph_to_world(uv[0], uv[1]);
        for p in [w + plane.normal * z_extrude, w - plane.normal * z_extrude] {
            lo = lo.min(p);
            hi = hi.max(p);
        }
    }
    (lo, hi)
}

/// Pick a [`NodeLod`] from the node's projected on-screen HEIGHT (the limiting readable dimension —
/// cards are wider than tall). Defaults to [`NodeLod::Full`] if projection is degenerate.
fn node_lod(ln: &LayoutNode, camera: &Camera, viewport: Vec2, cfg: &VisibilityConfig) -> NodeLod {
    let Some(px_h) = node_projected_height_px(ln, camera, viewport) else {
        return NodeLod::Full;
    };
    if px_h < cfg.lod_body_px {
        NodeLod::BodyOnly
    } else if px_h < cfg.lod_no_text_px {
        NodeLod::NoText
    } else {
        NodeLod::Full
    }
}

/// Projected pixel span between the node's top and bottom edge centers (its on-screen height),
/// following the card rotation. `None` if either sample projects behind the camera.
fn node_projected_height_px(ln: &LayoutNode, camera: &Camera, viewport: Vec2) -> Option<f32> {
    let hh = ln.size[1] * 0.5;
    let xf = ln.xform();
    let top = xf.point([ln.center[0], ln.center[1] - hh]);
    let bot = xf.point([ln.center[0], ln.center[1] + hh]);
    let s_top = camera.graph_to_screen(Vec2::new(top[0], top[1]), viewport.x, viewport.y);
    let s_bot = camera.graph_to_screen(Vec2::new(bot[0], bot[1]), viewport.x, viewport.y);
    let d = s_top - s_bot;
    let len = d.length();
    if len.is_finite() { Some(len) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nodes_core::LayoutNode;

    fn node_at(center: [f32; 2], size: [f32; 2]) -> LayoutNode {
        LayoutNode {
            center,
            size,
            rotation: 0.0,
        }
    }

    fn layout_with(nodes: &[(NodeId, LayoutNode)]) -> LayoutCache {
        let mut c = LayoutCache::default();
        for (id, n) in nodes {
            c.nodes.insert(*id, n.clone());
        }
        c
    }

    #[test]
    fn culling_disabled_reports_all_visible_full() {
        let id = NodeId::default();
        let layout = layout_with(&[(id, node_at([9.9e5, 0.0], [160.0, 90.0]))]);
        let cam = Camera::default();
        let cfg = VisibilityConfig {
            cull: false,
            ..Default::default()
        };
        let vis = compute_visible_nodes(&layout, &cam, Vec2::new(800.0, 600.0), cfg);
        assert!(vis.is_visible(id));
        assert_eq!(vis.lod(id), NodeLod::Full);
        assert!(!vis.culling());
    }

    #[test]
    fn on_screen_node_is_visible() {
        let id = NodeId::default();
        let layout = layout_with(&[(id, node_at([0.0, 0.0], [160.0, 90.0]))]);
        let mut cam = Camera::default();
        let vp = Vec2::new(800.0, 600.0);
        cam.reset_default_zoom(vp);
        let vis = compute_visible_nodes(&layout, &cam, vp, VisibilityConfig::default());
        assert!(vis.is_visible(id), "node at origin must be on screen");
    }

    #[test]
    fn far_offscreen_node_is_culled() {
        let id = NodeId::default();
        // Far to the right in graph +u: well outside the viewport at default zoom.
        let layout = layout_with(&[(id, node_at([5.0e5, 0.0], [160.0, 90.0]))]);
        let mut cam = Camera::default();
        let vp = Vec2::new(800.0, 600.0);
        cam.reset_default_zoom(vp);
        let vis = compute_visible_nodes(&layout, &cam, vp, VisibilityConfig::default());
        assert!(!vis.is_visible(id), "node far off +u must be culled");
    }

    #[test]
    fn far_offscreen_node_stays_culled_when_orbited() {
        // The failure mode of the retired uv-rect cull: grazing angles must NOT resurrect a
        // clearly-offscreen node, nor cull the on-screen one.
        let near = NodeId::default();
        let far = NodeId::default();
        // NodeId::default() collides, so build two distinct ids via a slotmap-free trick: reuse
        // the same map with one entry each in separate layouts.
        let mut cam = Camera::default();
        let vp = Vec2::new(800.0, 600.0);
        cam.reset_default_zoom(vp);
        cam.orbit(0.9, 0.5);

        let near_layout = layout_with(&[(near, node_at([0.0, 0.0], [160.0, 90.0]))]);
        let far_layout = layout_with(&[(far, node_at([5.0e5, 0.0], [160.0, 90.0]))]);
        let near_vis = compute_visible_nodes(&near_layout, &cam, vp, VisibilityConfig::default());
        let far_vis = compute_visible_nodes(&far_layout, &cam, vp, VisibilityConfig::default());
        assert!(
            near_vis.is_visible(near),
            "on-screen node visible under orbit"
        );
        assert!(
            !far_vis.is_visible(far),
            "offscreen node stays culled under orbit"
        );
    }

    #[test]
    fn frustum_behind_camera_box_is_culled() {
        // A box fully behind the eye (very negative z, camera looks toward +z) is culled.
        let cam = Camera::default();
        let vp = cam.view_proj(800.0, 600.0);
        let f = Frustum::from_view_proj(vp);
        // Camera default eye is in front of z=0 plane looking +z; place a box far behind it.
        let behind = f.intersects_world_aabb(
            Vec3::new(-1.0, -1.0, -1.0e6),
            Vec3::new(1.0, 1.0, -1.0e6 + 2.0),
        );
        assert!(!behind, "box far behind the camera must be culled");
    }
}
