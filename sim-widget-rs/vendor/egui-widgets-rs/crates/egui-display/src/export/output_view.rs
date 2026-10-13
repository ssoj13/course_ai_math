//! Output-view selection extracted from WarpBro's OCIO adapter.

use super::PngEncoding;

/// The display signal an output file carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputKind {
    Sdr,
    Pq,
    Hlg,
}

impl From<PngEncoding> for OutputKind {
    fn from(encoding: PngEncoding) -> Self {
        match encoding {
            PngEncoding::Sdr8 => Self::Sdr,
            PngEncoding::Hdr10 => Self::Pq,
            PngEncoding::Hlg => Self::Hlg,
        }
    }
}

impl OutputKind {
    /// OCIO declares HDR views without distinguishing PQ from HLG. ACES display
    /// names carry that distinction, following WarpBro's existing convention.
    pub fn display_is(self, display: &str) -> bool {
        let name = display.to_ascii_uppercase();
        match self {
            Self::Sdr => true,
            Self::Pq => name.contains("PQ") || name.contains("ST2084") || name.contains("ST-2084"),
            Self::Hlg => name.contains("HLG"),
        }
    }
}

/// A view's declared output category. Unknown or untagged picture views use
/// `Picture`, as in WarpBro's OCIO v1 handling; raw/data views use `Data`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputViewEncoding {
    Picture,
    Hdr,
    Data,
}

impl OutputViewEncoding {
    pub fn fits(self, kind: OutputKind) -> bool {
        match kind {
            OutputKind::Sdr => self == Self::Picture,
            OutputKind::Pq | OutputKind::Hlg => self == Self::Hdr,
        }
    }
}

/// Borrowed metadata supplied by the application's active OCIO config, in its
/// display/view order. Processors and measured peaks remain owned by the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputView<'a> {
    pub display: &'a str,
    pub view: &'a str,
    pub encoding: OutputViewEncoding,
}

impl OutputView<'_> {
    pub fn fits(self, kind: OutputKind) -> bool {
        kind.display_is(self.display) && self.encoding.fits(kind)
    }
}

/// Keep a compatible current choice. Otherwise select the first compatible
/// display and its first SDR view or HDR view closest to the requested measured
/// peak. Only the chosen display's HDR views are probed, once each.
///
/// An unmeasurable HDR view ranks after measurable ones. If none is measurable,
/// config order wins and the host must report the selected processor's error.
pub fn resolve_output_view<'a>(
    current: Option<(&str, &str)>,
    kind: OutputKind,
    peak_nits: f32,
    candidates: &[OutputView<'a>],
    mut measure_peak: impl FnMut(&str, &str) -> Option<f32>,
) -> Result<OutputView<'a>, String> {
    if let Some((display, view)) = current
        && let Some(candidate) = candidates.iter().copied().find(|candidate| {
            candidate.display == display && candidate.view == view && candidate.fits(kind)
        })
    {
        return Ok(candidate);
    }
    let Some(first) = candidates
        .iter()
        .copied()
        .find(|candidate| candidate.fits(kind))
    else {
        return Err(format!(
            "the config has no {} display: choose the output transform",
            match kind {
                OutputKind::Sdr => "SDR",
                OutputKind::Pq => "PQ (HDR10)",
                OutputKind::Hlg => "HLG",
            }
        ));
    };
    if kind == OutputKind::Sdr {
        return Ok(first);
    }
    Ok(candidates
        .iter()
        .copied()
        .filter(|candidate| candidate.display == first.display && candidate.fits(kind))
        .map(|candidate| {
            let distance = measure_peak(candidate.display, candidate.view)
                .filter(|peak| peak.is_finite() && *peak > 0.0)
                .map_or(f32::MAX, |peak| (peak - peak_nits).abs());
            (candidate, distance)
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .expect("the first compatible HDR view remains in its display")
        .0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWS: [OutputView<'static>; 7] = [
        OutputView {
            display: "sRGB",
            view: "Raw",
            encoding: OutputViewEncoding::Data,
        },
        OutputView {
            display: "sRGB",
            view: "SDR",
            encoding: OutputViewEncoding::Picture,
        },
        OutputView {
            display: "Rec.2100-PQ - Display",
            view: "1000",
            encoding: OutputViewEncoding::Hdr,
        },
        OutputView {
            display: "Rec.2100-PQ - Display",
            view: "4000",
            encoding: OutputViewEncoding::Hdr,
        },
        OutputView {
            display: "Rec.2100-PQ - Display",
            view: "Unmeasurable",
            encoding: OutputViewEncoding::Hdr,
        },
        OutputView {
            display: "Other PQ",
            view: "2000",
            encoding: OutputViewEncoding::Hdr,
        },
        OutputView {
            display: "Rec.2100-HLG - Display",
            view: "HLG",
            encoding: OutputViewEncoding::Hdr,
        },
    ];

    #[test]
    fn compatible_current_choice_wins_without_probing() {
        let selected = resolve_output_view(
            Some((VIEWS[3].display, VIEWS[3].view)),
            OutputKind::Pq,
            1000.0,
            &VIEWS,
            |_, _| panic!("a compatible current choice needs no peak ranking"),
        )
        .unwrap();
        assert_eq!(selected, VIEWS[3]);
    }

    #[test]
    fn sdr_excludes_hdr_and_data_and_never_probes() {
        for current in [
            None,
            Some((VIEWS[0].display, VIEWS[0].view)),
            Some((VIEWS[3].display, VIEWS[3].view)),
        ] {
            let selected = resolve_output_view(current, OutputKind::Sdr, 1000.0, &VIEWS, |_, _| {
                panic!("SDR does not measure HDR peaks")
            })
            .unwrap();
            assert_eq!(selected, VIEWS[1]);
        }
    }

    #[test]
    fn hdr_ranks_only_first_compatible_display_using_measured_peaks() {
        let mut probes = Vec::new();
        let selected =
            resolve_output_view(None, OutputKind::Pq, 3000.0, &VIEWS, |display, view| {
                assert_eq!(display, VIEWS[2].display);
                probes.push(view.to_owned());
                match view {
                    "1000" => Some(1100.0),
                    "4000" => Some(3800.0),
                    _ => None,
                }
            })
            .unwrap();
        assert_eq!(selected, VIEWS[3]);
        assert_eq!(probes, ["1000", "4000", "Unmeasurable"]);
    }

    #[test]
    fn equal_or_unmeasurable_peaks_keep_config_order() {
        for peak in [None, Some(f32::NAN), Some(f32::INFINITY), Some(1000.0)] {
            let selected =
                resolve_output_view(None, OutputKind::Pq, 2000.0, &VIEWS, |_, _| peak).unwrap();
            assert_eq!(selected, VIEWS[2]);
        }
    }

    #[test]
    fn pq_and_hlg_use_distinct_display_conventions() {
        let selected = resolve_output_view(None, OutputKind::Hlg, 1000.0, &VIEWS, |display, _| {
            assert_eq!(display, VIEWS[6].display);
            Some(1000.0)
        })
        .unwrap();
        assert_eq!(selected, VIEWS[6]);
        for display in ["Rec.2100-pq", "ST2084", "ST-2084"] {
            assert!(OutputKind::Pq.display_is(display));
            assert!(!OutputKind::Hlg.display_is(display));
        }
    }

    #[test]
    fn missing_compatible_output_is_an_error() {
        for kind in [OutputKind::Sdr, OutputKind::Pq, OutputKind::Hlg] {
            assert!(resolve_output_view(None, kind, 1000.0, &[], |_, _| None).is_err());
            assert!(resolve_output_view(None, kind, 1000.0, &VIEWS[..1], |_, _| None).is_err());
        }
        assert_eq!(OutputKind::from(PngEncoding::Sdr8), OutputKind::Sdr);
        assert_eq!(OutputKind::from(PngEncoding::Hdr10), OutputKind::Pq);
        assert_eq!(OutputKind::from(PngEncoding::Hlg), OutputKind::Hlg);
    }
}
