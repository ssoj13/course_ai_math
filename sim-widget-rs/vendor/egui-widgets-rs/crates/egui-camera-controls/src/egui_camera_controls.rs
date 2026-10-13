//! Map egui viewport input to host-agnostic [`CameraIntent`]s.
//!
//! Two gather functions translate raw egui input into a stream of intents the
//! host applies to its own camera rig (this crate owns no camera):
//! - [`gather_orbit_intents`] — LMB orbit (suppressible), MMB pan, RMB dolly,
//!   wheel zoom, plus release-inertia seeds.
//! - [`gather_fly_intents`] — fly mode: WASD/RF thrust, mouse-look or deflection
//!   steering, Q/E roll, Shift boost.
//!
//! (Gathered from gitnexus-rs's `cam-controls-egui`; the `cam_controls`
//! intent/settings types were inlined and the `render_present` pointer helpers
//! replaced with a local one, so the crate depends only on `egui` + `serde`.)

use egui::{Pos2, Response, Ui};
use serde::{Deserialize, Serialize};

// ─── intents ─────────────────────────────────────────────────────────────────

/// A high-level, host-agnostic camera manipulation request. The host maps these
/// onto its own rig (orbit / fly / spaceflight); a rig ignores the intents that
/// do not apply to it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CameraIntent {
    Orbit {
        yaw_delta: f32,
        pitch_delta: f32,
    },
    /// Coast seed emitted on LMB release (host integrates the decay).
    OrbitInertia {
        yaw_delta: f32,
        pitch_delta: f32,
    },
    Pan {
        dx_px: f32,
        dy_px: f32,
    },
    PanInertia {
        dx_px: f32,
        dy_px: f32,
    },
    Zoom {
        factor: f32,
    },
    ZoomAtCursor {
        cursor_x: f32,
        cursor_y: f32,
        factor: f32,
    },
    /// Per-frame mouse-look delta (radians) for fly backends.
    Look {
        dyaw: f32,
        dpitch: f32,
    },
    /// Held deflection-steer RATE axes (∈ [−1,1]) for "космосим" steering.
    LookRate {
        yaw: f32,
        pitch: f32,
    },
    /// Held roll axis (Q/E) for the spaceflight backend.
    Roll {
        d: f32,
    },
    /// Body-relative held thrust axes (WASD / RF).
    Thrust {
        forward: f32,
        right: f32,
        up: f32,
    },
    /// Speed-boost modifier edge (Shift).
    Boost(bool),
}

// ─── settings ────────────────────────────────────────────────────────────────

/// How wheel / dolly input maps to a multiplicative distance factor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoomMode {
    /// `factor ≈ 1 - scroll * linear_rate` (clamped).
    Linear,
    /// `factor ≈ exp(-scroll * exponential_rate)` (clamped). The default — a
    /// steadier relative zoom.
    #[default]
    Exponential,
}

/// Tuning for orbit navigation (the input side only — sensitivities + zoom mode +
/// axis inversion). Persistable via serde.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct OrbitSettings {
    pub zoom_mode: ZoomMode,
    /// Wheel zooms toward the cursor when `true`; toward the target when `false`.
    pub zoom_to_cursor: bool,
    pub orbit_sensitivity: f32,
    /// Per pixel of scroll when [`ZoomMode::Linear`].
    pub zoom_scroll_linear: f32,
    /// Per pixel of scroll when [`ZoomMode::Exponential`]: `exp(-scroll * rate)`.
    pub zoom_scroll_exponential: f32,
    /// RMB dolly sensitivity; uses the same [`ZoomMode`] as the wheel.
    pub dolly_sensitivity: f32,
    /// Multiplier on the last orbit/pan frame at release (inertia seed).
    pub orbit_release_inertia_scale: f32,
    pub invert_orbit_x: bool,
    pub invert_orbit_y: bool,
}

impl Default for OrbitSettings {
    fn default() -> Self {
        Self {
            zoom_mode: ZoomMode::Linear,
            zoom_to_cursor: false,
            orbit_sensitivity: 0.008,
            zoom_scroll_linear: 0.0025,
            zoom_scroll_exponential: 0.0035,
            dolly_sensitivity: 0.012,
            orbit_release_inertia_scale: 0.20,
            invert_orbit_x: false,
            invert_orbit_y: false,
        }
    }
}

impl OrbitSettings {
    // Tight per-frame zoom cap: egui smears one wheel notch over several frames,
    // so a loose cap compounds into a jarring jump.
    const ZOOM_CLAMP_MIN: f32 = 0.97;
    const ZOOM_CLAMP_MAX: f32 = 1.031;

    /// Multiplicative distance factor from one frame of vertical scroll.
    pub fn scroll_zoom_factor(&self, scroll_y: f32) -> f32 {
        if scroll_y.abs() < f32::EPSILON {
            return 1.0;
        }
        let factor = match self.zoom_mode {
            ZoomMode::Linear => 1.0 - scroll_y * self.zoom_scroll_linear,
            ZoomMode::Exponential => (-scroll_y * self.zoom_scroll_exponential).exp(),
        };
        factor.clamp(Self::ZOOM_CLAMP_MIN, Self::ZOOM_CLAMP_MAX)
    }

    /// Multiplicative distance factor from RMB vertical drag (pixels this frame).
    pub fn dolly_zoom_factor(&self, drag_dy_px: f32) -> f32 {
        if drag_dy_px.abs() < f32::EPSILON {
            return 1.0;
        }
        let factor = match self.zoom_mode {
            ZoomMode::Linear => 1.0 - drag_dy_px * self.dolly_sensitivity,
            ZoomMode::Exponential => (-drag_dy_px * self.dolly_sensitivity * 0.15).exp(),
        };
        factor.clamp(Self::ZOOM_CLAMP_MIN, Self::ZOOM_CLAMP_MAX)
    }
}

/// Mouse steering model for the fly backends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MouseSteer {
    /// Per-frame pointer motion turns the view 1:1 (classic FPS look).
    #[default]
    Relative,
    /// Cursor offset from the viewport centre → continuous yaw/pitch turn rate.
    Deflection,
}

/// Input-side tuning for fly mode (look sensitivity, axis inversion, steer mode).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FlySettings {
    /// Radians of yaw/pitch per pixel of mouse-look drag.
    pub look_sensitivity: f32,
    pub invert_look_x: bool,
    pub invert_look_y: bool,
    pub mouse_steer: MouseSteer,
    /// Dead-zone as a fraction (0..1) of the half-viewport in deflection mode.
    pub steer_deadzone: f32,
}

impl Default for FlySettings {
    fn default() -> Self {
        Self {
            look_sensitivity: 0.0035,
            invert_look_x: false,
            invert_look_y: false,
            mouse_steer: MouseSteer::Relative,
            steer_deadzone: 0.05,
        }
    }
}

// ─── orbit gather ────────────────────────────────────────────────────────────

/// Per-panel state for orbit/pan coast on pointer release.
#[derive(Debug, Clone, Copy, Default)]
pub struct OrbitDragState {
    pub last_orbit_yaw: f32,
    pub last_orbit_pitch: f32,
    smoothed_yaw: f32,
    smoothed_pitch: f32,
    pub last_pan_dx: f32,
    pub last_pan_dy: f32,
    smoothed_pan_dx: f32,
    smoothed_pan_dy: f32,
}

/// Camera intents gathered for one egui frame, plus a repaint hint.
#[derive(Debug, Clone, Default)]
pub struct CameraFrame {
    pub intents: Vec<CameraIntent>,
    pub request_repaint: bool,
}

/// Collect orbit / pan / dolly / zoom intents from a viewport `Response`.
///
/// `orbit_suppressed` lets the host reserve the LMB drag for something else
/// (e.g. node selection): when `true`, LMB-derived `Orbit`/`OrbitInertia` are
/// skipped, but MMB pan, RMB dolly, and wheel zoom still emit (unambiguously
/// camera). The smoothing/inertia logic mirrors a DCC viewport.
pub fn gather_orbit_intents(
    ui: &Ui,
    response: &Response,
    nav: &OrbitSettings,
    drag_state: &mut OrbitDragState,
    orbit_suppressed: bool,
) -> CameraFrame {
    let mut out = CameraFrame::default();

    let lmb = response.dragged_by(egui::PointerButton::Primary);
    let mmb = response.dragged_by(egui::PointerButton::Middle);
    let rmb = response.dragged_by(egui::PointerButton::Secondary);

    if lmb || mmb || rmb {
        out.request_repaint = true;
    }

    // `drag_stopped_by` is true when `dragged_by` is already false — keep outside the `lmb` branch.
    if response.drag_stopped_by(egui::PointerButton::Primary) && !orbit_suppressed {
        let yaw = if drag_state.smoothed_yaw.abs() > 1e-6 {
            drag_state.smoothed_yaw
        } else {
            drag_state.last_orbit_yaw
        };
        let pitch = if drag_state.smoothed_pitch.abs() > 1e-6 {
            drag_state.smoothed_pitch
        } else {
            drag_state.last_orbit_pitch
        };
        if yaw.abs() > 1e-5 || pitch.abs() > 1e-5 {
            let scale = nav.orbit_release_inertia_scale;
            out.intents.push(CameraIntent::OrbitInertia {
                yaw_delta: yaw * scale,
                pitch_delta: pitch * scale,
            });
            out.request_repaint = true;
        }
        drag_state.last_orbit_yaw = 0.0;
        drag_state.last_orbit_pitch = 0.0;
        drag_state.smoothed_yaw = 0.0;
        drag_state.smoothed_pitch = 0.0;
    }

    if response.drag_stopped_by(egui::PointerButton::Middle) {
        let dx = if drag_state.smoothed_pan_dx.abs() > 1e-4 {
            drag_state.smoothed_pan_dx
        } else {
            drag_state.last_pan_dx
        };
        let dy = if drag_state.smoothed_pan_dy.abs() > 1e-4 {
            drag_state.smoothed_pan_dy
        } else {
            drag_state.last_pan_dy
        };
        if dx.abs() > 1e-4 || dy.abs() > 1e-4 {
            out.intents.push(CameraIntent::PanInertia {
                dx_px: dx * nav.orbit_release_inertia_scale,
                dy_px: dy * nav.orbit_release_inertia_scale,
            });
            out.request_repaint = true;
        }
        drag_state.last_pan_dx = 0.0;
        drag_state.last_pan_dy = 0.0;
        drag_state.smoothed_pan_dx = 0.0;
        drag_state.smoothed_pan_dy = 0.0;
    }

    if mmb {
        let delta = pointer_drag_delta(ui, response);
        const SMOOTH: f32 = 0.4;
        drag_state.last_pan_dx = delta.x;
        drag_state.last_pan_dy = delta.y;
        drag_state.smoothed_pan_dx = drag_state.smoothed_pan_dx * (1.0 - SMOOTH) + delta.x * SMOOTH;
        drag_state.smoothed_pan_dy = drag_state.smoothed_pan_dy * (1.0 - SMOOTH) + delta.y * SMOOTH;
        out.intents.push(CameraIntent::Pan {
            dx_px: delta.x,
            dy_px: delta.y,
        });
    } else if rmb {
        let factor = nav.dolly_zoom_factor(response.drag_delta().y);
        out.intents.push(CameraIntent::Zoom { factor });
    } else if lmb && !orbit_suppressed {
        let delta = pointer_drag_delta(ui, response);
        let sx = if nav.invert_orbit_x { -1.0 } else { 1.0 };
        let sy = if nav.invert_orbit_y { -1.0 } else { 1.0 };
        let yaw = -delta.x * nav.orbit_sensitivity * sx;
        let pitch = delta.y * nav.orbit_sensitivity * sy;
        const SMOOTH: f32 = 0.4;
        if yaw != 0.0 || pitch != 0.0 {
            drag_state.last_orbit_yaw = yaw;
            drag_state.last_orbit_pitch = pitch;
            drag_state.smoothed_yaw = drag_state.smoothed_yaw * (1.0 - SMOOTH) + yaw * SMOOTH;
            drag_state.smoothed_pitch = drag_state.smoothed_pitch * (1.0 - SMOOTH) + pitch * SMOOTH;
        }
        out.intents.push(CameraIntent::Orbit {
            yaw_delta: yaw,
            pitch_delta: pitch,
        });
    }

    if response.hovered() {
        out.request_repaint = true;
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            ui.input_mut(|i| i.smooth_scroll_delta = egui::Vec2::ZERO);
            let factor = nav.scroll_zoom_factor(scroll);
            let anchored = if nav.zoom_to_cursor {
                pointer_in_viewport(response)
            } else {
                None
            };
            if let Some(point) = anchored {
                out.intents.push(CameraIntent::ZoomAtCursor {
                    cursor_x: point.x,
                    cursor_y: point.y,
                    factor,
                });
            } else {
                out.intents.push(CameraIntent::Zoom { factor });
            }
        }
    }

    out
}

// ─── fly gather ──────────────────────────────────────────────────────────────

/// Per-frame fly-camera intents while a fly backend is engaged.
///
/// - mouse → steering: `MouseSteer::Relative` → per-frame [`CameraIntent::Look`];
///   `MouseSteer::Deflection` → held [`CameraIntent::LookRate`] from the cursor's
///   offset to `steer_center` over the half-`viewport_rect`.
/// - W/S forward, A/D strafe, R/F up/down → one [`CameraIntent::Thrust`].
/// - Shift edge → [`CameraIntent::Boost`] (only on change; `boost_held` is the
///   caller-owned latch).
/// - Q/E → [`CameraIntent::Roll`] only when `allow_roll`.
///
/// Thrust/roll/look-rate are emitted EVERY frame (even zero) so the host's held
/// state refreshes and a release stops the motion.
pub fn gather_fly_intents(
    ui: &Ui,
    fly: &FlySettings,
    boost_held: &mut bool,
    allow_roll: bool,
    viewport_rect: egui::Rect,
    steer_center: Pos2,
) -> Vec<CameraIntent> {
    let mut out = Vec::new();

    let sx = if fly.invert_look_x { -1.0 } else { 1.0 };
    let sy = if fly.invert_look_y { -1.0 } else { 1.0 };
    if allow_roll && fly.mouse_steer == MouseSteer::Deflection {
        let (mut yaw, mut pitch) = (0.0f32, 0.0f32);
        if let Some(pos) = ui.input(|i| i.pointer.latest_pos()) {
            let c = steer_center;
            let half = (viewport_rect.size() * 0.5).max(egui::Vec2::splat(1.0));
            let dz = fly.steer_deadzone.clamp(0.0, 0.95);
            let shape = |raw: f32| -> f32 {
                let v = raw.clamp(-1.0, 1.0);
                let a = v.abs();
                if a <= dz {
                    0.0
                } else {
                    v.signum() * ((a - dz) / (1.0 - dz))
                }
            };
            yaw = shape((pos.x - c.x) / half.x) * sx;
            pitch = -shape((pos.y - c.y) / half.y) * sy;
        }
        out.push(CameraIntent::LookRate { yaw, pitch });
    } else {
        let delta = ui.input(|i| i.pointer.delta());
        if delta.x != 0.0 || delta.y != 0.0 {
            let s = fly.look_sensitivity;
            out.push(CameraIntent::Look {
                dyaw: delta.x * s * sx,
                dpitch: -delta.y * s * sy,
            });
        }
        // Keep the deflection axis zeroed when not steering, so toggling modes
        // mid-flight never leaves a stuck turn.
        if allow_roll {
            out.push(CameraIntent::LookRate {
                yaw: 0.0,
                pitch: 0.0,
            });
        }
    }

    let (forward, right, up, roll) = ui.input(|i| {
        let k = |key| i.key_down(key) as i32 as f32;
        let forward = k(egui::Key::W) - k(egui::Key::S);
        let right = k(egui::Key::D) - k(egui::Key::A);
        let up = k(egui::Key::R) - k(egui::Key::F);
        let roll = k(egui::Key::E) - k(egui::Key::Q);
        (forward, right, up, roll)
    });
    out.push(CameraIntent::Thrust { forward, right, up });

    let shift_now = ui.input(|i| i.modifiers.shift);
    if shift_now != *boost_held {
        *boost_held = shift_now;
        out.push(CameraIntent::Boost(shift_now));
    }

    if allow_roll {
        out.push(CameraIntent::Roll { d: roll });
    }

    out
}

// ─── local pointer helpers (replaces render_present) ─────────────────────────

/// Pointer position in viewport-local pixels (origin at the widget's top-left),
/// preferring the active interaction position, else hover. `None` off-widget.
fn pointer_in_viewport(response: &Response) -> Option<Pos2> {
    let p = response
        .interact_pointer_pos()
        .or_else(|| response.hover_pos())?;
    let r = response.rect;
    Some(Pos2::new(p.x - r.min.x, p.y - r.min.y))
}

/// Prefer the widget drag delta; fall back to raw pointer delta inside the rect.
fn pointer_drag_delta(ui: &Ui, response: &Response) -> egui::Vec2 {
    let delta = response.drag_delta();
    if delta.length_sq() > 0.0 {
        return delta;
    }
    let rect = response.rect;
    ui.input(|i| {
        if let Some(pos) = i.pointer.interact_pos()
            && rect.contains(pos)
        {
            return i.pointer.delta();
        }
        egui::Vec2::ZERO
    })
}
