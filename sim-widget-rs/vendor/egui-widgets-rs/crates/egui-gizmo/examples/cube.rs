//! On-screen proof for `egui-gizmo`: a fully configurable translate / rotate /
//! scale gizmo over a wireframe cube, with an orbit camera and a control panel.
//!
//! Run: `cargo run --example cube` (from `crates/egui-gizmo/`).
//!
//! - The **left panel** is the primary control: 3D/2D + Local/Global, per-element
//!   sizes, a **Tool** selector and the granular handle / axis **refinements**.
//! - **Q/W/E/R/T** pick a tool (move / move / rotate / scale / universal),
//!   **Tab** cycles tools (skipping ones unusable in the current space),
//!   **2/3** switch 2D/3D space, **X** toggles Local/Global.
//! - **`-`/`=`** nudge `gizmo_size`; with **Shift** stroke width, **Ctrl** ring
//!   scale, **Alt** arrow scale.
//! - Drag a handle to transform the cube; drag the empty viewport to orbit.
//!   While a handle is grabbed, `Gizmo::is_hovered()` suppresses the orbit.

use eframe::egui;
use egui::{Color32, Key, Pos2, Rect, Slider, Stroke};
use egui_gizmo::{
    CenterShape, Gizmo, GizmoConfig, GizmoFeatures, GizmoOrientation, GizmoResultKind, GizmoSpace,
    GizmoTool, GizmoVisuals, ShearAxis, Transform,
};
use glam::{Mat4, Vec2, Vec3};
// glam 0.33 moved the camera matrices out of `Mat4`'s inherent methods and split
// them by NDC convention. `directx` is the [0, 1] depth range, Y-up — the same
// one the old `Mat4::*_rh` produced, and the one wgpu wants.
use glam::camera::rh::{proj::directx as proj, view as view_rh};

struct CubeApp {
    gizmo: Gizmo,
    transform: Transform,
    /// 3D vs 2D projection space (2D hides the Z-axis handles).
    space: GizmoSpace,
    /// The active exclusive tool (move / rotate / scale / universal / box /
    /// shear / corner-pin). The single source of truth for the handle family.
    tool: GizmoTool,
    /// Refinements that narrow the active tool's handle set (handle-types +
    /// per-axis masks).
    features: GizmoFeatures,
    /// Visual style edited live by the panel (sizes, stroke, per-element scales).
    visuals: GizmoVisuals,
    /// `true` = Local (gizmo follows the object's rotation), `false` = Global.
    local: bool,
    /// 2D shear factors (x, y) applied to the demo quad. Driven by the shear
    /// handles' absolute-from-start amount; only meaningful in 2D space.
    shear: Vec2,
    /// 2D corner-pin corners (SW, SE, NE, NW in frame space, +Y up). `None` until
    /// the first corner-pin drag; once `Some`, the quad is drawn warped through
    /// these corners (the demo's stand-in "homography"). Only meaningful in 2D.
    corner_pin: Option<[Vec2; 4]>,
    // Orbit camera state.
    yaw: f32,
    pitch: f32,
    dist: f32,
    /// Orbit pivot = the selection centre, but FROZEN while a handle is grabbed so
    /// the camera doesn't chase the object mid-Move (chasing shifts the drag's
    /// frozen start point on screen → the delta runs away). Re-synced to
    /// `transform.translation` only when not manipulating.
    cam_target: Vec3,
}

impl Default for CubeApp {
    fn default() -> Self {
        Self {
            gizmo: Gizmo::new(GizmoConfig::default()),
            transform: Transform::default(),
            space: GizmoSpace::ThreeD,
            tool: GizmoTool::Move,
            features: GizmoFeatures::all(),
            visuals: GizmoVisuals::default(),
            local: true,
            shear: Vec2::ZERO,
            corner_pin: None,
            yaw: 0.7,
            pitch: 0.5,
            dist: 6.0,
            cam_target: Vec3::ZERO,
        }
    }
}

impl eframe::App for CubeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.handle_keys(ui);
        self.controls_panel(ui);

        egui::CentralPanel::default().show(ui, |ui| {
            self.viewport(ui);
        });
    }
}

impl CubeApp {
    /// Quick keyboard shortcuts (the panel is the primary control).
    ///
    /// Tool keys (Q/W/E/R/T) and space keys (2/3) plus `Tab`-cycle. Any change to
    /// `(tool, space)` clears the gizmo-owned 2D state (`reset_2d_state`) so a
    /// stale shear / corner-pin never leaks into the new tool.
    fn handle_keys(&mut self, ui: &egui::Ui) {
        // Snapshot the tool/space so we can detect a change after the closure and
        // reset the 2D state exactly once (mirrors the panel's own guard).
        let prev = (self.tool, self.space);

        ui.input(|i| {
            // --- Space (2D / 3D) ---
            if i.key_pressed(Key::Num2) {
                self.space = GizmoSpace::TwoD;
            }
            if i.key_pressed(Key::Num3) {
                self.space = GizmoSpace::ThreeD;
            }

            // --- Tool presets. Q defaults to Move (the "home" tool). ---
            if i.key_pressed(Key::Q) {
                self.tool = GizmoTool::Move;
            }
            if i.key_pressed(Key::W) {
                self.tool = GizmoTool::Move;
            }
            if i.key_pressed(Key::E) {
                self.tool = GizmoTool::Rotate;
            }
            if i.key_pressed(Key::R) {
                self.tool = GizmoTool::Scale;
            }
            if i.key_pressed(Key::T) {
                self.tool = GizmoTool::Universal;
            }

            // --- Tab cycles to the next tool usable in the current space. ---
            // egui sometimes consumes `Tab` for focus navigation, so
            // `key_pressed(Tab)` can miss; fall back to scanning the raw event
            // stream for a Tab key-press (the gizmo demo has no focusable widgets
            // competing for it in the viewport).
            let tab = i.key_pressed(Key::Tab)
                || i.events.iter().any(|e| {
                    matches!(
                        e,
                        egui::Event::Key {
                            key: Key::Tab,
                            pressed: true,
                            ..
                        }
                    )
                });
            if tab {
                self.tool = next_tool(self.tool, self.space);
            }

            if i.key_pressed(Key::X) {
                self.local = !self.local;
            }

            // `-`/`=` nudge a size; modifiers pick which one.
            let down = i.key_pressed(Key::Minus);
            let up = i.key_pressed(Key::Equals) || i.key_pressed(Key::Plus);
            if down || up {
                let step = if up { 1.0 } else { -1.0 };
                if i.modifiers.shift {
                    self.visuals.stroke_width =
                        (self.visuals.stroke_width + step * 0.5).clamp(0.5, 6.0);
                } else if i.modifiers.ctrl {
                    self.visuals.ring_scale =
                        (self.visuals.ring_scale + step * 0.1).clamp(0.2, 3.0);
                } else if i.modifiers.alt {
                    self.visuals.arrow_scale =
                        (self.visuals.arrow_scale + step * 0.1).clamp(0.2, 3.0);
                } else {
                    self.visuals.gizmo_size =
                        (self.visuals.gizmo_size + step * 8.0).clamp(24.0, 200.0);
                }
            }
        });

        // If a key changed the tool or space, drop the gizmo-owned 2D state so
        // the new tool starts clean (the gizmo itself also resets via
        // `modes_changed`; this keeps the demo's own copies in sync).
        if (self.tool, self.space) != prev {
            self.reset_2d_state();
        }
    }

    /// Reset the demo's 2D-only state (shear + corner-pin) to identity.
    ///
    /// Called whenever the tool or space changes. The gizmo-owned shear /
    /// corner-pin reset themselves inside `Gizmo::update_config` (their handle
    /// set rebuilds via `modes_changed`); this clears the DEMO's mirror copies
    /// that drive the quad's drawing so the subject snaps back too.
    fn reset_2d_state(&mut self) {
        // Clamp to a supported tool if the SPACE change just made the active tool
        // invalid (e.g. on CornerPin, press `3`): CornerPin has no 3D form, so
        // leaving it active would render an empty, non-interactive gizmo. `next_tool`
        // lands on the next supported tool (Move always supports both spaces).
        if !self.tool.supports(self.space) {
            self.tool = next_tool(self.tool, self.space);
        }
        self.shear = Vec2::ZERO;
        self.corner_pin = None;
    }

    /// Left control panel: everything aligned in two-column grids.
    fn controls_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("controls")
            .resizable(true)
            .default_size(220.0)
            .show(ui, |ui| {
                // All controls live in a vertical scroll area so the lower sections
                // (Center / Opacity) stay reachable at any window height.
                egui::ScrollArea::vertical().show(ui, |ui| {
                    // Uniform slider width so every control lines up in column 2.
                    ui.spacing_mut().slider_width = 110.0;

                    // Snapshot tool/space at the TOP of the panel so any control
                    // below (the Space selector or the Tool list) that changes either
                    // triggers the single 2D-state reset at the panel's end.
                    let prev = (self.tool, self.space);

                    ui.heading("egui-gizmo");
                    // Maya middle-mouse hint (the feature has no on-screen handle).
                    ui.label("MMB = screen move · Shift+MMB = axis move");
                    ui.add_space(4.0);

                    ui.strong("Mode");
                    egui::Grid::new("grid_mode")
                        .num_columns(2)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("Space");
                            ui.horizontal(|ui| {
                                ui.selectable_value(&mut self.space, GizmoSpace::ThreeD, "3D");
                                ui.selectable_value(&mut self.space, GizmoSpace::TwoD, "2D");
                            });
                            ui.end_row();

                            ui.label("Axes");
                            ui.horizontal(|ui| {
                                ui.selectable_value(&mut self.local, true, "Local");
                                ui.selectable_value(&mut self.local, false, "Global");
                            });
                            ui.end_row();
                        });

                    ui.separator();
                    ui.strong("Sizes");
                    egui::Grid::new("grid_sizes")
                        .num_columns(2)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("gizmo_size");
                            ui.add(Slider::new(&mut self.visuals.gizmo_size, 24.0..=200.0));
                            ui.end_row();
                            ui.label("stroke_width");
                            ui.add(Slider::new(&mut self.visuals.stroke_width, 0.5..=6.0));
                            ui.end_row();
                            ui.label("arrow_scale");
                            ui.add(Slider::new(&mut self.visuals.arrow_scale, 0.2..=3.0));
                            ui.end_row();
                            ui.label("plane_scale");
                            ui.add(Slider::new(&mut self.visuals.plane_scale, 0.2..=3.0));
                            ui.end_row();
                            ui.label("ring_scale");
                            ui.add(Slider::new(&mut self.visuals.ring_scale, 0.2..=3.0));
                            ui.end_row();
                            ui.label("scale_box_scale");
                            ui.add(Slider::new(&mut self.visuals.scale_box_scale, 0.2..=3.0));
                            ui.end_row();
                            ui.label("bbox_handle");
                            ui.add(Slider::new(&mut self.visuals.bbox_handle_size, 4.0..=24.0));
                            ui.end_row();
                        });

                    // Exclusive Tool selector. Each row is wrapped in an enabled-guard
                    // so tools unusable in the current space (CornerPin in 3D) render
                    // greyed-out and unclickable.
                    ui.separator();
                    ui.strong("Tool");
                    for tool in GizmoTool::ALL {
                        ui.add_enabled_ui(tool.supports(self.space), |ui| {
                            ui.selectable_value(&mut self.tool, tool, tool_label(tool));
                        });
                    }

                    // Orthogonal refinements that narrow whatever tool is active.
                    // Checkboxes use the native [box | label] form so every box
                    // left-aligns at the same x across ALL groups.
                    ui.separator();
                    ui.strong("Refinements");
                    ui.checkbox(&mut self.features.axes, "axes");
                    ui.checkbox(&mut self.features.planes, "planes");
                    ui.checkbox(&mut self.features.screen, "screen");
                    ui.checkbox(&mut self.features.uniform_scale, "uniform scale");
                    ui.checkbox(&mut self.features.arcball, "arcball");

                    ui.separator();
                    ui.strong("Axes");
                    ui.checkbox(&mut self.features.axis_x, "X");
                    ui.checkbox(&mut self.features.axis_y, "Y");
                    ui.checkbox(&mut self.features.axis_z, "Z");

                    // A tool/space change drops the demo's 2D state so the new tool
                    // starts from identity (the gizmo resets its own copy too).
                    if (self.tool, self.space) != prev {
                        self.reset_2d_state();
                    }

                    ui.separator();
                    ui.strong("Center");
                    ui.horizontal(|ui| {
                        ui.selectable_value(
                            &mut self.visuals.center_shape,
                            CenterShape::Ring,
                            "Ring",
                        );
                        ui.selectable_value(
                            &mut self.visuals.center_shape,
                            CenterShape::Box,
                            "Box",
                        );
                    });
                    egui::Grid::new("grid_center")
                        .num_columns(2)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("size");
                            ui.add(Slider::new(&mut self.visuals.center_scale, 0.2..=2.0));
                            ui.end_row();
                            ui.label("stroke");
                            ui.add(Slider::new(
                                &mut self.visuals.center_stroke_width,
                                0.5..=8.0,
                            ));
                            ui.end_row();
                            ui.label("color");
                            egui_colorpicker::color_button_srgba(
                                ui,
                                &mut self.visuals.center_color,
                            );
                            ui.end_row();
                        });

                    ui.separator();
                    ui.strong("Opacity");
                    egui::Grid::new("grid_opacity")
                        .num_columns(2)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("translate");
                            ui.add(Slider::new(&mut self.visuals.translate_alpha, 0.0..=1.0));
                            ui.end_row();
                            ui.label("rotate");
                            ui.add(Slider::new(&mut self.visuals.rotate_alpha, 0.0..=1.0));
                            ui.end_row();
                            ui.label("scale");
                            ui.add(Slider::new(&mut self.visuals.scale_alpha, 0.0..=1.0));
                            ui.end_row();
                            ui.label("center");
                            ui.add(Slider::new(&mut self.visuals.center_alpha, 0.0..=1.0));
                            ui.end_row();
                        });

                    ui.separator();
                    ui.strong("HUD");
                    ui.checkbox(&mut self.visuals.show_drag_hud, "show drag HUD");
                    ui.checkbox(&mut self.visuals.hud_readout, "readout");
                    ui.checkbox(&mut self.visuals.dial_ticks, "dial ticks");
                    ui.checkbox(&mut self.visuals.axis_guides, "axis guides");
                    egui::Grid::new("grid_hud")
                        .num_columns(2)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("font");
                            ui.add(Slider::new(&mut self.visuals.hud_font_size, 6.0..=20.0));
                            ui.end_row();
                            // srgba picker exposes the alpha channel → HUD text transparency.
                            ui.label("text color");
                            egui_colorpicker::color_button_srgba(
                                ui,
                                &mut self.visuals.hud_text_color,
                            );
                            ui.end_row();
                        });
                }); // close the ScrollArea
            });
    }

    /// Central 3D viewport: orbit camera, wireframe cube, the gizmo + a readout.
    fn viewport(&mut self, ui: &mut egui::Ui) {
        let t = self.transform;
        ui.label(format!(
            "T {:?}   R {:?}   S {:?}",
            round3(t.translation.to_array()),
            round3(t.rotation.to_array()),
            round3(t.scale.to_array()),
        ));
        ui.separator();

        let rect = ui.available_rect_before_wrap();

        // Orbit on a left-drag over the viewport — UNLESS the gizmo is grabbed.
        // We read the RAW pointer delta rather than allocating a competing
        // `ui.interact` region (the gizmo's own hover-interact would otherwise
        // eat the background drag).
        let over_vp = ui
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|p| rect.contains(p));
        if over_vp && ui.input(|i| i.pointer.primary_down()) && !self.gizmo.is_hovered() {
            let d = ui.input(|i| i.pointer.delta());
            self.yaw -= d.x * 0.01;
            self.pitch = (self.pitch - d.y * 0.01).clamp(-1.5, 1.5);
        }

        // Camera matrices (glam f32, column-major — exactly what the facade wants).
        // The camera ORBITS the selection centre (`target`), so tumbling spins
        // the object in place and the gizmo (which sits at the same centre) never
        // drifts off screen. 2D uses an orthographic projection down -Z.
        let aspect = rect.width() / rect.height().max(1.0);
        // Re-sync the orbit pivot to the selection ONLY when no handle is grabbed.
        // Freezing it during a drag stops the camera chasing a moving object — which
        // would shift the drag's frozen start point on screen and make Move (and any
        // translation-changing tool) run away after the first frame.
        if !self.gizmo.is_hovered() {
            self.cam_target = self.transform.translation;
        }
        let target = self.cam_target;
        let (view, projection) = match self.space {
            GizmoSpace::ThreeD => {
                let dir = Vec3::new(
                    self.yaw.cos() * self.pitch.cos(),
                    self.pitch.sin(),
                    self.yaw.sin() * self.pitch.cos(),
                );
                let eye = target + dir * self.dist;
                let view = view_rh::look_at_mat4(eye, target, Vec3::Y);
                let projection = proj::perspective(60f32.to_radians(), aspect, 0.1, 100.0);
                (view, projection)
            }
            GizmoSpace::TwoD => {
                // Look straight down -Z at the selection centre (still ortho).
                let eye = target + Vec3::new(0.0, 0.0, 5.0);
                let view = view_rh::look_at_mat4(eye, target, Vec3::Y);
                let half = 2.0;
                let projection =
                    proj::orthographic(-half * aspect, half * aspect, -half, half, 0.1, 100.0);
                (view, projection)
            }
        };

        // Subject: a wireframe cube in 3D, a flat quad (image stand-in) in 2D.
        // (The 2D shear / corner-pin state is reset by `reset_2d_state` on every
        // tool/space change, so no per-frame clearing is needed here.)
        match self.space {
            GizmoSpace::ThreeD => {
                draw_cube(ui, rect, projection * view, &self.transform);
            }
            GizmoSpace::TwoD => draw_quad(
                ui,
                rect,
                projection * view,
                &self.transform,
                self.shear,
                self.corner_pin,
            ),
        }

        // Drive the gizmo from the live panel state.
        self.gizmo.update_config(GizmoConfig {
            view,
            projection,
            viewport: rect,
            space: self.space,
            tool: self.tool,
            features: self.features,
            orientation: if self.local {
                GizmoOrientation::Local
            } else {
                GizmoOrientation::Global
            },
            visuals: self.visuals,
            // Match the 2D quad's ±0.8 corners so the bbox grips sit on its edges.
            bbox_half_extent: Vec2::splat(0.8),
            ..Default::default()
        });
        if let Some((result, new_transforms)) = self.gizmo.interact(ui, &[self.transform]) {
            match result.kind {
                // TRS path: the interaction is fully described by the returned
                // transform — apply it (shear is left untouched).
                GizmoResultKind::Transform => {
                    if let Some(t) = new_transforms.into_iter().next() {
                        self.transform = t;
                    }
                }
                // Shear path: `amount` is the ABSOLUTE shear factor (the gizmo
                // reports `base + drift`, so a re-grab continues from the current
                // value), so we REPLACE the matching factor rather than accumulate.
                GizmoResultKind::Shear { axis, amount } => match axis {
                    ShearAxis::X => self.shear.x = amount,
                    ShearAxis::Y => self.shear.y = amount,
                },
                // Corner-pin path: the four corner positions ARE the warp (a
                // homography the demo applies by drawing the quad through them).
                GizmoResultKind::CornerPin { corners } => self.corner_pin = Some(corners),
            }
        }
    }
}

/// Project a world point to screen space; `None` when behind the camera.
fn project(mvp: Mat4, rect: Rect, p: Vec3) -> Option<Pos2> {
    let clip = mvp * p.extend(1.0);
    if clip.w <= 1e-6 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    Some(Pos2::new(
        rect.center().x + ndc.x * rect.width() * 0.5,
        rect.center().y - ndc.y * rect.height() * 0.5, // egui Y is down
    ))
}

/// Draw the unit cube under `transform` as a wireframe.
fn draw_cube(ui: &egui::Ui, rect: Rect, view_proj: Mat4, transform: &Transform) {
    let model = Mat4::from_scale_rotation_translation(
        transform.scale,
        transform.rotation,
        transform.translation,
    );
    let mvp = view_proj * model;
    let c = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
    let corners = [
        c(-0.5, -0.5, -0.5),
        c(0.5, -0.5, -0.5),
        c(-0.5, 0.5, -0.5),
        c(0.5, 0.5, -0.5),
        c(-0.5, -0.5, 0.5),
        c(0.5, -0.5, 0.5),
        c(-0.5, 0.5, 0.5),
        c(0.5, 0.5, 0.5),
    ];
    const EDGES: [(usize, usize); 12] = [
        (0, 1),
        (1, 3),
        (3, 2),
        (2, 0), // back face
        (4, 5),
        (5, 7),
        (7, 6),
        (6, 4), // front face
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7), // connectors
    ];
    let stroke = Stroke::new(1.0, Color32::from_gray(150));
    for (a, b) in EDGES {
        if let (Some(pa), Some(pb)) = (
            project(mvp, rect, corners[a]),
            project(mvp, rect, corners[b]),
        ) {
            ui.painter().line_segment([pa, pb], stroke);
        }
    }
}

/// Draw a flat quad (image stand-in) under `transform` in the z=0 plane — the 2D
/// mode's subject. Filled translucent + outlined so the transform reads clearly.
///
/// `shear` is the 2D shear factor (x, y) reported by the shear handles; it is applied
/// to the LOCAL corners about the quad centre (= local origin) BEFORE the model
/// matrix: `x' = x + shear.x * y; y' = y + shear.y * x`. Zero shear leaves the quad
/// untouched (the original path).
///
/// `corner_pin` (when `Some`) is the demo's stand-in "homography": the four corner
/// positions (SW, SE, NE, NW) live directly in frame space (world XY at z=0), so
/// they are projected with `view_proj` ALONE — no model/shear path — and the quad
/// is drawn straight through them.
fn draw_quad(
    ui: &egui::Ui,
    rect: Rect,
    view_proj: Mat4,
    transform: &Transform,
    shear: Vec2,
    corner_pin: Option<[Vec2; 4]>,
) {
    let corners = if let Some(cp) = corner_pin {
        // Corner-pin warp: corners are already in frame/world space at z=0.
        cp.map(|c| Vec3::new(c.x, c.y, 0.0))
    } else {
        let model = Mat4::from_scale_rotation_translation(
            transform.scale,
            transform.rotation,
            transform.translation,
        );
        // Apply the 2×2 shear to each local corner about the local origin, then the
        // model matrix, to get the world-space corner positions.
        let sh = |c: Vec3| Vec3::new(c.x + shear.x * c.y, c.y + shear.y * c.x, c.z);
        [
            model.transform_point3(sh(Vec3::new(-0.8, -0.8, 0.0))),
            model.transform_point3(sh(Vec3::new(0.8, -0.8, 0.0))),
            model.transform_point3(sh(Vec3::new(0.8, 0.8, 0.0))),
            model.transform_point3(sh(Vec3::new(-0.8, 0.8, 0.0))),
        ]
    };
    let pts: Vec<Pos2> = corners
        .iter()
        .filter_map(|&c| project(view_proj, rect, c))
        .collect();
    if pts.len() == 4 {
        ui.painter().add(egui::Shape::convex_polygon(
            pts,
            Color32::from_rgba_unmultiplied(80, 120, 170, 60),
            Stroke::new(1.5, Color32::from_gray(170)),
        ));
    }
}

fn round3<const N: usize>(a: [f32; N]) -> [f32; N] {
    a.map(|v| (v * 100.0).round() / 100.0)
}

/// Human-readable label for a tool (used by the panel selector).
fn tool_label(tool: GizmoTool) -> &'static str {
    match tool {
        GizmoTool::Move => "Move",
        GizmoTool::Rotate => "Rotate",
        GizmoTool::Scale => "Scale",
        GizmoTool::Universal => "Universal",
        GizmoTool::Box => "Box",
        GizmoTool::Box3D => "Box 3D",
        GizmoTool::Shear => "Shear",
        GizmoTool::CornerPin => "Corner Pin",
    }
}

/// The next tool in `GizmoTool::ALL` order that is usable in `space`, wrapping
/// around and skipping any tool the space does not support (e.g. CornerPin in
/// 3D). Drives the `Tab` cycle.
fn next_tool(current: GizmoTool, space: GizmoSpace) -> GizmoTool {
    let all = GizmoTool::ALL;
    let start = all.iter().position(|&t| t == current).unwrap_or(0);
    // Scan the ring starting just after `current`; the current tool is always
    // supported (we are on it), so a usable next tool is guaranteed to exist.
    for offset in 1..=all.len() {
        let candidate = all[(start + offset) % all.len()];
        if candidate.supports(space) {
            return candidate;
        }
    }
    current
}

fn main() -> eframe::Result<()> {
    eframe::run_native(
        "egui-gizmo · cube",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([860.0, 920.0]),
            ..Default::default()
        },
        Box::new(|_cc| Ok(Box::new(CubeApp::default()))),
    )
}
