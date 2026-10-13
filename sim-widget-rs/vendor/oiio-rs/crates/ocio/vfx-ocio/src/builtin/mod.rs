//! Built-in OCIO configurations.
//!
//! [`embedded`] holds OCIO's built-in configs (`BuiltinConfigRegistry`): the ACES CG and Studio
//! configs of every `OpenColorIO-Config-ACES` release OCIO 2.5 ships, compiled in, plus the
//! Studio all-views config. `Config::from_file("ocio://...")` and
//! [`Config::create_from_builtin_config`](crate::Config::create_from_builtin_config) reach them
//! as OCIO does.
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use vfx_ocio::{builtin, Config};
//!
//! let cfg    = builtin::default_config();                     // ocio://default: ACES 2.0 CG
//! let studio = Config::from_file("ocio://studio-config-latest")?;
//! let v1     = Config::create_from_builtin_config("cg-config-v1.0.0_aces-v1.3_ocio-v2.1")?;
//! let all    = builtin::aces_2_0_studio_all_views();          // not an OCIO built-in
//! # Ok(())
//! # }
//! ```

pub mod embedded;

#[cfg(test)]
pub mod test_schema;

#[cfg(test)]
pub use test_schema::TestSchema;

/// OCIO's `BuiltinTransformRegistry`: the builtin transform styles `BuiltinTransform { style }`
/// accepts. Styles match case-insensitively and otherwise exactly, as in OCIO — there are no
/// aliases.
///
/// This module used to offer invented names (`sRGB_to_Linear`, `Linear_to_Rec709`,
/// `ACES2065-1_to_ACEScg`, ...) that no OCIO build knows; the two that were real OCIO styles
/// remain, and [`styles`] lists every one.
pub mod transforms {
    /// ACEScct to ACES2065-1.
    pub const ACESCCT_TO_ACES2065_1: &str = "ACEScct_to_ACES2065-1";
    /// ACEScg to ACES2065-1.
    pub const ACESCG_TO_ACES2065_1: &str = "ACEScg_to_ACES2065-1";

    /// Every builtin style, in OCIO's registry order (`BuiltinTransformRegistry::getBuiltinStyle`).
    pub fn styles() -> impl Iterator<Item = &'static str> {
        crate::builtin_transforms::builtin_styles()
    }

    /// The description OCIO registers for `style`, or `None` if it is not a builtin style
    /// (`BuiltinTransformRegistry::getBuiltinDescription`).
    pub fn description(style: &str) -> Option<&'static str> {
        crate::builtin_transforms::builtin_description(style)
    }
}

use crate::Config;
use embedded::BuiltinConfigEntry;

// Embedded builtins — explicit re-exports for convenience.
pub use embedded::{aces_2_0_cg, aces_2_0_studio, aces_2_0_studio_all_views};

/// OCIO's default built-in config, `ocio://default`: the latest ACES CG config
/// (`cg-config-v4.0.0_aces-v2.0_ocio-v2.5`), as `BuiltinConfigRegistry.cpp` defines it.
pub fn default_config() -> Config {
    aces_2_0_cg()
}

// ============================================================================
// CG -> Studio routing (the SSOT for "resolve in CG first, fall back to Studio of the same
// release")
// ============================================================================

/// The CG release -> Studio release pairing this crate embeds, one entry per
/// `OpenColorIO-Config-ACES` release: `(cg_name, studio_name)`. Built from
/// [`embedded::ocio_builtins`] rather than hand-listed, so a release added to [`embedded::REGISTRY`]
/// is paired automatically and cannot silently go unpaired.
///
/// This is the data half of [`BuiltinConfigEntry::studio_twin`] — use that accessor for a single
/// lookup; this function is for enumerating every pairing at once (tests, diagnostics).
pub fn cg_to_studio_releases() -> Vec<(&'static str, &'static str)> {
    embedded::ocio_builtins()
        .iter()
        .filter(|e| e.is_cg())
        .filter_map(|cg| Some((cg.name, cg.studio_twin()?.name)))
        .collect()
}

/// Which config answered a [`pick_builtin`] request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinSource {
    /// The CG config knew everything the request needed, on its own.
    Cg,
    /// The CG config lacked something the request needed; the Studio config of the SAME
    /// release (its [`BuiltinConfigEntry::studio_twin`]) answered the whole request instead.
    Studio,
}

/// A [`pick_builtin`] result: the config that answers, and which one it was.
pub struct PickedBuiltin {
    /// The config chosen: `cg` itself, or its Studio twin.
    pub entry: &'static BuiltinConfigEntry,
    /// Which one `entry` is.
    pub source: BuiltinSource,
}

impl PickedBuiltin {
    /// The chosen config, parsed (see [`BuiltinConfigEntry::config`]; cached per process).
    pub fn config(&self) -> Config {
        self.entry.config()
    }
}

/// Whether `cfg` knows `name` on its own: a colour space (by name or alias), a named transform,
/// or a look. This is everything OCIO itself resolves a bare name to; it deliberately does NOT
/// include any host's informal-alias table (OIIO's `sRGB`, `linear`, `ACEScg`, ... belong to the
/// host — e.g. `vfx_io`'s colour-space name table — which ORs this with its own alias lookup
/// before calling [`pick_builtin`], via that function's `cg_extra`/`studio_extra` closures). An
/// empty name needs no answer (an unset colour space is not a missing one).
pub fn knows_name(cfg: &Config, name: &str) -> bool {
    name.is_empty()
        || cfg.colorspace(name).is_some()
        || cfg.named_transform(name).is_some()
        || cfg.looks().get(name).is_some()
}

/// Whether `cfg` knows `display`/`view` (`display` empty or `"default"` meaning the config's
/// default display; `view` empty or `"default"` meaning any view registered on it).
pub fn knows_display_view(cfg: &Config, display: &str, view: &str) -> bool {
    let display = if display.is_empty() || display == "default" {
        match cfg.default_display() {
            Some(d) => d,
            None => return false,
        }
    } else {
        display
    };
    if cfg.displays().display(display).is_none() {
        return false;
    }
    view.is_empty() || view == "default" || cfg.get_views(display).iter().any(|v| v.name() == view)
}

/// The SSOT for the CG-then-Studio routing rule: resolve a name (colour space, named transform,
/// look) or a display+view pair in the ACES CG built-in config first; if the CG config lacks
/// something the request needs, resolve the WHOLE request in the Studio config of the SAME ACES
/// release instead.
///
/// # Why this exists
///
/// OCIO's own default is the CG config — `ocio://default`, same as OIIO 3.1's built-in
/// `ColorConfig`. But the CG config's own description says it "does not include camera
/// colorspaces and the less common displays and looks" (e.g. `Camera Rec.709`,
/// `Rec.2100-HLG - Display`): a real request can name one of those. The Studio config of the
/// same release is always the right place to look them up in, because it is a strict superset —
/// every colour space, role, display/view, named transform and look the CG config has, the
/// Studio config has too, with the identical definition (`tests/ocio_config_yaml_parity.rs`
/// checks this for every release). So a name the CG config lacks means the WHOLE request that
/// name appears in should be answered by Studio, not just that one name in isolation — mixing
/// definitions from two configs for one conversion would be wrong the moment they ever diverge
/// on a name they share.
///
/// Every crate that resolves a built-in-config name against the CG default must call this
/// instead of re-deriving the rule: `vfx_io::ColorConfig::route`
/// (`crates/oiio/vfx-io/src/colorconfig.rs`) does, and `playa-engine`'s
/// `colour::display::with_display_processor` (`crates/playa-engine/src/colour/display.rs`)
/// should too — it currently hand-rolls the same CG-then-`studio-config-latest` fallback because
/// playa-engine cannot depend on vfx-io.
///
/// # Parameters
///
/// `cg` must be a CG config entry (`cg.is_cg()`); a non-CG entry has no Studio twin and this
/// always returns `cg` with [`BuiltinSource::Cg`]. `names` are colour space / named-transform /
/// look names the request needs (an empty name is always known — pass `&[]` for none).
/// `display_view` is an optional `(display, view)` pair the request also needs.
/// `cg_extra`/`studio_extra` let a caller fold in its own alias table (OIIO's informal aliases,
/// say) on top of each config's own [`knows_name`]; pass `|_| false` for neither.
///
/// If even the Studio twin cannot answer the whole request (or `cg` has none), `cg` is returned
/// anyway — the caller's own not-found error path takes over from there, exactly as if no
/// routing had happened.
pub fn pick_builtin(
    cg: &'static BuiltinConfigEntry,
    names: &[&str],
    display_view: Option<(&str, &str)>,
    cg_extra: impl Fn(&str) -> bool,
    studio_extra: impl Fn(&str) -> bool,
) -> PickedBuiltin {
    let cg_cfg = cg.config();
    let cg_knows = |n: &str| knows_name(&cg_cfg, n) || cg_extra(n);
    let dv_cg = display_view.is_none_or(|(d, v)| knows_display_view(&cg_cfg, d, v));
    if dv_cg && names.iter().all(|n| cg_knows(n)) {
        return PickedBuiltin {
            entry: cg,
            source: BuiltinSource::Cg,
        };
    }
    if let Some(studio) = cg.studio_twin() {
        let studio_cfg = studio.config();
        let studio_knows = |n: &str| knows_name(&studio_cfg, n) || studio_extra(n);
        let dv_studio = display_view.is_none_or(|(d, v)| knows_display_view(&studio_cfg, d, v));
        if dv_studio && names.iter().all(|n| cg_knows(n) || studio_knows(n)) {
            return PickedBuiltin {
                entry: studio,
                source: BuiltinSource::Studio,
            };
        }
    }
    PickedBuiltin {
        entry: cg,
        source: BuiltinSource::Cg,
    }
}

#[cfg(test)]
mod route_tests {
    use super::*;

    fn cg_entries() -> impl Iterator<Item = &'static BuiltinConfigEntry> {
        embedded::ocio_builtins().iter().filter(|e| e.is_cg())
    }

    /// The pairing covers all four releases and matches `studio_twin`.
    #[test]
    fn pairing_covers_all_four_releases() {
        let pairs = cg_to_studio_releases();
        assert_eq!(pairs.len(), 4, "{pairs:?}");
        for (cg_name, studio_name) in &pairs {
            let cg = embedded::find(cg_name).unwrap();
            assert_eq!(cg.studio_twin().unwrap().name, *studio_name);
        }
    }

    /// A name every CG release has (a role alias) resolves from the CG config itself.
    #[test]
    fn cg_name_stays_on_cg() {
        for cg in cg_entries() {
            let picked = pick_builtin(cg, &["ACEScg"], None, |_| false, |_| false);
            assert_eq!(picked.source, BuiltinSource::Cg, "{}", cg.name);
            assert!(std::ptr::eq(picked.entry, cg));
        }
    }

    /// A display only the Studio config has (`Rec.2100-HLG - Display`, excluded from the CG
    /// config's "less common displays") and a colour space only the Studio config has
    /// (`Camera Rec.709`, one of the CG config's excluded "camera colorspaces") both route to
    /// the Studio twin, for every release that has one.
    #[test]
    fn studio_only_display_and_colorspace_route_to_studio() {
        let mut checked = 0;
        for cg in cg_entries() {
            let studio = cg.studio_twin().unwrap();
            let studio_cfg = studio.config();

            if studio_cfg
                .displays()
                .display("Rec.2100-HLG - Display")
                .is_some()
                && cg
                    .config()
                    .displays()
                    .display("Rec.2100-HLG - Display")
                    .is_none()
            {
                let picked = pick_builtin(
                    cg,
                    &[],
                    Some(("Rec.2100-HLG - Display", "")),
                    |_| false,
                    |_| false,
                );
                assert_eq!(picked.source, BuiltinSource::Studio, "{}: display", cg.name);
                assert_eq!(picked.entry.name, studio.name);
                checked += 1;
            }

            if studio_cfg.colorspace("Camera Rec.709").is_some()
                && cg.config().colorspace("Camera Rec.709").is_none()
            {
                let picked = pick_builtin(cg, &["Camera Rec.709"], None, |_| false, |_| false);
                assert_eq!(
                    picked.source,
                    BuiltinSource::Studio,
                    "{}: colorspace",
                    cg.name
                );
                assert_eq!(picked.entry.name, studio.name);
                checked += 1;
            }
        }
        assert!(
            checked > 0,
            "expected at least one release to exercise the Studio fallback"
        );
    }

    /// A name neither config knows falls back to `cg` (not an error here — the caller's own
    /// not-found path takes over).
    #[test]
    fn unknown_name_falls_back_to_cg() {
        for cg in cg_entries() {
            let picked = pick_builtin(
                cg,
                &["no such colour space at all"],
                None,
                |_| false,
                |_| false,
            );
            assert_eq!(picked.source, BuiltinSource::Cg, "{}", cg.name);
            assert!(std::ptr::eq(picked.entry, cg));
        }
    }

    /// A non-CG entry (Studio itself) has no twin: always routes to itself.
    #[test]
    fn non_cg_entry_has_no_twin() {
        let studio = embedded::find(embedded::LATEST_STUDIO_BUILTIN_CONFIG).unwrap();
        assert!(!studio.is_cg());
        let picked = pick_builtin(studio, &["Camera Rec.709"], None, |_| false, |_| false);
        assert_eq!(picked.source, BuiltinSource::Cg);
        assert!(std::ptr::eq(picked.entry, studio));
    }
}
