//! RGB XYZ axes at world origin `(0, 0, 0)`.

use super::world_line_pipeline::WorldLineVertex;

/// Fixed world length of each XYZ axis branch from the origin.
///
/// A CONSTANT world size (not a fixed-screen-size heuristic): the axes are a world-space gizmo at
/// the origin, and the old `72px / px_per_world_at_target` estimate was measured at the camera
/// TARGET, not the origin — so when the camera was far/at a grazing angle (origin ≠ target) the
/// estimate blew up and the gizmo ballooned to the 5000-unit clamp. A constant length can never
/// balloon and reads consistently. ~half a default node width (`DEFAULT_NODE_W ≈ 320`).
pub const WORLD_AXES_LEN: f32 = 160.0;

/// Length and colors for [`build_world_axes_vertices`].
#[derive(Debug, Clone, Copy)]
pub struct WorldAxesConfig {
    /// Axis length in world units (each positive branch).
    pub length: f32,
}

impl Default for WorldAxesConfig {
    fn default() -> Self {
        Self {
            length: WORLD_AXES_LEN,
        }
    }
}

const X_COLOR: [f32; 4] = [0.92, 0.28, 0.28, 1.0];
const Y_COLOR: [f32; 4] = [0.32, 0.88, 0.38, 1.0];
const Z_COLOR: [f32; 4] = [0.38, 0.52, 0.96, 1.0];

/// Six vertices (three segments) for +X, +Y, +Z from the origin.
pub fn build_world_axes_vertices(cfg: WorldAxesConfig) -> Vec<WorldLineVertex> {
    let l = cfg.length.max(1.0);
    let o = [0.0_f32, 0.0, 0.0];
    let mut out = Vec::with_capacity(6);
    segment(&mut out, o, [l, 0.0, 0.0], X_COLOR);
    segment(&mut out, o, [0.0, l, 0.0], Y_COLOR);
    segment(&mut out, o, [0.0, 0.0, l], Z_COLOR);
    out
}

fn segment(out: &mut Vec<WorldLineVertex>, a: [f32; 3], b: [f32; 3], color: [f32; 4]) {
    out.push(WorldLineVertex { pos: a, color });
    out.push(WorldLineVertex { pos: b, color });
}
