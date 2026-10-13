//! Generic 3-D viewport overlays painted with egui — axis indicator, world-AABB
//! and camera-frustum wireframes, ground-plane grid, camera letterbox mask and
//! reticles. All drawn on top of an already-rendered viewport rect using egui
//! painter primitives. The widget owns NO scene and NO GPU.
//!
//! Gathered from vfx-rs `vfx-view/src/panels/overlays.rs` (referenced usdviewq
//! `stageView.py`). The only host coupling was `usd_gf::{Matrix4d, Vec3d}`,
//! replaced here by the tiny in-crate [`Mat4`] / [`Vec3`] so the crate stays
//! hermetic (egui only).
//!
//! # Matrix convention (preserved verbatim from the source)
//!
//! Row-vector (Imath / OpenUSD): `clip = point * VP`, where `VP = View * Proj`.
//! When projecting a point, dot it with `VP.column(i)` — NOT `VP.row(i)`. The
//! caller therefore passes view / projection as **row-major** [`Mat4`]
//! (`m[row][col]`), exactly how `GfMatrix4d` stores them. NDC -> screen is
//! `egui_gizmo::viewport_matrix`, shared with the gizmo. See `project_to_screen`.

use egui::{Color32, Pos2, Rect, Stroke, Ui};

// ─────────────────────────────────────────────────────────────────────────────
// Minimal linear algebra (row-major, USD/Imath row-vector convention).
//
// This replaces `usd_gf::{Matrix4d, Vec3d}` so the crate depends on egui only.
// The arithmetic matches `GfMatrix4d`/`GfVec3d`: standard matrix multiply,
// row-vector homogeneous `transform_point`, and a cofactor inverse. Verified by
// the unit tests at the bottom of this file.
// ─────────────────────────────────────────────────────────────────────────────

/// A 3-D point / direction in world space (f64, matching USD precision).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    #[inline]
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }
}

/// A 4-component row / column slice of a [`Mat4`] (internal helper).
#[derive(Debug, Clone, Copy)]
struct Vec4 {
    x: f64,
    y: f64,
    z: f64,
    w: f64,
}

/// A 4×4 matrix stored **row-major** (`m[row][col]`), USD/Imath layout.
///
/// Construct from a caller's view / projection with [`Mat4::from_rows`]. The
/// row-vector convention (`clip = point * VP`) is baked into [`Mat4::mul`],
/// [`Mat4::transform_point`] and the projection helpers — pass matrices exactly
/// as `GfMatrix4d` stores them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat4(pub [[f64; 4]; 4]);

impl Mat4 {
    /// Build from a row-major 4×4 array (`m[row][col]`).
    #[inline]
    pub fn from_rows(rows: [[f64; 4]; 4]) -> Self {
        Mat4(rows)
    }

    #[inline]
    fn row(&self, i: usize) -> Vec4 {
        let r = &self.0[i];
        Vec4 {
            x: r[0],
            y: r[1],
            z: r[2],
            w: r[3],
        }
    }

    #[inline]
    fn column(&self, j: usize) -> Vec4 {
        let m = &self.0;
        Vec4 {
            x: m[0][j],
            y: m[1][j],
            z: m[2][j],
            w: m[3][j],
        }
    }

    /// Row-vector homogeneous transform: `[p, 1] * M`, then perspective divide.
    /// Matches `GfMatrix4d::Transform`.
    fn transform_point(&self, p: Vec3) -> Vec3 {
        let m = &self.0;
        let x = p.x * m[0][0] + p.y * m[1][0] + p.z * m[2][0] + m[3][0];
        let y = p.x * m[0][1] + p.y * m[1][1] + p.z * m[2][1] + m[3][1];
        let z = p.x * m[0][2] + p.y * m[1][2] + p.z * m[2][2] + m[3][2];
        let w = p.x * m[0][3] + p.y * m[1][3] + p.z * m[2][3] + m[3][3];
        if w.abs() > 1e-12 {
            Vec3::new(x / w, y / w, z / w)
        } else {
            Vec3::new(x, y, z)
        }
    }

    /// 4×4 inverse via cofactor / adjugate. `None` for a singular matrix.
    fn inverse(&self) -> Option<Mat4> {
        let a = &self.0;
        // Signed 3×3 minor (cofactor) obtained by deleting row `i`, column `j`.
        let cof = |i: usize, j: usize| -> f64 {
            let mut sub = [[0.0f64; 3]; 3];
            let mut r = 0;
            for (ri, arow) in a.iter().enumerate() {
                if ri == i {
                    continue;
                }
                let mut c = 0;
                for (ci, &val) in arow.iter().enumerate() {
                    if ci == j {
                        continue;
                    }
                    sub[r][c] = val;
                    c += 1;
                }
                r += 1;
            }
            let d = sub[0][0] * (sub[1][1] * sub[2][2] - sub[1][2] * sub[2][1])
                - sub[0][1] * (sub[1][0] * sub[2][2] - sub[1][2] * sub[2][0])
                + sub[0][2] * (sub[1][0] * sub[2][1] - sub[1][1] * sub[2][0]);
            if (i + j).is_multiple_of(2) { d } else { -d }
        };

        let mut det = 0.0;
        for (j, &v) in a[0].iter().enumerate() {
            det += v * cof(0, j);
        }
        if det.abs() < 1e-20 {
            return None;
        }
        let inv_det = 1.0 / det;

        let mut inv = [[0.0f64; 4]; 4];
        // adjugate transposes the cofactor matrix: adj[i][j] = C[j][i].
        for (i, row) in inv.iter_mut().enumerate() {
            for (j, slot) in row.iter_mut().enumerate() {
                *slot = cof(j, i) * inv_det;
            }
        }
        Some(Mat4(inv))
    }
}

impl std::ops::Mul for Mat4 {
    type Output = Mat4;
    /// Standard (row-vector consistent) matrix multiply: `(p * A) * B == p * (A*B)`.
    fn mul(self, rhs: Mat4) -> Mat4 {
        let a = &self.0;
        let b = &rhs.0;
        let mut c = [[0.0f64; 4]; 4];
        for i in 0..4 {
            for j in 0..4 {
                c[i][j] =
                    a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j] + a[i][3] * b[3][j];
            }
        }
        Mat4(c)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Overlay colours. The X/Y/Z axis line + label colours come from the shared
// `egui_widgets_config::AxisColors` convention (one source across every 3D
// widget). Mask / reticle colours remain caller parameters so they can match the
// host theme.
// ─────────────────────────────────────────────────────────────────────────────

/// Overlay visibility toggles. Caller-owned (serde-derived for layout persistence).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OverlayState {
    /// Show RGB axis indicator in corner.
    pub show_axes: bool,
    /// Show selection bounding box wireframe.
    pub show_selection_bbox: bool,
    /// Show full scene bounding box wireframe.
    pub show_scene_bbox: bool,
    /// Show camera mask (letterbox) overlay.
    pub show_camera_mask: bool,
    /// Show camera reticles (crosshair / rule-of-thirds).
    pub show_reticles: bool,
    /// Camera aspect ratio for mask/reticle (0 = use viewport aspect).
    pub camera_aspect: f64,
}

impl Default for OverlayState {
    fn default() -> Self {
        Self {
            show_axes: true,
            show_selection_bbox: true,
            show_scene_bbox: false,
            show_camera_mask: false,
            show_reticles: false,
            camera_aspect: 0.0,
        }
    }
}

impl OverlayState {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Draw the axis orientation indicator in the bottom-left corner.
///
/// Projects world XYZ directions through the view matrix rotation
/// (ignoring translation) to get screen-space axis directions.
pub fn draw_axes(ui: &mut Ui, rect: Rect, view_matrix: &Mat4) {
    // Shared axis-colour convention (line colour + derived brighter label).
    let ax = egui_widgets_config::WidgetsConfig::get(ui).axis;
    let painter = ui.painter();
    let axis_len = 30.0_f32;
    let origin = Pos2::new(rect.left() + 40.0, rect.bottom() - 40.0);

    // Extract rotation from view matrix (upper-left 3x3)
    let axes = [
        (
            Vec3::new(1.0, 0.0, 0.0),
            ax.x,
            egui_widgets_config::AxisColors::label(ax.x),
            "X",
        ),
        (
            Vec3::new(0.0, 1.0, 0.0),
            ax.y,
            egui_widgets_config::AxisColors::label(ax.y),
            "Y",
        ),
        (
            Vec3::new(0.0, 0.0, 1.0),
            ax.z,
            egui_widgets_config::AxisColors::label(ax.z),
            "Z",
        ),
    ];

    for (dir, color, label_color, label) in &axes {
        let rotated = transform_direction(view_matrix, dir);
        // Project to screen: x right, y up (egui y is down, so negate)
        let end = Pos2::new(
            origin.x + rotated.x as f32 * axis_len,
            origin.y - rotated.y as f32 * axis_len,
        );
        painter.line_segment([origin, end], Stroke::new(2.0, *color));

        // Label at axis tip
        let label_pos = Pos2::new(
            origin.x + rotated.x as f32 * (axis_len + 10.0),
            origin.y - rotated.y as f32 * (axis_len + 10.0),
        );
        painter.text(
            label_pos,
            egui::Align2::CENTER_CENTER,
            *label,
            egui::FontId::proportional(10.0),
            *label_color,
        );
    }
}

/// Draw bounding box wireframe overlay projected to screen space.
///
/// `bbox_min`/`bbox_max` are world-space AABB corners.
/// Uses view+proj matrices to project the 8 corners, then draws 12 edges.
pub fn draw_bbox(
    ui: &mut Ui,
    rect: Rect,
    bbox_min: Vec3,
    bbox_max: Vec3,
    view_matrix: &Mat4,
    proj_matrix: &Mat4,
    color: Color32,
) {
    let painter = ui.painter();
    let vp = *view_matrix * *proj_matrix;

    // 8 corners of the AABB
    let corners = [
        Vec3::new(bbox_min.x, bbox_min.y, bbox_min.z),
        Vec3::new(bbox_max.x, bbox_min.y, bbox_min.z),
        Vec3::new(bbox_max.x, bbox_max.y, bbox_min.z),
        Vec3::new(bbox_min.x, bbox_max.y, bbox_min.z),
        Vec3::new(bbox_min.x, bbox_min.y, bbox_max.z),
        Vec3::new(bbox_max.x, bbox_min.y, bbox_max.z),
        Vec3::new(bbox_max.x, bbox_max.y, bbox_max.z),
        Vec3::new(bbox_min.x, bbox_max.y, bbox_max.z),
    ];

    // Project corners to screen space
    let screen: Vec<Option<Pos2>> = corners
        .iter()
        .map(|c| project_to_screen(&vp, c, rect))
        .collect();

    // 12 edges of a box (index pairs)
    let edges = [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0), // front face
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 4), // back face
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7), // connecting edges
    ];

    let stroke = Stroke::new(1.0, color);
    for (a, b) in &edges {
        if let (Some(pa), Some(pb)) = (screen[*a], screen[*b]) {
            painter.line_segment([pa, pb], stroke);
        }
    }
}

/// Draw camera mask (letterbox bars) for a given target aspect ratio.
///
/// If `camera_aspect` is wider than viewport, draws top/bottom bars.
/// If narrower, draws left/right bars.
pub fn draw_camera_mask(ui: &mut Ui, rect: Rect, camera_aspect: f64, mask_color: Color32) {
    if camera_aspect <= 0.0 {
        return;
    }

    let painter = ui.painter();
    let vp_aspect = (rect.width() / rect.height().max(1.0)) as f64;

    if camera_aspect > vp_aspect {
        // Camera is wider — letterbox top/bottom
        let visible_h = rect.width() as f64 / camera_aspect;
        let bar_h = ((rect.height() as f64 - visible_h) * 0.5).max(0.0) as f32;
        painter.rect_filled(
            Rect::from_min_size(rect.left_top(), egui::vec2(rect.width(), bar_h)),
            0.0,
            mask_color,
        );
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(rect.left(), rect.bottom() - bar_h),
                egui::vec2(rect.width(), bar_h),
            ),
            0.0,
            mask_color,
        );
    } else {
        // Camera is narrower — pillarbox left/right
        let visible_w = rect.height() as f64 * camera_aspect;
        let bar_w = ((rect.width() as f64 - visible_w) * 0.5).max(0.0) as f32;
        painter.rect_filled(
            Rect::from_min_size(rect.left_top(), egui::vec2(bar_w, rect.height())),
            0.0,
            mask_color,
        );
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(rect.right() - bar_w, rect.top()),
                egui::vec2(bar_w, rect.height()),
            ),
            0.0,
            mask_color,
        );
    }
}

/// Draw camera mask outline (thin border around the masked region).
pub fn draw_camera_mask_outline(ui: &mut Ui, rect: Rect, camera_aspect: f64) {
    if camera_aspect <= 0.0 {
        return;
    }
    let painter = ui.painter();
    let vp_aspect = (rect.width() / rect.height().max(1.0)) as f64;
    let stroke = Stroke::new(1.0, Color32::from_rgb(200, 200, 200));

    if camera_aspect > vp_aspect {
        let visible_h = rect.width() as f64 / camera_aspect;
        let bar_h = ((rect.height() as f64 - visible_h) * 0.5).max(0.0) as f32;
        let inner = Rect::from_min_max(
            Pos2::new(rect.left(), rect.top() + bar_h),
            Pos2::new(rect.right(), rect.bottom() - bar_h),
        );
        painter.rect_stroke(inner, 0.0, stroke, egui::StrokeKind::Outside);
    } else {
        let visible_w = rect.height() as f64 * camera_aspect;
        let bar_w = ((rect.width() as f64 - visible_w) * 0.5).max(0.0) as f32;
        let inner = Rect::from_min_max(
            Pos2::new(rect.left() + bar_w, rect.top()),
            Pos2::new(rect.right() - bar_w, rect.bottom()),
        );
        painter.rect_stroke(inner, 0.0, stroke, egui::StrokeKind::Outside);
    }
}

/// Draw camera reticles (rule-of-thirds grid + center crosshair).
pub fn draw_reticles(ui: &mut Ui, rect: Rect, reticle_color: Color32) {
    let painter = ui.painter();
    let stroke = Stroke::new(1.0, reticle_color);

    // Center crosshair
    let cx = rect.center().x;
    let cy = rect.center().y;
    let cross_size = 10.0;
    painter.line_segment(
        [
            Pos2::new(cx - cross_size, cy),
            Pos2::new(cx + cross_size, cy),
        ],
        stroke,
    );
    painter.line_segment(
        [
            Pos2::new(cx, cy - cross_size),
            Pos2::new(cx, cy + cross_size),
        ],
        stroke,
    );

    // Rule-of-thirds grid (2 horizontal + 2 vertical lines)
    let w = rect.width();
    let h = rect.height();
    for i in 1..=2 {
        let t = i as f32 / 3.0;
        // Vertical
        let x = rect.left() + w * t;
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            stroke,
        );
        // Horizontal
        let y = rect.top() + h * t;
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            stroke,
        );
    }
}

/// Ground-plane grid layout (groups the grid's knobs to keep `draw_grid` lean).
#[derive(Debug, Clone, Copy)]
pub struct GridConfig {
    /// Half-size of the grid in world units (covers `-extent..=extent`).
    pub extent: f64,
    /// Cell size in world units.
    pub spacing: f64,
    /// Draw a thicker major line every N cells (0 = no major lines).
    pub major_every: i32,
    /// `true` = Z-up (XY floor at Z=0); `false` = Y-up (XZ floor at Y=0).
    pub is_z_up: bool,
}

impl Default for GridConfig {
    fn default() -> Self {
        Self {
            extent: 100.0,
            spacing: 1.0,
            major_every: 10,
            is_z_up: false,
        }
    }
}

/// Draw XZ ground-plane grid overlay projected to screen space.
///
/// Grid covers +-`extent` units with `spacing` cell size and `major_every`
/// major-line interval. Axis lines (X=red at Z=0, Z=blue at X=0) are drawn
/// thicker for orientation. Lines fade near the grid edge to avoid hard cutoff.
pub fn draw_grid(
    ui: &mut Ui,
    rect: Rect,
    view_matrix: &Mat4,
    proj_matrix: &Mat4,
    cfg: &GridConfig,
) {
    let GridConfig {
        extent,
        spacing,
        major_every,
        is_z_up,
    } = *cfg;
    let vp = *view_matrix * *proj_matrix;
    let painter = ui.painter();

    let steps = (extent / spacing).round() as i32;
    // Fade zone: lines in the outer 30% of extent fade to transparent
    let fade_start = extent * 0.7;

    for i in -steps..=steps {
        let t = i as f64 * spacing;
        let is_major = major_every > 0 && i % major_every == 0;
        let is_axis = i == 0;

        // Edge fade factor: 1.0 at center, 0.0 at extent boundary
        let abs_t = t.abs();
        let fade = if abs_t > fade_start {
            1.0 - ((abs_t - fade_start) / (extent - fade_start)).clamp(0.0, 1.0)
        } else {
            1.0
        };

        // Grid plane: Y-up → XZ floor (Y=0), Z-up → XY floor (Z=0).
        // First axis lines (parallel to X): at i=0 this IS the X axis (red).
        let (base_color_x, width_x) = if is_axis {
            ([200u8, 60, 60], 2.0_f32)
        } else if is_major {
            ([120, 120, 120], 1.2)
        } else {
            ([80, 80, 80], 0.7)
        };
        let alpha_x = if is_axis {
            220.0
        } else if is_major {
            140.0
        } else {
            100.0
        };
        let a = (alpha_x * fade) as u8;
        let color_x =
            Color32::from_rgba_unmultiplied(base_color_x[0], base_color_x[1], base_color_x[2], a);

        let (p0, p1) = if is_z_up {
            // Z-up: lines along X at Y=t, Z=0
            (Vec3::new(-extent, t, 0.0), Vec3::new(extent, t, 0.0))
        } else {
            // Y-up: lines along X at Z=t, Y=0
            (Vec3::new(-extent, 0.0, t), Vec3::new(extent, 0.0, t))
        };
        if let (Some(pa), Some(pb)) = (
            project_to_screen(&vp, &p0, rect),
            project_to_screen(&vp, &p1, rect),
        ) {
            painter.line_segment([pa, pb], egui::Stroke::new(width_x, color_x));
        }

        // Second axis lines: Y-up → parallel to Z (blue), Z-up → parallel to Y (green).
        let (base_color_z, width_z) = if is_axis {
            if is_z_up {
                ([60u8, 200, 60], 2.0_f32) // Y axis = green for Z-up
            } else {
                ([60u8, 100, 200], 2.0_f32) // Z axis = blue for Y-up
            }
        } else if is_major {
            ([120, 120, 120], 1.2)
        } else {
            ([80, 80, 80], 0.7)
        };
        let alpha_z = if is_axis {
            220.0
        } else if is_major {
            140.0
        } else {
            100.0
        };
        let az = (alpha_z * fade) as u8;
        let color_z =
            Color32::from_rgba_unmultiplied(base_color_z[0], base_color_z[1], base_color_z[2], az);

        let (q0, q1) = if is_z_up {
            // Z-up: lines along Y at X=t, Z=0
            (Vec3::new(t, -extent, 0.0), Vec3::new(t, extent, 0.0))
        } else {
            // Y-up: lines along Z at X=t, Y=0
            (Vec3::new(t, 0.0, -extent), Vec3::new(t, 0.0, extent))
        };
        if let (Some(pc), Some(pd)) = (
            project_to_screen(&vp, &q0, rect),
            project_to_screen(&vp, &q1, rect),
        ) {
            painter.line_segment([pc, pd], egui::Stroke::new(width_z, color_z));
        }
    }
}

/// Choose a bbox line color that contrasts with the given clear/background color.
pub fn contrasting_bbox_color(clear: Color32) -> Color32 {
    let luma = 0.299 * clear.r() as f64 + 0.587 * clear.g() as f64 + 0.114 * clear.b() as f64;
    if luma > 128.0 {
        Color32::from_rgb(40, 40, 40) // Dark lines on bright bg
    } else {
        Color32::from_rgb(200, 200, 50) // Yellow lines on dark bg
    }
}

/// Draw a camera frustum wireframe oracle for a scene camera.
///
/// Projects the 8 corners of the camera frustum (near + far planes) into
/// screen space and draws 12 edges as a wireframe box. Skipped when the
/// viewport is already looking through `cam_view`/`cam_proj` (i.e. the
/// oracle camera itself).
pub fn draw_camera_oracle(
    ui: &mut Ui,
    rect: Rect,
    cam_view: &Mat4,
    cam_proj: &Mat4,
    viewport_view: &Mat4,
    viewport_proj: &Mat4,
) {
    // Compute inverse VP of the scene camera to get frustum corners in world space
    let cam_vp = *cam_view * *cam_proj;
    let inv_vp = match cam_vp.inverse() {
        Some(m) => m,
        None => return,
    };

    // 8 NDC frustum corners: near (z=-1) and far (z=1) planes
    let ndc_corners: [(f64, f64, f64); 8] = [
        (-1.0, -1.0, -1.0),
        (1.0, -1.0, -1.0),
        (1.0, 1.0, -1.0),
        (-1.0, 1.0, -1.0),
        (-1.0, -1.0, 1.0),
        (1.0, -1.0, 1.0),
        (1.0, 1.0, 1.0),
        (-1.0, 1.0, 1.0),
    ];

    // Unproject NDC corners to world space
    let world_corners: Vec<Vec3> = ndc_corners
        .iter()
        .map(|&(x, y, z)| inv_vp.transform_point(Vec3::new(x, y, z)))
        .collect();

    // Project world-space frustum corners through viewport's VP
    let vp = *viewport_view * *viewport_proj;
    let screen: Vec<Option<Pos2>> = world_corners
        .iter()
        .map(|c| project_to_screen(&vp, c, rect))
        .collect();

    // 12 edges of the frustum box
    let edges = [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0), // near plane
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 4), // far plane
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7), // connecting edges
    ];

    let stroke = Stroke::new(1.5, Color32::from_rgb(255, 180, 50));
    let painter = ui.painter();
    for (a, b) in &edges {
        if let (Some(pa), Some(pb)) = (screen[*a], screen[*b]) {
            painter.line_segment([pa, pb], stroke);
        }
    }
}

// --- Internal helpers ---

/// Transform a direction vector by the upper-left 3x3 of a 4x4 matrix.
fn transform_direction(m: &Mat4, dir: &Vec3) -> Vec3 {
    // Use the row accessors (upper-left 3x3), matching the original GfMatrix4d path.
    let r0 = m.row(0);
    let r1 = m.row(1);
    let r2 = m.row(2);
    Vec3::new(
        r0.x * dir.x + r0.y * dir.y + r0.z * dir.z,
        r1.x * dir.x + r1.y * dir.y + r1.z * dir.z,
        r2.x * dir.x + r2.y * dir.y + r2.z * dir.z,
    )
}

/// Project a world-space point to egui screen points of `rect`.
///
/// Clip space comes from the USD row-vector `VP` (`clip = point * VP`, columns not rows); NDC -> screen
/// is the gizmo's own viewport transform ([`egui_gizmo::viewport_matrix`]), so these overlays and the
/// gizmo map NDC to the screen through ONE formula. `None` if the point is behind the camera
/// (`w <= 0`).
fn project_to_screen(vp_matrix: &Mat4, point: &Vec3, rect: Rect) -> Option<Pos2> {
    let c0 = vp_matrix.column(0);
    let c1 = vp_matrix.column(1);
    let c3 = vp_matrix.column(3);

    let clip_x = point.x * c0.x + point.y * c0.y + point.z * c0.z + c0.w;
    let clip_y = point.x * c1.x + point.y * c1.y + point.z * c1.z + c1.w;
    let clip_w = point.x * c3.x + point.y * c3.y + point.z * c3.z + c3.w;

    if clip_w <= 0.0 {
        return None; // Behind camera
    }

    let screen =
        egui_gizmo::viewport_matrix(rect) * glam::DVec3::new(clip_x / clip_w, clip_y / clip_w, 1.0);
    Some(Pos2::new(screen.x as f32, screen.y as f32))
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDENTITY: Mat4 = Mat4([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);

    fn approx_eq(a: &Mat4, b: &Mat4) -> bool {
        for i in 0..4 {
            for j in 0..4 {
                if (a.0[i][j] - b.0[i][j]).abs() > 1e-9 {
                    return false;
                }
            }
        }
        true
    }

    /// NDC corners land on the rect's corners (Y up in NDC, down on screen) and a point behind
    /// the camera is dropped: the one viewport transform shared with the gizmo.
    #[test]
    fn project_to_screen_maps_ndc_onto_the_rect() {
        let rect = Rect::from_min_size(Pos2::new(10.0, 20.0), egui::vec2(200.0, 100.0));
        let identity = Mat4::from_rows([
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]);
        let at =
            |x, y| project_to_screen(&identity, &Vec3::new(x, y, 0.0), rect).expect("in front");
        assert_eq!(at(-1.0, 1.0), rect.left_top());
        assert_eq!(at(1.0, -1.0), rect.right_bottom());
        assert_eq!(at(0.0, 0.0), rect.center());
        // w = -1: behind the camera.
        let flip = Mat4::from_rows([
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, -1.0],
        ]);
        assert!(project_to_screen(&flip, &Vec3::new(0.0, 0.0, 0.0), rect).is_none());
    }

    #[test]
    fn identity_mul_is_identity() {
        assert!(approx_eq(&(IDENTITY * IDENTITY), &IDENTITY));
    }

    #[test]
    fn inverse_of_identity_is_identity() {
        assert!(approx_eq(&IDENTITY.inverse().unwrap(), &IDENTITY));
    }

    #[test]
    fn inverse_round_trip_is_identity() {
        // A non-trivial affine + perspective-ish matrix (row-major).
        let m = Mat4([
            [2.0, 0.5, 0.0, 0.0],
            [0.0, 3.0, 0.0, 0.0],
            [1.0, 0.0, 4.0, 0.0],
            [5.0, -2.0, 1.0, 1.0],
        ]);
        let inv = m.inverse().unwrap();
        assert!(approx_eq(&(m * inv), &IDENTITY));
        assert!(approx_eq(&(inv * m), &IDENTITY));
    }

    #[test]
    fn singular_matrix_has_no_inverse() {
        let zero = Mat4([[0.0; 4]; 4]);
        assert!(zero.inverse().is_none());
    }

    #[test]
    fn transform_point_identity_is_noop() {
        let p = Vec3::new(3.0, -1.5, 7.0);
        let q = IDENTITY.transform_point(p);
        assert!((p.x - q.x).abs() < 1e-9 && (p.y - q.y).abs() < 1e-9 && (p.z - q.z).abs() < 1e-9);
    }
}
