//! GradingTone operation — faithful port of OCIO's GradingTone CPU op.
//!
//! This module mirrors, line-for-line where practical, the reference:
//! - `ops/gradingtone/GradingTone.cpp`        (precompute of curve knots/slopes)
//! - `ops/gradingtone/GradingToneOpCPU.cpp`   (forward + inverse faux-cubic per zone)
//!
//! The op composes six tonal adjustments. The forward apply order is
//! mids → highlightShadow(false) → whiteBlack(false) → highlightShadow(true) →
//! whiteBlack(true) → scontrast; the inverse runs the conjugate algorithm in the
//! exactly reversed order (GradingToneOpCPU.cpp `apply`).
//!
//! Each zone is a "faux cubic": a quadratic-Bezier-like segment pair joined at a
//! knot. The forward uses `FauxCubicFwd`-style evaluation; the inverse solves the
//! same quadratics for `x` given `y` (`FauxCubicRev`). Master (`M`) channel applies
//! the same curve to all three RGB components; R/G/B apply to a single component.

use crate::transform::{GradingRgbmsw, GradingToneStyle};

/// RGBM channel enum for grading operations.
/// OCIO ref: GradingTone.h `enum RGBMChannel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RgbmChannel {
    /// Red.
    R = 0,
    /// Green.
    G = 1,
    /// Blue.
    B = 2,
    /// Master (applies to all RGB).
    M = 3,
}

/// Extension trait for GradingRgbmsw to add channel access + identity test.
pub trait GradingRgbmswExt {
    /// Gets channel value (R/G/B/M).
    fn channel(&self, ch: RgbmChannel) -> f64;
    /// Checks if all RGBM values are identity (== 1.0). OCIO uses exact `== 1.`.
    fn is_identity(&self) -> bool;
}

impl GradingRgbmswExt for GradingRgbmsw {
    #[inline]
    fn channel(&self, ch: RgbmChannel) -> f64 {
        match ch {
            RgbmChannel::R => self.red,
            RgbmChannel::G => self.green,
            RgbmChannel::B => self.blue,
            RgbmChannel::M => self.master,
        }
    }

    #[inline]
    fn is_identity(&self) -> bool {
        // OCIO GradingTone.cpp IsIdentity uses exact equality to 1.
        self.red == 1.0 && self.green == 1.0 && self.blue == 1.0 && self.master == 1.0
    }
}

/// GradingTone parameters (OCIO `GradingTone`).
///
/// Field naming follows OCIO: for `midtones` the `start` field is the curve
/// CENTER and `width` is the curve WIDTH; for `shadows`/`highlights` the `start`
/// field is the zone START and `width` is the PIVOT; for `blacks`/`whites` the
/// `start` is the zone START and `width` is the zone WIDTH.
#[derive(Debug, Clone, PartialEq)]
pub struct GradingTone {
    /// Blacks adjustment.
    pub blacks: GradingRgbmsw,
    /// Shadows adjustment.
    pub shadows: GradingRgbmsw,
    /// Midtones adjustment.
    pub midtones: GradingRgbmsw,
    /// Highlights adjustment.
    pub highlights: GradingRgbmsw,
    /// Whites adjustment.
    pub whites: GradingRgbmsw,
    /// Saturation contrast (1.0 == identity).
    pub s_contrast: f64,
}

impl GradingTone {
    /// Reject parameters outside the bounds the reference enforces.
    ///
    /// Reference: `GradingTone::validate` (`ops/gradingtone/GradingTone.cpp:42-234`).
    /// Bounds: blacks/mids/whites `[0.1, 1.9]`, shadows/highlights `[0.2, 1.8]`,
    /// s-contrast `[0.01, 1.99]`, each widened by `1e-6` so float round-trips do not trip them.
    ///
    /// `width` means two different things: for blacks/midtones/whites it is a width with a
    /// `0.01` floor; for shadows/highlights it is a **pivot**, checked only against overlapping
    /// `start`. A uniform width floor rejects the LOG defaults (`shadows.width = 0.0`).
    pub fn validate(&self) -> crate::OcioResult<()> {
        use crate::OcioError;

        const ERR: f64 = 1e-6;
        const MIN_BMW: f64 = 0.1;
        const MAX_BMW: f64 = 1.9;
        const MIN_SH: f64 = 0.2;
        const MAX_SH: f64 = 1.8;
        const MIN_WIDTH: f64 = 0.01;
        const MIN_SC: f64 = 0.01;
        const MAX_SC: f64 = 1.99;

        let levels =
            |zone: &GradingRgbmsw, name: &str, lo: f64, hi: f64| -> crate::OcioResult<()> {
                for (ch, v) in [
                    ("red", zone.red),
                    ("green", zone.green),
                    ("blue", zone.blue),
                    ("master", zone.master),
                ] {
                    if v < lo - ERR || v > hi + ERR {
                        return Err(OcioError::InvalidTransform {
                            reason: format!(
                                "GradingTone {name}.{ch} = {v} is outside [{lo}, {hi}]"
                            ),
                        });
                    }
                }
                Ok(())
            };

        for (zone, name) in [
            (&self.blacks, "blacks"),
            (&self.midtones, "midtones"),
            (&self.whites, "whites"),
        ] {
            levels(zone, name, MIN_BMW, MAX_BMW)?;
            if zone.width < MIN_WIDTH - ERR {
                return Err(OcioError::InvalidTransform {
                    reason: format!(
                        "GradingTone {name}.width = {} is below {MIN_WIDTH}",
                        zone.width
                    ),
                });
            }
        }

        levels(&self.shadows, "shadows", MIN_SH, MAX_SH)?;
        if self.shadows.start < self.shadows.width + MIN_WIDTH - ERR {
            return Err(OcioError::InvalidTransform {
                reason: format!(
                    "GradingTone shadows.start = {} overlaps pivot {}",
                    self.shadows.start, self.shadows.width
                ),
            });
        }

        levels(&self.highlights, "highlights", MIN_SH, MAX_SH)?;
        if self.highlights.start > self.highlights.width - MIN_WIDTH + ERR {
            return Err(OcioError::InvalidTransform {
                reason: format!(
                    "GradingTone highlights.start = {} overlaps pivot {}",
                    self.highlights.start, self.highlights.width
                ),
            });
        }

        if self.s_contrast < MIN_SC - ERR || self.s_contrast > MAX_SC + ERR {
            return Err(OcioError::InvalidTransform {
                reason: format!(
                    "GradingTone s_contrast = {} is outside [{MIN_SC}, {MAX_SC}]",
                    self.s_contrast
                ),
            });
        }
        Ok(())
    }

    /// Builds the per-style default GradingTone (OCIO `GradingTone(GradingStyle)`).
    /// OCIO ref: OpenColorTransforms.h `explicit GradingTone(GradingStyle style)`.
    /// The `(a, b)` pairs below are the `GradingRGBMSW(start, width)` 2-arg ctor:
    /// RGBM stay 1.0 and only start/width are set.
    pub fn from_style(style: GradingToneStyle) -> Self {
        let rgbmsw = |start: f64, width: f64| GradingRgbmsw {
            red: 1.0,
            green: 1.0,
            blue: 1.0,
            master: 1.0,
            start,
            width,
        };
        match style {
            GradingToneStyle::Linear => Self {
                blacks: rgbmsw(0.0, 4.0),
                shadows: rgbmsw(2.0, -7.0),
                midtones: rgbmsw(0.0, 8.0),
                highlights: rgbmsw(-2.0, 9.0),
                whites: rgbmsw(0.0, 8.0),
                s_contrast: 1.0,
            },
            GradingToneStyle::Log => Self {
                blacks: rgbmsw(0.4, 0.4),
                shadows: rgbmsw(0.5, 0.0),
                midtones: rgbmsw(0.4, 0.6),
                highlights: rgbmsw(0.3, 1.0),
                whites: rgbmsw(0.4, 0.5),
                s_contrast: 1.0,
            },
            GradingToneStyle::Video => Self {
                blacks: rgbmsw(0.4, 0.4),
                shadows: rgbmsw(0.6, 0.0),
                midtones: rgbmsw(0.4, 0.7),
                highlights: rgbmsw(0.2, 1.0),
                whites: rgbmsw(0.5, 0.5),
                s_contrast: 1.0,
            },
        }
    }

    /// True iff every zone is identity and s_contrast == 1 (OCIO `IsIdentity`).
    pub fn is_identity(&self) -> bool {
        self.blacks.is_identity()
            && self.shadows.is_identity()
            && self.midtones.is_identity()
            && self.highlights.is_identity()
            && self.whites.is_identity()
            && self.s_contrast == 1.0
    }
}

impl Default for GradingTone {
    /// Default is the LOG-style GradingTone (OCIO default style is GRADING_LOG).
    fn default() -> Self {
        Self::from_style(GradingToneStyle::Log)
    }
}

/// Pre-computed constants for GradingTone CPU evaluation.
/// OCIO ref: GradingTone.h `struct GradingTonePreRender`.
#[derive(Debug, Clone)]
pub struct GradingTonePreRender {
    // Style-dependent values (GradingTonePreRender::FromStyle).
    /// Upper bound of the working range.
    pub top: f32,
    /// Upper bound used by s-contrast.
    pub top_sc: f32,
    /// Lower bound of the working range.
    pub bottom: f32,
    /// S-contrast pivot.
    pub pivot: f32,

    // Pre-computed shadow/highlight/black/white ranges (doubles in OCIO).
    /// Shadow start (resolved).
    pub shadows_start: f64,
    /// Shadow pivot.
    pub shadows_width: f64,
    /// Highlight start (resolved).
    pub highlights_start: f64,
    /// Highlight pivot.
    pub highlights_width: f64,
    /// Blacks start (resolved through the shadow curve).
    pub blacks_start: f64,
    /// Blacks width (resolved through the shadow curve).
    pub blacks_width: f64,
    /// Whites start (resolved through the highlight curve).
    pub whites_start: f64,
    /// Whites width (resolved through the highlight curve).
    pub whites_width: f64,

    // Midtones curve data [channel][point] — 6 control points per channel.
    /// Midtones x knots.
    pub mid_x: [[f32; 6]; 4],
    /// Midtones y knots.
    pub mid_y: [[f32; 6]; 4],
    /// Midtones slopes.
    pub mid_m: [[f32; 6]; 4],

    // Highlights/Shadows curve data [hs_type][channel][point]; hs_type: 0 = highlights, 1 = shadows.
    /// HS x knots (3 per curve).
    pub hs_x: [[[f32; 3]; 4]; 2],
    /// HS y knots (3 per curve).
    pub hs_y: [[[f32; 3]; 4]; 2],
    /// HS slopes — index [0] is m0, index [1] is m2 (m1 unused; OCIO comment).
    pub hs_m: [[[f32; 2]; 4]; 2],

    // Whites/Blacks curve data [wb_type][channel][point]; wb_type: 0 = whites, 1 = blacks.
    /// WB x knots (2 per curve).
    pub wb_x: [[[f32; 2]; 4]; 2],
    /// WB y knots (2 per curve).
    pub wb_y: [[[f32; 2]; 4]; 2],
    /// WB slopes (m0, m1).
    pub wb_m: [[[f32; 2]; 4]; 2],
    /// WB gain (used only in the slope-increasing case).
    pub wb_gain: [[f32; 4]; 2],

    // Saturation-contrast data [top/bottom]; 4 x/y values, slopes m0 & m3.
    /// SC x knots.
    pub sc_x: [[f32; 4]; 2],
    /// SC y knots.
    pub sc_y: [[f32; 4]; 2],
    /// SC slopes (m0, m3).
    pub sc_m: [[f32; 2]; 2],

    /// True iff the whole op is identity (skip apply).
    pub local_bypass: bool,

    style: GradingToneStyle,
}

impl GradingTonePreRender {
    /// Creates new pre-render data for the given style (zeroed curves).
    /// OCIO ref: `GradingTonePreRender(GradingStyle)` → `setStyle`.
    pub fn new(style: GradingToneStyle) -> Self {
        let (top, top_sc, bottom, pivot) = Self::from_style(style);
        Self {
            top,
            top_sc,
            bottom,
            pivot,
            shadows_start: 0.0,
            shadows_width: 0.0,
            highlights_start: 0.0,
            highlights_width: 0.0,
            blacks_start: 0.0,
            blacks_width: 0.0,
            whites_start: 0.0,
            whites_width: 0.0,
            mid_x: [[0.0; 6]; 4],
            mid_y: [[0.0; 6]; 4],
            mid_m: [[0.0; 6]; 4],
            hs_x: [[[0.0; 3]; 4]; 2],
            hs_y: [[[0.0; 3]; 4]; 2],
            hs_m: [[[0.0; 2]; 4]; 2],
            wb_x: [[[0.0; 2]; 4]; 2],
            wb_y: [[[0.0; 2]; 4]; 2],
            wb_m: [[[0.0; 2]; 4]; 2],
            wb_gain: [[0.0; 4]; 2],
            sc_x: [[0.0; 4]; 2],
            sc_y: [[0.0; 4]; 2],
            sc_m: [[0.0; 2]; 2],
            local_bypass: false,
            style,
        }
    }

    /// Returns (top, top_sc, bottom, pivot) for a style.
    /// OCIO ref: GradingTone.cpp `GradingTonePreRender::FromStyle`.
    pub fn from_style(style: GradingToneStyle) -> (f32, f32, f32, f32) {
        match style {
            GradingToneStyle::Log => (1.0, 1.0, 0.0, 0.4),
            GradingToneStyle::Linear => (7.5, 6.5, -5.5, 0.0),
            GradingToneStyle::Video => (1.0, 1.0, 0.0, 0.4),
        }
    }

    /// Recomputes all curve tables from a GradingTone value.
    /// OCIO ref: GradingTone.cpp `GradingTonePreRender::update`.
    pub fn update(&mut self, v: &GradingTone) {
        self.local_bypass = v.is_identity();
        if self.local_bypass {
            return;
        }

        // Highlights/whites range resolution (GradingTone.cpp:409-425).
        {
            let master = v.highlights.master;
            let start = v.highlights.start;
            let pivot = v.highlights.width; // "pivot" for highlights.
            let startw = v.whites.start;
            let widthw = v.whites.width;

            self.highlights_start = if start > pivot - 0.01 {
                pivot - 0.01
            } else {
                start
            };
            self.highlights_width = pivot;

            let new_start =
                highlight_fwd_eval(startw, self.highlights_start, self.highlights_width, master);
            let new_end = highlight_fwd_eval(
                startw + widthw,
                self.highlights_start,
                self.highlights_width,
                master,
            );
            self.whites_start = new_start;
            self.whites_width = new_end - new_start;
        }

        // Shadows/blacks range resolution (GradingTone.cpp:426-441).
        {
            let master = v.shadows.master;
            let start = v.shadows.start;
            let pivot = v.shadows.width; // "pivot" for shadows.
            let startb = v.blacks.start;
            let widthb = v.blacks.width;

            self.shadows_start = if start < pivot + 0.01 {
                pivot + 0.01
            } else {
                start
            };
            self.shadows_width = pivot;

            let new_start = shadow_fwd_eval(startb, self.shadows_width, self.shadows_start, master);
            let new_end = shadow_fwd_eval(
                startb - widthb,
                self.shadows_width,
                self.shadows_start,
                master,
            );
            self.blacks_start = new_start;
            self.blacks_width = new_start - new_end;
        }

        self.mids_precompute(v, self.top, self.bottom);
        self.highlight_shadow_precompute(v);
        self.white_black_precompute(v);
        self.scontrast_precompute(v, self.top_sc, self.bottom, self.pivot);
    }

    /// Midtones knot/slope precompute.
    /// OCIO ref: GradingTone.cpp `mids_precompute`.
    fn mids_precompute(&mut self, v: &GradingTone, top: f32, bottom: f32) {
        const HALO: f32 = 0.4;

        for ch in 0..4 {
            let channel = idx_to_channel(ch);
            let mut mid_adj = clamp_f32(v.midtones.channel(channel) as f32, 0.01, 1.99);
            if mid_adj == 1.0 {
                continue;
            }

            let x0 = bottom;
            let x5 = top;

            let max_width = (x5 - x0) * 0.95;
            let width = clamp_f32(v.midtones.width as f32, 0.01, max_width);
            let min_cent = x0 + width * 0.51;
            let max_cent = x5 - width * 0.51;
            let center = clamp_f32(v.midtones.start as f32, min_cent, max_cent);

            let x1 = center - width * 0.5;
            let x4 = x1 + width;
            let x2 = x1 + (x4 - x1) * 0.25;
            let x3 = x1 + (x4 - x1) * 0.75;
            let y0 = x0;
            let m0 = 1.0;
            let m5 = 1.0;

            const MIN_SLOPE: f32 = 0.1;
            mid_adj -= 1.0;
            mid_adj *= 1.0 - MIN_SLOPE;

            let m2 = 1.0 + mid_adj;
            let m3 = 1.0 - mid_adj;
            let mut m1 = 1.0 + mid_adj * HALO;
            let mut m4 = 1.0 - mid_adj * HALO;

            if center <= (x5 + x0) * 0.5 {
                let area = (x1 - x0) * (m1 - m0) * 0.5
                    + (x2 - x1) * ((m1 - m0) + (m2 - m1) * 0.5)
                    + (center - x2) * (m2 - m0) * 0.5;
                m4 = (-0.5 * (x5 - x4) * m5
                    + (x4 - x3) * (0.5 * m3 - m5)
                    + (x3 - center) * (m3 - m5) * 0.5
                    + area)
                    / (-0.5 * (x5 - x3));
            } else {
                let area = (x5 - x4) * (m4 - m5) * 0.5
                    + (x4 - x3) * ((m4 - m5) + (m3 - m4) * 0.5)
                    + (x3 - center) * (m3 - m5) * 0.5;
                m1 = (-0.5 * (x1 - x0) * m0
                    + (x2 - x1) * (0.5 * m2 - m0)
                    + (center - x2) * (m2 - m0) * 0.5
                    + area)
                    / (-0.5 * (x2 - x0));
            }

            let y1 = y0 + (m0 + m1) * (x1 - x0) * 0.5;
            let y2 = y1 + (m1 + m2) * (x2 - x1) * 0.5;
            let y3 = y2 + (m2 + m3) * (x3 - x2) * 0.5;
            let y4 = y3 + (m3 + m4) * (x4 - x3) * 0.5;
            let y5 = y4 + (m4 + m5) * (x5 - x4) * 0.5;

            self.mid_x[ch] = [x0, x1, x2, x3, x4, x5];
            self.mid_y[ch] = [y0, y1, y2, y3, y4, y5];
            self.mid_m[ch] = [m0, m1, m2, m3, m4, m5];
        }
    }

    /// Highlights/shadows knot/slope precompute.
    /// OCIO ref: GradingTone.cpp `highlightShadow_precompute`.
    fn highlight_shadow_precompute(&mut self, v: &GradingTone) {
        // OCIO iterates { false, true } (false = highlights -> index 0).
        for &is_shadow in &[false, true] {
            let idx = if is_shadow { 1 } else { 0 };
            for ch in 0..4 {
                let channel = idx_to_channel(ch);
                let mut val = if is_shadow {
                    v.shadows.channel(channel) as f32
                } else {
                    v.highlights.channel(channel) as f32
                };
                if !is_shadow {
                    val = 2.0 - val;
                }
                if val == 1.0 {
                    continue;
                }

                let start = if is_shadow {
                    self.shadows_start as f32
                } else {
                    self.highlights_start as f32
                };
                let pivot = if is_shadow {
                    self.shadows_width as f32
                } else {
                    self.highlights_width as f32
                };

                let x0 = if is_shadow { pivot } else { start };
                let x2 = if is_shadow { start } else { pivot };
                let y0 = x0;
                let y2 = x2;
                let x1 = x0 + (x2 - x0) * 0.5;

                let (m0, m2, y1);
                if val < 1.0 {
                    m0 = if is_shadow { 0.01_f32.max(val) } else { 1.0 };
                    m2 = if is_shadow { 1.0 } else { 0.01_f32.max(val) };
                    y1 = (0.5 / (x2 - x0))
                        * ((2.0 * y0 + m0 * (x1 - x0)) * (x2 - x1)
                            + (2.0 * y2 - m2 * (x2 - x1)) * (x1 - x0));
                } else {
                    // val > 1.0
                    m0 = if is_shadow {
                        0.01_f32.max(2.0 - val)
                    } else {
                        1.0
                    };
                    m2 = if is_shadow {
                        1.0
                    } else {
                        0.01_f32.max(2.0 - val)
                    };
                    y1 = (0.5 / ((x2 - x1) + (x1 - x0)))
                        * ((2.0 * y0 + m0 * (x1 - x0)) * (x2 - x1)
                            + (2.0 * y2 - m2 * (x2 - x1)) * (x1 - x0));
                }

                self.hs_x[idx][ch] = [x0, x1, x2];
                self.hs_y[idx][ch] = [y0, y1, y2];
                self.hs_m[idx][ch] = [m0, m2];
            }
        }
    }

    /// Whites/blacks knot/slope precompute.
    /// OCIO ref: GradingTone.cpp `whiteBlack_precompute`.
    fn white_black_precompute(&mut self, v: &GradingTone) {
        // OCIO iterates { false, true } (false = whites -> index 0).
        for &is_black in &[false, true] {
            let idx = if is_black { 1 } else { 0 };
            for ch in 0..4 {
                let channel = idx_to_channel(ch);

                let start = if is_black {
                    self.blacks_start as f32
                } else {
                    self.whites_start as f32
                };
                let width = if is_black {
                    self.blacks_width as f32
                } else {
                    self.whites_width as f32
                };

                let val = if is_black {
                    v.blacks.channel(channel) as f32
                } else {
                    v.whites.channel(channel) as f32
                };

                let x0 = if !is_black { start } else { start - width };
                let x1 = if !is_black { x0 + width } else { start };

                let mtest = if !is_black { val } else { 2.0 - val };

                let (mut m0, mut m1, mut y0, mut y1, mut gain) =
                    (0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32);

                if mtest < 1.0 {
                    // Slope is decreasing case.
                    if !is_black {
                        m0 = 1.0;
                        m1 = 0.01_f32.max(val);
                        y0 = x0;
                        y1 = y0 + (m0 + m1) * (x1 - x0) * 0.5;
                    } else {
                        m0 = 0.01_f32.max(2.0 - val);
                        m1 = 1.0;
                        y1 = x1;
                        y0 = y1 - (m0 + m1) * (x1 - x0) * 0.5;
                    }
                } else if mtest > 1.0 {
                    // Slope is increasing case.
                    if !is_black {
                        m0 = 1.0;
                        m1 = 0.01_f32.max(2.0 - val);
                        y0 = x0;
                        // y1 won't be used.
                    } else {
                        m0 = 0.01_f32.max(val);
                        m1 = 1.0;
                        y1 = x1;
                        y0 = y1 - (m0 + m1) * (x1 - x0) * 0.5;
                    }
                    gain = (m0 + m1) * 0.5;
                }

                self.wb_x[idx][ch] = [x0, x1];
                self.wb_y[idx][ch] = [y0, y1];
                self.wb_m[idx][ch] = [m0, m1];
                self.wb_gain[idx][ch] = gain;
            }
        }
    }

    /// Saturation-contrast knot/slope precompute.
    /// OCIO ref: GradingTone.cpp `scontrast_precompute`.
    fn scontrast_precompute(&mut self, v: &GradingTone, top_sc: f32, bottom: f32, pivot: f32) {
        let mut contrast = v.s_contrast as f32;
        if contrast == 1.0 {
            return;
        }
        // Limit the range of values to prevent reversals.
        contrast = if contrast > 1.0 {
            1.0 / (1.8125 - 0.8125 * contrast.min(1.99))
        } else {
            0.28125 + 0.71875 * contrast.max(0.01)
        };

        // Top end (index 0).
        {
            let x3 = top_sc;
            let y3 = top_sc;
            let y0 = pivot + (y3 - pivot) * 0.25;
            let m0 = contrast;
            let x0 = pivot + (y0 - pivot) / m0;
            let min_width = (x3 - x0) * 0.3;
            let mut m3 = 1.0 / m0;
            // NB: m0 != m3 (contrast != 1).
            let center = (y3 - y0 - m3 * x3 + m0 * x0) / (m0 - m3);
            let mut x1 = x0;
            let mut x2 = 2.0 * center - x1;
            if x2 > x3 {
                x2 = x3;
                x1 = 2.0 * center - x2;
            } else if (x2 - x1) < min_width {
                x2 = x1 + min_width;
                let new_center = (x2 + x1) * 0.5;
                m3 = (y3 - y0 + m0 * x0 - new_center * m0) / (x3 - new_center);
            }
            let y1 = y0;
            let y2 = y1 + (m0 + m3) * (x2 - x1) * 0.5;

            self.sc_x[0] = [x0, x1, x2, x3];
            self.sc_y[0] = [y0, y1, y2, y3];
            self.sc_m[0] = [m0, m3];
        }

        // Bottom end (index 1).
        {
            let x0 = bottom;
            let y0 = bottom;
            let y3 = pivot - (pivot - y0) * 0.25;
            let m3 = contrast;
            let x3 = pivot - (pivot - y3) / m3;
            let min_width = (x3 - x0) * 0.3;
            let mut m0 = 1.0 / m3;
            let center = (y3 - y0 - m3 * x3 + m0 * x0) / (m0 - m3);
            let mut x2 = x3;
            let mut x1 = 2.0 * center - x2;
            if x1 < x0 {
                x1 = x0;
                x2 = 2.0 * center - x1;
            } else if (x2 - x1) < min_width {
                x1 = x2 - min_width;
                let new_center = (x2 + x1) * 0.5;
                m0 = (y3 - y0 - m3 * x3 + new_center * m3) / (new_center - x0);
            }
            let y2 = y3;
            let y1 = y2 - (m0 + m3) * (x2 - x1) * 0.5;

            self.sc_x[1] = [x0, x1, x2, x3];
            self.sc_y[1] = [y0, y1, y2, y3];
            self.sc_m[1] = [m0, m3];
        }
    }

    // ---- Per-pixel forward zone evaluators (GradingToneFwdOpCPU) ----

    /// Forward midtones. OCIO ref: `GradingToneFwdOpCPU::mids`.
    fn mids_fwd(&self, v: &GradingTone, channel: RgbmChannel, out: &mut [f32; 3]) {
        let mid_adj = clamp_f32(v.midtones.channel(channel) as f32, 0.01, 1.99);
        if mid_adj == 1.0 {
            return;
        }
        let ch = channel as usize;
        let (x, y, m) = (&self.mid_x[ch], &self.mid_y[ch], &self.mid_m[ch]);
        apply_master_or_single(channel, out, |t| mids_fwd_eval(t, x, y, m));
    }

    /// Inverse midtones. OCIO ref: `GradingToneRevOpCPU::mids`.
    fn mids_rev(&self, v: &GradingTone, channel: RgbmChannel, out: &mut [f32; 3]) {
        let mid_adj = clamp_f32(v.midtones.channel(channel) as f32, 0.01, 1.99);
        if mid_adj == 1.0 {
            return;
        }
        let ch = channel as usize;
        let (x, y, m) = (&self.mid_x[ch], &self.mid_y[ch], &self.mid_m[ch]);
        apply_master_or_single(channel, out, |t| mids_rev_eval(t, x, y, m));
    }

    /// Forward highlight/shadow. OCIO ref: `GradingToneFwdOpCPU::highlightShadow`.
    fn highlight_shadow_fwd(
        &self,
        v: &GradingTone,
        channel: RgbmChannel,
        is_shadow: bool,
        out: &mut [f32; 3],
    ) {
        let mut val = if is_shadow {
            v.shadows.channel(channel) as f32
        } else {
            v.highlights.channel(channel) as f32
        };
        if !is_shadow {
            val = 2.0 - val;
        }
        if val == 1.0 {
            return;
        }
        let idx = if is_shadow { 1 } else { 0 };
        let ch = channel as usize;
        let x = &self.hs_x[idx][ch];
        let y = &self.hs_y[idx][ch];
        let (m0, m2) = (self.hs_m[idx][ch][0], self.hs_m[idx][ch][1]);

        if val < 1.0 {
            apply_master_or_single(channel, out, |t| compute_hs_fwd(t, x, y, m0, m2));
        } else {
            apply_master_or_single(channel, out, |t| compute_hs_rev(t, x, y, m0, m2));
        }
    }

    /// Inverse highlight/shadow. OCIO ref: `GradingToneRevOpCPU::highlightShadow`.
    fn highlight_shadow_rev(
        &self,
        v: &GradingTone,
        channel: RgbmChannel,
        is_shadow: bool,
        out: &mut [f32; 3],
    ) {
        let mut val = if is_shadow {
            v.shadows.channel(channel) as f32
        } else {
            v.highlights.channel(channel) as f32
        };
        if !is_shadow {
            val = 2.0 - val;
        }
        if val == 1.0 {
            return;
        }
        let idx = if is_shadow { 1 } else { 0 };
        let ch = channel as usize;
        let x = &self.hs_x[idx][ch];
        let y = &self.hs_y[idx][ch];
        let (m0, m2) = (self.hs_m[idx][ch][0], self.hs_m[idx][ch][1]);

        // Conjugate of fwd: val<1 uses Rev algorithm, val>1 uses Fwd.
        if val < 1.0 {
            apply_master_or_single(channel, out, |t| compute_hs_rev(t, x, y, m0, m2));
        } else {
            apply_master_or_single(channel, out, |t| compute_hs_fwd(t, x, y, m0, m2));
        }
    }

    /// Forward white/black. OCIO ref: `GradingToneFwdOpCPU::whiteBlack`.
    fn white_black_fwd(
        &self,
        v: &GradingTone,
        channel: RgbmChannel,
        is_black: bool,
        out: &mut [f32; 3],
    ) {
        let val = if is_black {
            v.blacks.channel(channel) as f32
        } else {
            v.whites.channel(channel) as f32
        };
        let idx = if is_black { 1 } else { 0 };
        let ch = channel as usize;
        let x0 = self.wb_x[idx][ch][0];
        let x1 = self.wb_x[idx][ch][1];
        let y0 = self.wb_y[idx][ch][0];
        let y1 = self.wb_y[idx][ch][1];
        let m0 = self.wb_m[idx][ch][0];
        let m1 = self.wb_m[idx][ch][1];
        let gain = self.wb_gain[idx][ch];

        apply_master_or_single(channel, out, |t| {
            compute_wb_fwd(is_black, val, x0, x1, y0, y1, m0, m1, gain, t)
        });
    }

    /// Inverse white/black. OCIO ref: `GradingToneRevOpCPU::whiteBlack`.
    fn white_black_rev(
        &self,
        v: &GradingTone,
        channel: RgbmChannel,
        is_black: bool,
        out: &mut [f32; 3],
    ) {
        let val = if is_black {
            v.blacks.channel(channel) as f32
        } else {
            v.whites.channel(channel) as f32
        };
        let idx = if is_black { 1 } else { 0 };
        let ch = channel as usize;
        let x0 = self.wb_x[idx][ch][0];
        let x1 = self.wb_x[idx][ch][1];
        let y0 = self.wb_y[idx][ch][0];
        let y1 = self.wb_y[idx][ch][1];
        let m0 = self.wb_m[idx][ch][0];
        let m1 = self.wb_m[idx][ch][1];
        let gain = self.wb_gain[idx][ch];

        apply_master_or_single(channel, out, |t| {
            compute_wb_rev(is_black, val, x0, x1, y0, y1, m0, m1, gain, t)
        });
    }

    /// Forward saturation contrast. OCIO ref: `GradingToneFwdOpCPU::scontrast`.
    fn scontrast_fwd(&self, v: &GradingTone, out: &mut [f32; 3]) {
        let mut contrast = v.s_contrast as f32;
        if contrast == 1.0 {
            return;
        }
        contrast = if contrast > 1.0 {
            1.0 / (1.8125 - 0.8125 * contrast.min(1.99))
        } else {
            0.28125 + 0.71875 * contrast.max(0.01)
        };

        for o in out.iter_mut() {
            let t = *o;
            let mut out_color = (t - self.pivot) * contrast + self.pivot;

            // Top end (index 0).
            {
                let x1 = self.sc_x[0][1];
                let x2 = self.sc_x[0][2];
                let y1 = self.sc_y[0][1];
                let y2 = self.sc_y[0][2];
                let m0 = self.sc_m[0][0];
                let m3 = self.sc_m[0][1];

                let t_r = (t - x1) / (x2 - x1);
                let res = t_r * (x2 - x1) * (t_r * 0.5 * (m3 - m0) + m0) + y1;
                if t >= x1 {
                    out_color = res;
                }
                let res2 = y2 + (t - x2) * m3;
                if t >= x2 {
                    out_color = res2;
                }
            }

            // Bottom end (index 1).
            {
                let x1 = self.sc_x[1][1];
                let x2 = self.sc_x[1][2];
                let y1 = self.sc_y[1][1];
                let m0 = self.sc_m[1][0];
                let m3 = self.sc_m[1][1];

                let t_r = (t - x1) / (x2 - x1);
                let res = t_r * (x2 - x1) * (t_r * 0.5 * (m3 - m0) + m0) + y1;
                // setOnLimit(outColor, t, x2, res, outColor): t < x2 -> res.
                if t < x2 {
                    out_color = res;
                }
                let res1 = y1 + (t - x1) * m0;
                // setOnLimit(outColor, t, x1, res1, outColor): t < x1 -> res1.
                if t < x1 {
                    out_color = res1;
                }
            }

            *o = out_color;
        }
    }

    /// Inverse saturation contrast. OCIO ref: `GradingToneRevOpCPU::scontrast`.
    fn scontrast_rev(&self, v: &GradingTone, out: &mut [f32; 3]) {
        let mut contrast = v.s_contrast as f32;
        if contrast == 1.0 {
            return;
        }
        contrast = if contrast > 1.0 {
            1.0 / (1.8125 - 0.8125 * contrast.min(1.99))
        } else {
            0.28125 + 0.71875 * contrast.max(0.01)
        };

        for o in out.iter_mut() {
            let t = *o;
            let mut out_color = (t - self.pivot) / contrast + self.pivot;

            // Top end (index 0).
            {
                let x1 = self.sc_x[0][1];
                let x2 = self.sc_x[0][2];
                let y1 = self.sc_y[0][1];
                let y2 = self.sc_y[0][2];
                let m0 = self.sc_m[0][0];
                let m3 = self.sc_m[0][1];

                let b = m0 * (x2 - x1);
                let a = (m3 - m0) * 0.5 * (x2 - x1);
                let cc = y1 - t;
                let discrim = (b * b - 4.0 * a * cc).sqrt();
                let res = (x2 - x1) * (-2.0 * cc) / (discrim + b) + x1;

                // setOnLimit(outColor, t, y1, outColor, res): t >= y1 -> res.
                if t >= y1 {
                    out_color = res;
                }
                // setOnLimit(outColor, t, y2, outColor, x2 + (t-y2)/m3): t >= y2.
                if t >= y2 {
                    out_color = x2 + (t - y2) / m3;
                }
            }

            // Bottom end (index 1).
            {
                let x1 = self.sc_x[1][1];
                let x2 = self.sc_x[1][2];
                let y1 = self.sc_y[1][1];
                let y2 = self.sc_y[1][2];
                let m0 = self.sc_m[1][0];
                let m3 = self.sc_m[1][1];

                let b = m0 * (x2 - x1);
                let a = (m3 - m0) * 0.5 * (x2 - x1);
                let cc = y1 - t;
                let discrim = (b * b - 4.0 * a * cc).sqrt();
                let res = (x2 - x1) * (-2.0 * cc) / (discrim + b) + x1;

                // setOnLimit(outColor, t, y2, res, outColor): t < y2 -> res.
                if t < y2 {
                    out_color = res;
                }
                // setOnLimit(outColor, t, y1, x1 + (t-y1)/m0, outColor): t < y1.
                if t < y1 {
                    out_color = x1 + (t - y1) / m0;
                }
            }

            *o = out_color;
        }
    }

    /// Linear-style lin->log breakpoint transform (per RGB component).
    /// OCIO ref: GradingToneOpCPU.cpp `LinLog` (scalar branch).
    fn lin_log(out: &mut [f32; 3]) {
        for v in out.iter_mut() {
            *v = if *v < LL_XBRK {
                *v * LL_GAIN + LL_OFFS
            } else {
                LL_BASE2 * ((*v + LL_SHIFT) * LL_M).ln()
            };
        }
    }

    /// Linear-style log->lin inverse breakpoint transform (per RGB component).
    /// OCIO ref: GradingToneOpCPU.cpp `LogLin` (scalar branch).
    fn log_lin(out: &mut [f32; 3]) {
        for v in out.iter_mut() {
            *v = if *v < LL_YBRK {
                (*v - LL_OFFS) / LL_GAIN
            } else {
                2.0_f32.powf(*v) * (0.18 + LL_SHIFT) - LL_SHIFT
            };
        }
    }

    /// Clamp each component to the max half-float (OCIO `ClampMaxRGB`).
    fn clamp_max_rgb(out: &mut [f32; 3]) {
        for v in out.iter_mut() {
            *v = v.min(65504.0);
        }
    }

    /// Applies the full GradingTone forward chain to one RGB pixel.
    /// OCIO ref: `GradingToneFwdOpCPU::apply` / `GradingToneLinearFwdOpCPU::apply`.
    pub fn apply_forward(&self, v: &GradingTone, out: &mut [f32; 3]) {
        if self.local_bypass {
            return;
        }
        let linear = self.style == GradingToneStyle::Linear;
        if linear {
            Self::lin_log(out);
        }

        for ch in CHANNELS {
            self.mids_fwd(v, ch, out);
        }
        for ch in CHANNELS {
            self.highlight_shadow_fwd(v, ch, false, out);
        }
        for ch in CHANNELS {
            self.white_black_fwd(v, ch, false, out);
        }
        for ch in CHANNELS {
            self.highlight_shadow_fwd(v, ch, true, out);
        }
        for ch in CHANNELS {
            self.white_black_fwd(v, ch, true, out);
        }
        self.scontrast_fwd(v, out);

        if linear {
            Self::log_lin(out);
        }
        Self::clamp_max_rgb(out);
    }

    /// Applies the full GradingTone inverse chain to one RGB pixel.
    /// OCIO ref: `GradingToneRevOpCPU::apply` / `GradingToneLinearRevOpCPU::apply`.
    pub fn apply_inverse(&self, v: &GradingTone, out: &mut [f32; 3]) {
        if self.local_bypass {
            return;
        }
        let linear = self.style == GradingToneStyle::Linear;
        if linear {
            Self::lin_log(out);
        }

        self.scontrast_rev(v, out);

        // Order: M, R, G, B for each zone (GradingToneRevOpCPU::apply).
        for ch in CHANNELS_REV {
            self.white_black_rev(v, ch, true, out);
        }
        for ch in CHANNELS_REV {
            self.highlight_shadow_rev(v, ch, true, out);
        }
        for ch in CHANNELS_REV {
            self.white_black_rev(v, ch, false, out);
        }
        for ch in CHANNELS_REV {
            self.highlight_shadow_rev(v, ch, false, out);
        }
        for ch in CHANNELS_REV {
            self.mids_rev(v, ch, out);
        }

        if linear {
            Self::log_lin(out);
        }
        Self::clamp_max_rgb(out);
    }
}

/// Forward apply order for zones: R, G, B, M.
const CHANNELS: [RgbmChannel; 4] = [
    RgbmChannel::R,
    RgbmChannel::G,
    RgbmChannel::B,
    RgbmChannel::M,
];

/// Inverse apply order for zones: M, R, G, B.
const CHANNELS_REV: [RgbmChannel; 4] = [
    RgbmChannel::M,
    RgbmChannel::R,
    RgbmChannel::G,
    RgbmChannel::B,
];

// LogLinConstants (GradingToneOpCPU.cpp, scalar branch) — taken from the single source rather
// than re-typed here. `vfx_core::consts`' module doc records this block as having had 6+ copies
// with two of them wrong; these locals were verified bit-identical to the source before being
// replaced, so nothing moved numerically. Aliased to keep the `LL_` call sites unchanged.
use vfx_core::consts::loglin::{
    BASE2 as LL_BASE2, GAIN as LL_GAIN, M as LL_M, OFFS as LL_OFFS, SHIFT as LL_SHIFT,
    XBRK as LL_XBRK, YBRK as LL_YBRK,
};

/// Maps 0..3 to the RGBM channel enum.
#[inline]
fn idx_to_channel(ch: usize) -> RgbmChannel {
    match ch {
        0 => RgbmChannel::R,
        1 => RgbmChannel::G,
        2 => RgbmChannel::B,
        _ => RgbmChannel::M,
    }
}

/// Float clamp matching OCIO `Clamp(v, lo, hi)`.
#[inline]
fn clamp_f32(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// Applies a scalar curve `f` either to one component (R/G/B) or all three (M).
/// Mirrors OCIO's `if (channel != M) { single } else { float3 }` dispatch.
#[inline]
fn apply_master_or_single<F: Fn(f32) -> f32>(channel: RgbmChannel, out: &mut [f32; 3], f: F) {
    if channel != RgbmChannel::M {
        let ch = channel as usize;
        out[ch] = f(out[ch]);
    } else {
        for v in out.iter_mut() {
            *v = f(*v);
        }
    }
}

// ---- Scalar curve evaluators (single-component; reused for each RGB lane) ----

/// Forward midtones 6-knot faux-cubic eval. OCIO ref: `GradingToneFwdOpCPU::mids`.
#[inline]
fn mids_fwd_eval(t: f32, x: &[f32; 6], y: &[f32; 6], m: &[f32; 6]) -> f32 {
    let t_l = (t - x[0]) / (x[1] - x[0]);
    let t_m = (t - x[1]) / (x[2] - x[1]);
    let t_r = (t - x[2]) / (x[3] - x[2]);
    let t_r2 = (t - x[3]) / (x[4] - x[3]);
    let t_r3 = (t - x[4]) / (x[5] - x[4]);

    let f_l = t_l * (x[1] - x[0]) * (t_l * 0.5 * (m[1] - m[0]) + m[0]) + y[0];
    let f_m = t_m * (x[2] - x[1]) * (t_m * 0.5 * (m[2] - m[1]) + m[1]) + y[1];
    let f_r = t_r * (x[3] - x[2]) * (t_r * 0.5 * (m[3] - m[2]) + m[2]) + y[2];
    let f_r2 = t_r2 * (x[4] - x[3]) * (t_r2 * 0.5 * (m[4] - m[3]) + m[3]) + y[3];
    let f_r3 = t_r3 * (x[5] - x[4]) * (t_r3 * 0.5 * (m[5] - m[4]) + m[4]) + y[4];

    let mut res = if t < x[1] { f_l } else { f_m };
    if t > x[2] {
        res = f_r;
    }
    if t > x[3] {
        res = f_r2;
    }
    if t > x[4] {
        res = f_r3;
    }
    if t < x[0] {
        res = y[0] + (t - x[0]) * m[0];
    }
    if t > x[5] {
        res = y[5] + (t - x[5]) * m[5];
    }
    res
}

/// Inverse midtones eval (solve the per-segment quadratic for x).
/// OCIO ref: `GradingToneRevOpCPU::mids` (single-channel branch).
#[inline]
fn mids_rev_eval(t: f32, x: &[f32; 6], y: &[f32; 6], m: &[f32; 6]) -> f32 {
    // Mirrors the master-channel float3 branch (the per-component one matches it):
    // the single-channel C++ branch has a typo `t >= y5 -> x0+(t-y0)/m0`, but the
    // float3 branch (used for M, and the correct general inverse) uses x5+(t-y5)/m5.
    // We port the float3 branch for all channels for correctness/round-trip.
    let solve = |c: f32, b: f32, a: f32, lo_x: f32, hi_x: f32| -> f32 {
        let discrim = (b * b - 4.0 * a * c).sqrt();
        let tmp = (2.0 * c) / (-b - discrim);
        tmp * (hi_x - lo_x) + lo_x
    };

    let out_r4 = x[5] + (t - y[5]) / m[5];
    let out_r3 = solve(
        y[4] - t,
        m[4] * (x[5] - x[4]),
        0.5 * (m[5] - m[4]) * (x[5] - x[4]),
        x[4],
        x[5],
    );
    let out_r2 = solve(
        y[3] - t,
        m[3] * (x[4] - x[3]),
        0.5 * (m[4] - m[3]) * (x[4] - x[3]),
        x[3],
        x[4],
    );
    let out_r = solve(
        y[2] - t,
        m[2] * (x[3] - x[2]),
        0.5 * (m[3] - m[2]) * (x[3] - x[2]),
        x[2],
        x[3],
    );
    let out_m = solve(
        y[1] - t,
        m[1] * (x[2] - x[1]),
        0.5 * (m[2] - m[1]) * (x[2] - x[1]),
        x[1],
        x[2],
    );
    let out_l = solve(
        y[0] - t,
        m[0] * (x[1] - x[0]),
        0.5 * (m[1] - m[0]) * (x[1] - x[0]),
        x[0],
        x[1],
    );
    let out_l0 = x[0] + (t - y[0]) / m[0];

    // res.setOnLimit(t, y1, outL, outM); etc. setOnLimit(res, t, lim, below, above):
    // res = (t < lim) ? below : above.
    let mut res = if t < y[1] { out_l } else { out_m };
    if t >= y[2] {
        res = out_r;
    }
    if t >= y[3] {
        res = out_r2;
    }
    if t >= y[4] {
        res = out_r3;
    }
    if t < y[0] {
        res = out_l0;
    }
    if t >= y[5] {
        res = out_r4;
    }
    res
}

/// Forward highlight/shadow quadratic-Bezier eval.
/// OCIO ref: `ComputeHSFwd`.
#[inline]
fn compute_hs_fwd(t: f32, x: &[f32; 3], y: &[f32; 3], m0: f32, m2: f32) -> f32 {
    let (x0, x1, x2) = (x[0], x[1], x[2]);
    let (y0, y1, y2) = (y[0], y[1], y[2]);
    let t_l = (t - x0) / (x1 - x0);
    let t_r = (t - x1) / (x2 - x1);
    let f_l = y0 * (1.0 - t_l * t_l) + y1 * t_l * t_l + m0 * (1.0 - t_l) * t_l * (x1 - x0);
    let f_r = y1 * (1.0 - t_r) * (1.0 - t_r)
        + y2 * (2.0 - t_r) * t_r
        + m2 * (t_r - 1.0) * t_r * (x2 - x1);

    let mut res = if t < x1 { f_l } else { f_r };
    if t < x0 {
        res = (t - x0) * m0 + y0;
    }
    if t > x2 {
        res = (t - x2) * m2 + y2;
    }
    res
}

/// Inverse highlight/shadow eval (solve the two quadratics for x).
/// OCIO ref: `ComputeHSRev`.
#[inline]
fn compute_hs_rev(t: f32, x: &[f32; 3], y: &[f32; 3], m0: f32, m2: f32) -> f32 {
    let (x0, x1, x2) = (x[0], x[1], x[2]);
    let (y0, y1, y2) = (y[0], y[1], y[2]);

    let b_l = m0 * (x1 - x0);
    let a_l = y1 - y0 - m0 * (x1 - x0);
    let c_l = y0 - t;
    let discrim_l = (b_l * b_l - 4.0 * a_l * c_l).sqrt();
    let out_l = (-2.0 * c_l) / (discrim_l + b_l) * (x1 - x0) + x0;

    let b_r = 2.0 * y2 - 2.0 * y1 - m2 * (x2 - x1);
    let a_r = y1 - y2 + m2 * (x2 - x1);
    let c_r = y1 - t;
    let discrim_r = (b_r * b_r - 4.0 * a_r * c_r).sqrt();
    let out_r = (-2.0 * c_r) / (discrim_r + b_r) * (x2 - x1) + x1;

    let mut res = if t < y1 { out_l } else { out_r };
    if t < y0 {
        res = (t - y0) / m0 + x0;
    }
    if t > y2 {
        res = (t - y2) / m2 + x2;
    }
    res
}

/// Forward white/black eval. OCIO ref: `ComputeWBFwd`.
#[inline]
#[allow(clippy::too_many_arguments)]
fn compute_wb_fwd(
    is_black: bool,
    val: f32,
    x0: f32,
    x1: f32,
    y0: f32,
    y1: f32,
    m0: f32,
    m1: f32,
    gain: f32,
    t: f32,
) -> f32 {
    let mtest = if !is_black { val } else { 2.0 - val };

    if mtest < 1.0 {
        // Slope is decreasing case.
        let tlocal = (t - x0) / (x1 - x0);
        let mut res = tlocal * (x1 - x0) * (tlocal * 0.5 * (m1 - m0) + m0) + y0;
        if t < x0 {
            res = y0 + (t - x0) * m0;
        }
        if t > x1 {
            res = y1 + (t - x1) * m1;
        }
        res
    } else if mtest > 1.0 {
        // Slope is increasing case.
        let mut t = if !is_black {
            (t - x0) * gain + x0
        } else {
            (t - x1) * gain + x1
        };

        let a = 0.5 * (m1 - m0) * (x1 - x0);
        let b = m0 * (x1 - x0);
        let c = y0 - t;
        let discrim = (b * b - 4.0 * a * c).sqrt();
        let tmp = (-2.0 * c) / (discrim + b);
        let mut res = tmp * (x1 - x0) + x0;
        if t < y0 {
            res = x0 + (t - y0) / m0;
        }

        if !is_black {
            res = (res - x0) / gain + x0;
            // Quadratic extrapolation for HDR control.
            let new_y1 = (x1 - x0) / gain + x0;
            let xd = x0 + (x1 - x0) * 0.99;
            let mut md = m0 + (xd - x0) * (m1 - m0) / (x1 - x0);
            md = 1.0 / md;
            let aa = 0.5 * (1.0 / m1 - md) / (x1 - xd);
            let bb = 1.0 / m1 - 2.0 * aa * x1;
            let cc = new_y1 - bb * x1 - aa * x1 * x1;
            t = (t - x0) / gain + x0;
            let res1 = (aa * t + bb) * t + cc;
            // `>=`, not `>`: ref `setOnLimit(res, t, x1, res, res1)` (GradingToneOpCPU.cpp:694)
            // is `t < x1 ? res : res1`. The extrapolation piece is fitted independently of the
            // gain-mapped piece and they are not required to meet at x1 — equality has magnitude.
            if t >= x1 {
                res = res1;
            }
            res
        } else {
            let res1 = x1 + (t - y1) / m1;
            // Ref `:699` — same `setOnLimit` semantics at y1.
            if t >= y1 {
                res = res1;
            }
            (res - x1) / gain + x1
        }
    } else {
        // mtest == 1: identity for this channel.
        t
    }
}

/// Inverse white/black eval. OCIO ref: `ComputeWBRev`.
#[inline]
#[allow(clippy::too_many_arguments)]
fn compute_wb_rev(
    is_black: bool,
    val: f32,
    x0: f32,
    x1: f32,
    y0: f32,
    y1: f32,
    m0: f32,
    m1: f32,
    gain: f32,
    t: f32,
) -> f32 {
    let mtest = if !is_black { val } else { 2.0 - val };

    if mtest < 1.0 {
        // Slope is decreasing case (solve the fwd quadratic for x).
        let a = 0.5 * (m1 - m0) * (x1 - x0);
        let b = m0 * (x1 - x0);
        let c = y0 - t;
        let discrim = (b * b - 4.0 * a * c).sqrt();
        let tmp = (-2.0 * c) / (discrim + b);
        let mut res = tmp * (x1 - x0) + x0;
        if t < y0 {
            res = x0 + (t - y0) / m0;
        }
        if t > y1 {
            res = x1 + (t - y1) / m1;
        }
        res
    } else if mtest > 1.0 {
        // Slope is increasing case.
        let mut t = if !is_black {
            (t - x0) * gain + x0
        } else {
            (t - x1) * gain + x1
        };

        let tlocal = (t - x0) / (x1 - x0);
        let mut res = tlocal * (x1 - x0) * (tlocal * 0.5 * (m1 - m0) + m0) + y0;
        if t < x0 {
            res = y0 + (t - x0) * m0;
        }

        if !is_black {
            res = (res - x0) / gain + x0;
            // Quadratic extrapolation for HDR control.
            let new_y1 = (x1 - x0) / gain + x0;
            let xd = x0 + (x1 - x0) * 0.99;
            let mut md = m0 + (xd - x0) * (m1 - m0) / (x1 - x0);
            md = 1.0 / md;
            let aa = 0.5 * (1.0 / m1 - md) / (x1 - xd);
            let bb = 1.0 / m1 - 2.0 * aa * x1;
            let cc = new_y1 - bb * x1 - aa * x1 * x1;
            t = (t - x0) / gain + x0;

            let c = cc - t;
            let discrim = (bb * bb - 4.0 * aa * c).sqrt();
            let res1 = (-2.0 * c) / (discrim + bb);
            let brk = (aa * x1 + bb) * x1 + cc;
            // Ref `:762` setOnLimit — `>=` selects the inverse-extrapolation branch.
            if t >= brk {
                res = res1;
            }
            res
        } else {
            let res1 = y1 + (t - x1) * m1;
            // Ref `:767`.
            if t >= x1 {
                res = res1;
            }
            (res - x1) / gain + x1
        }
    } else {
        // mtest == 1: identity.
        t
    }
}

// ---- precompute-only faux-cubic helpers (used by `update`) ----

/// A faux-cubic curve: end points `(x0, y0)` and `(x2, y2)` with slopes `m0` and `m2`, joined
/// at `x1`. OCIO `FauxCubicFwdEval`/`FauxCubicRevEval` take these as seven scalars.
#[derive(Debug, Clone, Copy)]
struct FauxCubic {
    x0: f64,
    x1: f64,
    x2: f64,
    y0: f64,
    y2: f64,
    m0: f64,
    m2: f64,
}

/// Forward faux-cubic eval used during range resolution.
/// OCIO ref: GradingTone.cpp anonymous `FauxCubicFwdEval` (double precision).
fn faux_cubic_fwd_eval(t: f64, curve: &FauxCubic) -> f64 {
    let FauxCubic {
        x0,
        x1,
        x2,
        y0,
        y2,
        m0,
        m2,
    } = *curve;
    let y1 = (0.5 / ((x2 - x1) + (x1 - x0)))
        * ((2.0 * y0 + m0 * (x1 - x0)) * (x2 - x1) + (2.0 * y2 - m2 * (x2 - x1)) * (x1 - x0));
    let t_l = (t - x0) / (x1 - x0);
    let t_r = (t - x1) / (x2 - x1);
    let f_l = y0 * (1.0 - t_l * t_l) + y1 * t_l * t_l + m0 * (1.0 - t_l) * t_l * (x1 - x0);
    let f_r = y1 * (1.0 - t_r) * (1.0 - t_r)
        + y2 * (2.0 - t_r) * t_r
        + m2 * (t_r - 1.0) * t_r * (x2 - x1);

    let mut res = if t < x1 { f_l } else { f_r };
    if t < x0 {
        res = y0 + (t - x0) * m0;
    }
    if t > x2 {
        res = y2 + (t - x2) * m2;
    }
    res
}

/// Inverse faux-cubic eval used during range resolution.
/// OCIO ref: GradingTone.cpp anonymous `FauxCubicRevEval` (double precision).
fn faux_cubic_rev_eval(t: f64, curve: &FauxCubic) -> f64 {
    let FauxCubic {
        x0,
        x1,
        x2,
        y0,
        y2,
        m0,
        m2,
    } = *curve;
    let y1 = (0.5 / ((x2 - x1) + (x1 - x0)))
        * ((2.0 * y0 + m0 * (x1 - x0)) * (x2 - x1) + (2.0 * y2 - m2 * (x2 - x1)) * (x1 - x0));

    let c_l = y0 - t;
    let b_l = m0 * (x1 - x0);
    let a_l = y1 - y0 - m0 * (x1 - x0);
    let discrim_l = (b_l * b_l - 4.0 * a_l * c_l).sqrt();
    let tmp_l = (2.0 * c_l) / (-discrim_l - b_l);
    let out_l = tmp_l * (x1 - x0) + x0;

    let c_r = y1 - t;
    let b_r = 2.0 * y2 - 2.0 * y1 - m2 * (x2 - x1);
    let a_r = y1 - y2 + m2 * (x2 - x1);
    let discrim_r = (b_r * b_r - 4.0 * a_r * c_r).sqrt();
    let tmp_r = (2.0 * c_r) / (-discrim_r - b_r);
    let out_r = tmp_r * (x2 - x1) + x1;

    let mut res = if t < y1 { out_l } else { out_r };
    if t < y0 {
        res = x0 + (t - y0) / m0;
    }
    if t > y2 {
        res = x2 + (t - y2) / m2;
    }
    res
}

/// Highlight forward eval used to resolve whites start/width.
/// OCIO ref: GradingTone.cpp anonymous `HighlightFwdEval`.
fn highlight_fwd_eval(t: f64, start: f64, pivot: f64, mut val: f64) -> f64 {
    let x0 = start;
    let x2 = pivot;
    let y0 = x0;
    let y2 = x2;
    let m0 = 1.0;
    let x1 = x0 + (x2 - x0) * 0.5;
    val = 2.0 - val;
    if val <= 1.0 {
        let m2 = if val < 0.01 { 0.01 } else { val };
        faux_cubic_fwd_eval(
            t,
            &FauxCubic {
                x0,
                x1,
                x2,
                y0,
                y2,
                m0,
                m2,
            },
        )
    } else {
        let m2 = if 2.0 - val < 0.01 { 0.01 } else { 2.0 - val };
        faux_cubic_rev_eval(
            t,
            &FauxCubic {
                x0,
                x1,
                x2,
                y0,
                y2,
                m0,
                m2,
            },
        )
    }
}

/// Shadow forward eval used to resolve blacks start/width.
/// OCIO ref: GradingTone.cpp anonymous `ShadowFwdEval`.
fn shadow_fwd_eval(t: f64, start: f64, pivot: f64, val: f64) -> f64 {
    let x0 = start;
    let x2 = pivot;
    let y0 = x0;
    let y2 = x2;
    let m2 = 1.0;
    let x1 = x0 + (x2 - x0) * 0.5;
    if val <= 1.0 {
        let m0 = if val < 0.01 { 0.01 } else { val };
        faux_cubic_fwd_eval(
            t,
            &FauxCubic {
                x0,
                x1,
                x2,
                y0,
                y2,
                m0,
                m2,
            },
        )
    } else {
        let m0 = if 2.0 - val < 0.01 { 0.01 } else { 2.0 - val };
        faux_cubic_rev_eval(
            t,
            &FauxCubic {
                x0,
                x1,
                x2,
                y0,
                y2,
                m0,
                m2,
            },
        )
    }
}
