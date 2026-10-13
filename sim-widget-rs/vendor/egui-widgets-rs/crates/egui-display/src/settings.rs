use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayPrefs {
    /// The surface format + colour space (`present::Output`).
    #[serde(deserialize_with = "output_or_default")]
    pub output: crate::Output,
    /// SDR reference white in nits on an HDR output (the UI and canvas 1.0);
    /// `None` = what the OS reports (Windows' SDR brightness slider), else the
    /// output's default (`present::Output::default_white`).
    pub white: Option<f32>,
    /// The display's nominal peak in nits, the `L_W` of the HLG OOTF; `None` =
    /// what the OS reports, else BT.2100's 1000 (`present::HLG_REF_PEAK`).
    pub peak: Option<f32>,
}

/// An output this build does not know (a config written by a newer one) reads as
/// the default output, logged: one unknown name must not fail `Config::load`, which
/// would drop the whole config (recents, layouts) for the defaults.
fn output_or_default<'de, D: serde::Deserializer<'de>>(d: D) -> Result<crate::Output, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value(v.clone()).unwrap_or_else(|e| {
        log::warn!("persist: unknown display output {v} ({e}); using the default");
        crate::Output::default()
    }))
}

impl DisplayPrefs {
    /// Manual SDR reference white, nits; independent of the HDR rendering peak.
    /// The default still comes from the OS or the output, not this upper bound.
    pub const WHITE: std::ops::RangeInclusive<f32> = 80.0..=1000.0;
    /// Allowed display peak, nits: where BT.2100 gives the HLG system gamma
    /// `1.2 + 0.42 log10(L_W / 1000)` (its note on displays of 400 to 2000 nits).
    pub const PEAK: std::ops::RangeInclusive<f32> = 400.0..=2000.0;

    /// The one rule for what the output encodes for: each level is the user's
    /// (clamped to its range), else the OS's (`DisplayHdrInfo`: SDR white, peak),
    /// else the output's default.
    pub fn target(&self, output: crate::Output, info: &wgpu::DisplayHdrInfo) -> crate::Target {
        let lum = info.luminance;
        crate::Target {
            hdr: output.is_hdr(),
            white: level(self.white, lum.and_then(|l| l.sdr_white_nits), &Self::WHITE)
                .unwrap_or_else(|| output.default_white()),
            peak: level(self.peak, lum.and_then(|l| l.max_nits), &Self::PEAK)
                .unwrap_or(crate::HLG_REF_PEAK),
        }
    }
}

/// A light level: the user's if set, else the OS's if it reports a usable one,
/// each clamped to `range`.
fn level(user: Option<f32>, os: Option<f32>, range: &std::ops::RangeInclusive<f32>) -> Option<f32> {
    user.filter(|v| v.is_finite() && *v > 0.0)
        .or(os.filter(|v| v.is_finite() && *v > 0.0))
        .map(|v| v.clamp(*range.start(), *range.end()))
}

/// Runtime facts for one window, supplied by the host before drawing Preferences.
#[derive(Clone, Debug)]
pub struct DisplayState {
    pub output: crate::Output,
    pub target: crate::Target,
    pub info: wgpu::DisplayHdrInfo,
    pub error: Option<String>,
    pub available: Vec<crate::Output>,
}
/// Context slot shared by a window host and its preference panel.
pub fn state_id() -> egui::Id {
    egui::Id::new("egui_display.state")
}

/// Edit display settings. Persistence and surface reconfiguration belong to the host.
pub fn settings_ui(
    ui: &mut egui::Ui,
    prefs: &mut DisplayPrefs,
    state: Option<&DisplayState>,
) -> bool {
    let before = *prefs;
    ui.horizontal(|ui| {
        ui.label("Output");
        egui::ComboBox::from_id_salt("display_output")
            .selected_text(prefs.output.label())
            .show_ui(ui, |ui| {
                for output in crate::Output::ALL {
                    let supported = state.is_some_and(|s| s.available.contains(&output));
                    let hint = if supported {
                        output.hint().to_owned()
                    } else {
                        format!(
                            "{}\n\nNot offered by this display / GPU (HDR off in the OS?).",
                            output.hint()
                        )
                    };
                    ui.add_enabled_ui(supported, |ui| {
                        ui.selectable_value(&mut prefs.output, output, output.label())
                            .on_hover_text(hint);
                    });
                }
            });
    });
    if let Some(state) = state {
        ui.label(format!("Active output: {}", state.output.label()));
        if let Some(error) = &state.error {
            ui.colored_label(ui.visuals().warn_fg_color, error);
        }
        if state.output.is_hdr() || prefs.output.is_hdr() {
            level_ui(
                ui,
                "SDR reference white",
                &mut prefs.white,
                state.target.white,
                DisplayPrefs::WHITE,
            );
            if state.output == crate::Output::Hlg || prefs.output == crate::Output::Hlg {
                level_ui(
                    ui,
                    "HLG display peak",
                    &mut prefs.peak,
                    state.target.peak,
                    DisplayPrefs::PEAK,
                );
            }
            ui.weak("Auto uses the display's values reported by the operating system.");
            ui.weak("Reference white sets UI brightness; it does not limit PQ highlights. HLG display peak affects the HLG signal only.");
        }
        if state.output.is_hdr()
            && state.info.coarse.and_then(|c| c.high_dynamic_range) == Some(false)
        {
            ui.colored_label(ui.visuals().warn_fg_color, "The display reports SDR (HDR off in the OS?): values above white are clipped by the OS.");
        }
        ui.separator();
        ui.label("Display (as reported by the OS)");
        let lum = state.info.luminance;
        let nits = |n: Option<f32>| n.map_or("unknown".to_owned(), |v| format!("{v:.0} nits"));
        egui::Grid::new("display_os_info").show(ui, |ui| {
            for (label, value) in [
                ("Peak", nits(lum.and_then(|l| l.max_nits))),
                ("Full frame", nits(lum.and_then(|l| l.max_full_frame_nits))),
                ("SDR white", nits(lum.and_then(|l| l.sdr_white_nits))),
                (
                    "Headroom",
                    state
                        .info
                        .tone_map_headroom()
                        .map_or("unknown".to_owned(), |v| format!("{v:.2}x SDR white")),
                ),
                (
                    "Bits per colour",
                    state
                        .info
                        .bits_per_color
                        .map_or("unknown".to_owned(), |v| v.to_string()),
                ),
            ] {
                ui.label(label);
                ui.label(value);
                ui.end_row();
            }
        });
    } else {
        ui.weak("Display capabilities are unavailable in this host.");
    }
    before != *prefs
}
fn level_ui(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut Option<f32>,
    automatic: f32,
    range: std::ops::RangeInclusive<f32>,
) {
    ui.horizontal(|ui| {
        ui.label(label);
        let mut auto = value.is_none();
        if ui.checkbox(&mut auto, "Auto").changed() {
            *value = if auto {
                None
            } else {
                Some(automatic.clamp(*range.start(), *range.end()))
            };
        }
        if let Some(v) = value {
            ui.add(
                egui::DragValue::new(v)
                    .range(range)
                    .speed(1.0)
                    .suffix(" nits"),
            );
        } else {
            ui.weak(format!("{automatic:.0} nits"));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bad_manual_levels_do_not_poison_the_output() {
        let p = DisplayPrefs {
            output: crate::Output::Scrgb,
            white: Some(f32::NAN),
            peak: Some(f32::INFINITY),
        };
        let target = p.target(p.output, &wgpu::DisplayHdrInfo::default());
        assert_eq!(target.white, crate::SCRGB_NITS);
        assert_eq!(target.peak, crate::HLG_REF_PEAK);
    }
    #[test]
    fn preferences_round_trip() {
        let p = DisplayPrefs {
            output: crate::Output::Hdr10,
            white: Some(240.0),
            peak: Some(600.0),
        };
        assert_eq!(
            serde_json::from_str::<DisplayPrefs>(&serde_json::to_string(&p).unwrap()).unwrap(),
            p
        );
    }
}
