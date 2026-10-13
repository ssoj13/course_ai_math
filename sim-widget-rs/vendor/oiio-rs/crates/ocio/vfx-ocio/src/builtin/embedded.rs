//! OCIO's built-in configs (`BuiltinConfigRegistry`), embedded.
//!
//! Every config OCIO 2.5 registers is compiled in with `include_str!`: the ACES **CG** config and
//! the ACES **Studio** config of each `OpenColorIO-Config-ACES` release (ACES 1.3 v1.0.0, v2.1.0,
//! v2.2.0; ACES 2.0 v4.0.0). The files under `src/builtin/data/` are OCIO's own text, as
//! PyOpenColorIO 2.5.1 hands it out (`BuiltinConfigRegistry()[name]`, line endings LF), written
//! by `tests/data/gen_ocio_configs.py`; `tests/ocio_config_yaml_parity.rs` checks the registry
//! (names, order, UI names, recommended/default flags, text hashes) against PyOpenColorIO, that
//! every CG config is a subset of the Studio config of the same release, and every conversion
//! each config makes.
//!
//! Names and URIs behave as in OCIO (`BuiltinConfigRegistry.cpp`, `Config::CreateFromFile`,
//! `Config::CreateFromBuiltinConfig`): names match case-insensitively; `ocio://default` and
//! `ocio://cg-config-latest` are the latest CG config, `ocio://studio-config-latest` the latest
//! Studio config ([`resolve_config_path`]).
//!
//! One more config is embedded that OCIO does not ship: the Studio **all-views** config of the
//! v4.0.0 release (every display with every view). It is found by its name through [`get`], but
//! not through `ocio://` (OCIO would not find it either) and it is not in [`ocio_builtins`].
//!
//! Parsed configs are cached per process, so repeated [`get`] calls clone the parse instead of
//! parsing the YAML again.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

use crate::Config;

/// One embedded config.
#[derive(Debug, Clone, Copy)]
pub struct BuiltinConfigEntry {
    /// The config's name (OCIO's registry name, e.g. `cg-config-v4.0.0_aces-v2.0_ocio-v2.5`).
    pub name: &'static str,
    /// OCIO's UI name (`getBuiltinConfigUIName`).
    pub ui_name: &'static str,
    /// OCIO's "recommended" flag (`isBuiltinConfigRecommended`).
    pub recommended: bool,
    /// Whether OCIO's registry has it (false only for the all-views config).
    pub ocio_builtin: bool,
    /// The whole `.ocio` text.
    pub yaml: &'static str,
}

impl BuiltinConfigEntry {
    /// `ocio://<name>`.
    pub fn uri(&self) -> String {
        format!("{OCIO_URI_PREFIX}{}", self.name)
    }

    /// Whether this is one of the ACES CG configs.
    pub fn is_cg(&self) -> bool {
        self.name.starts_with("cg-config-")
    }

    /// For a CG config, the Studio config of the same release (a superset: every colour space,
    /// role, display/view and named transform of the CG config is in it with the same
    /// definition; `tests/ocio_config_yaml_parity.rs` checks this). `None` otherwise.
    pub fn studio_twin(&self) -> Option<&'static BuiltinConfigEntry> {
        let release = self.name.strip_prefix("cg-config-")?;
        ocio_builtins()
            .iter()
            .find(|e| e.name.strip_prefix("studio-config-") == Some(release))
    }

    /// The parsed config (cached).
    pub fn config(&'static self) -> Config {
        parsed(self)
    }
}

macro_rules! entry {
    ($name:literal, $ui:literal, $recommended:literal) => {
        BuiltinConfigEntry {
            name: $name,
            ui_name: $ui,
            recommended: $recommended,
            ocio_builtin: true,
            yaml: include_str!(concat!("data/", $name, ".ocio")),
        }
    };
}

/// Every embedded config: OCIO's registry in OCIO's order (`CGCONFIG::Register` then
/// `STUDIOCONFIG::Register`), then the all-views config.
pub const REGISTRY: &[BuiltinConfigEntry] = &[
    entry!(
        "cg-config-v1.0.0_aces-v1.3_ocio-v2.1",
        "Academy Color Encoding System - CG Config [COLORSPACES v1.0.0] [ACES v1.3] [OCIO v2.1]",
        false
    ),
    entry!(
        "cg-config-v2.1.0_aces-v1.3_ocio-v2.3",
        "Academy Color Encoding System - CG Config [COLORSPACES v2.0.0] [ACES v1.3] [OCIO v2.3]",
        false
    ),
    entry!(
        "cg-config-v2.2.0_aces-v1.3_ocio-v2.4",
        "Academy Color Encoding System - CG Config [COLORSPACES v2.2.0] [ACES v1.3] [OCIO v2.4]",
        false
    ),
    entry!(
        "cg-config-v4.0.0_aces-v2.0_ocio-v2.5",
        "Academy Color Encoding System - CG Config [COLORSPACES v4.0.0] [ACES v2.0] [OCIO v2.5]",
        true
    ),
    entry!(
        "studio-config-v1.0.0_aces-v1.3_ocio-v2.1",
        "Academy Color Encoding System - Studio Config [COLORSPACES v1.0.0] [ACES v1.3] [OCIO v2.1]",
        false
    ),
    entry!(
        "studio-config-v2.1.0_aces-v1.3_ocio-v2.3",
        "Academy Color Encoding System - Studio Config [COLORSPACES v2.0.0] [ACES v1.3] [OCIO v2.3]",
        false
    ),
    entry!(
        "studio-config-v2.2.0_aces-v1.3_ocio-v2.4",
        "Academy Color Encoding System - Studio Config [COLORSPACES v2.2.0] [ACES v1.3] [OCIO v2.4]",
        false
    ),
    entry!(
        "studio-config-v4.0.0_aces-v2.0_ocio-v2.5",
        "Academy Color Encoding System - Studio Config [COLORSPACES v4.0.0] [ACES v2.0] [OCIO v2.5]",
        true
    ),
    BuiltinConfigEntry {
        name: STUDIO_ALL_VIEWS,
        ui_name: "ACES 2.0 Studio All-Views (v4.0.0)",
        recommended: false,
        ocio_builtin: false,
        yaml: include_str!("data/studio-config-all-views-v4.0.0_aces-v2.0_ocio-v2.5.ocio"),
    },
];

/// The URI prefix of a built-in config (`OCIO_BUILTIN_URI_PREFIX`).
pub const OCIO_URI_PREFIX: &str = "ocio://";
/// What `ocio://default` stands for (`DEFAULT_BUILTIN_CONFIG_URI`): the latest CG config.
pub const DEFAULT_BUILTIN_CONFIG: &str = "cg-config-v4.0.0_aces-v2.0_ocio-v2.5";
/// What `ocio://cg-config-latest` stands for (`LATEST_CG_BUILTIN_CONFIG_URI`).
pub const LATEST_CG_BUILTIN_CONFIG: &str = "cg-config-v4.0.0_aces-v2.0_ocio-v2.5";
/// What `ocio://studio-config-latest` stands for (`LATEST_STUDIO_BUILTIN_CONFIG_URI`).
pub const LATEST_STUDIO_BUILTIN_CONFIG: &str = "studio-config-v4.0.0_aces-v2.0_ocio-v2.5";
/// The Studio all-views config (not an OCIO built-in).
pub const STUDIO_ALL_VIEWS: &str = "studio-config-all-views-v4.0.0_aces-v2.0_ocio-v2.5";

/// OCIO's registry (`BuiltinConfigRegistry`): every entry but the all-views config, in OCIO's
/// order.
pub fn ocio_builtins() -> &'static [BuiltinConfigEntry] {
    &REGISTRY[..REGISTRY.len() - 1]
}

/// Every embedded config's name.
pub fn names() -> Vec<&'static str> {
    REGISTRY.iter().map(|e| e.name).collect()
}

/// Every embedded config.
pub fn entries() -> &'static [BuiltinConfigEntry] {
    REGISTRY
}

/// The name after `ocio://` in `path`, when `path` holds such a URI (OCIO's
/// `ocio:\/\/([^\s]+)` search, `Config::CreateFromFile`).
pub fn uri_name(path: &str) -> Option<&str> {
    let at = path.find(OCIO_URI_PREFIX)?;
    let rest = &path[at + OCIO_URI_PREFIX.len()..];
    let name = rest.split(char::is_whitespace).next().unwrap_or("");
    (!name.is_empty()).then_some(name)
}

/// OCIO's `ResolveConfigPath`: `ocio://default` and `ocio://cg-config-latest` become the latest CG
/// config's URI, `ocio://studio-config-latest` the latest Studio config's (names compared
/// case-insensitively); anything else is returned unchanged.
pub fn resolve_config_path(path: &str) -> String {
    if let Some(name) = uri_name(path) {
        for (alias, target) in [
            ("default", DEFAULT_BUILTIN_CONFIG),
            ("cg-config-latest", LATEST_CG_BUILTIN_CONFIG),
            ("studio-config-latest", LATEST_STUDIO_BUILTIN_CONFIG),
        ] {
            if name.eq_ignore_ascii_case(alias) {
                return format!("{OCIO_URI_PREFIX}{target}");
            }
        }
    }
    path.to_string()
}

/// The OCIO built-in a name or URI stands for, as `Config::CreateFromBuiltinConfig` finds it: a
/// registry name (any case), `default`, `cg-config-latest` or `studio-config-latest`, with or
/// without `ocio://`. The all-views config is not an OCIO built-in and is not found here.
pub fn find_ocio_builtin(name_or_uri: &str) -> Option<&'static BuiltinConfigEntry> {
    let uri = if uri_name(name_or_uri).is_some() {
        name_or_uri.to_string()
    } else {
        format!("{OCIO_URI_PREFIX}{name_or_uri}")
    };
    let resolved = resolve_config_path(&uri);
    let name = uri_name(&resolved)?;
    ocio_builtins()
        .iter()
        .find(|e| e.name.eq_ignore_ascii_case(name))
}

/// Any embedded config by name or URI: an OCIO built-in as [`find_ocio_builtin`] finds it, or
/// the all-views config by its name.
pub fn find(name: &str) -> Option<&'static BuiltinConfigEntry> {
    find_ocio_builtin(name).or_else(|| REGISTRY.iter().find(|e| e.name.eq_ignore_ascii_case(name)))
}

/// Parse cache, keyed by the entry name.
fn cache() -> &'static RwLock<HashMap<&'static str, Arc<Config>>> {
    static CACHE: OnceLock<RwLock<HashMap<&'static str, Arc<Config>>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

fn parsed(entry: &'static BuiltinConfigEntry) -> Config {
    if let Some(cached) = cache().read().ok().and_then(|c| c.get(entry.name).cloned()) {
        return (*cached).clone();
    }
    // A bad embedded file is a build defect, caught by `every_registry_entry_parses`.
    let cfg = Config::from_yaml_str(entry.yaml, std::path::PathBuf::new())
        .unwrap_or_else(|e| panic!("embedded config {:?} failed to parse: {e:?}", entry.name));
    if let Ok(mut write) = cache().write() {
        write
            .entry(entry.name)
            .or_insert_with(|| Arc::new(cfg.clone()));
    }
    cfg
}

/// The embedded config `name` or URI stands for ([`find`]), parsed; `None` if there is none.
pub fn get(name: &str) -> Option<Config> {
    find(name).map(|e| e.config())
}

/// OCIO's default built-in config (`ocio://default`): the ACES 2.0 CG config.
pub fn aces_2_0_cg() -> Config {
    get(DEFAULT_BUILTIN_CONFIG).expect("the default built-in config is embedded")
}

/// The ACES 2.0 Studio config (`ocio://studio-config-latest`).
pub fn aces_2_0_studio() -> Config {
    get(LATEST_STUDIO_BUILTIN_CONFIG).expect("the latest Studio config is embedded")
}

/// The ACES 2.0 Studio all-views config (every display with every view; not an OCIO built-in).
pub fn aces_2_0_studio_all_views() -> Config {
    get(STUDIO_ALL_VIEWS).expect("the all-views config is embedded")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transform::Transform;

    /// Every embedded config must parse and have colour spaces.
    #[test]
    fn every_registry_entry_parses() {
        for entry in REGISTRY {
            let cfg = Config::from_yaml_str(entry.yaml, std::path::PathBuf::new())
                .unwrap_or_else(|e| panic!("embedded config {:?} failed: {e:?}", entry.name));
            assert!(
                cfg.num_colorspaces() > 0,
                "{}: expected colorspaces, got none",
                entry.name
            );
        }
    }

    /// Names and URIs are found the way OCIO finds them.
    #[test]
    fn names_and_uris_resolve_like_ocio() {
        for entry in ocio_builtins() {
            assert_eq!(find(entry.name).unwrap().name, entry.name);
            assert_eq!(find(&entry.uri()).unwrap().name, entry.name);
            assert_eq!(find(&entry.name.to_uppercase()).unwrap().name, entry.name);
        }
        for (name, want) in [
            ("default", DEFAULT_BUILTIN_CONFIG),
            ("ocio://default", DEFAULT_BUILTIN_CONFIG),
            ("ocio://Default", DEFAULT_BUILTIN_CONFIG),
            ("cg-config-latest", LATEST_CG_BUILTIN_CONFIG),
            ("ocio://studio-config-latest", LATEST_STUDIO_BUILTIN_CONFIG),
        ] {
            assert_eq!(find(name).unwrap().name, want, "{name}");
        }
        assert!(find("ocio://no-such-config").is_none());
        assert!(
            find_ocio_builtin(STUDIO_ALL_VIEWS).is_none(),
            "not an OCIO built-in"
        );
        assert_eq!(find(STUDIO_ALL_VIEWS).unwrap().name, STUDIO_ALL_VIEWS);
        assert_eq!(
            find(DEFAULT_BUILTIN_CONFIG)
                .unwrap()
                .studio_twin()
                .unwrap()
                .name,
            LATEST_STUDIO_BUILTIN_CONFIG
        );
        assert!(
            find(LATEST_STUDIO_BUILTIN_CONFIG)
                .unwrap()
                .studio_twin()
                .is_none()
        );
        assert_eq!(ocio_builtins().iter().filter(|e| e.is_cg()).count(), 4);
        assert!(
            ocio_builtins()
                .iter()
                .filter(|e| e.is_cg())
                .all(|e| e.studio_twin().is_some())
        );
    }

    /// Every `BuiltinTransform { style: ... }` referenced anywhere in the embedded configs
    /// (colour spaces, view transforms, looks, named transforms) must resolve through
    /// `get_builtin`; a missing style would produce zero/NaN output at render time.
    #[test]
    fn every_builtin_style_resolves() {
        use crate::builtin_transforms::get_builtin;

        fn collect_styles(t: &Transform, out: &mut Vec<String>) {
            match t {
                Transform::Builtin(bt) => out.push(bt.style.clone()),
                Transform::Group(g) => {
                    for c in &g.transforms {
                        collect_styles(c, out);
                    }
                }
                _ => {}
            }
        }

        for entry in REGISTRY {
            let cfg = entry.config();
            let mut styles = Vec::new();
            for cs in cfg.colorspaces() {
                // All four accessors: display-referred colour spaces (e.g. sRGB - Display)
                // keep their builtin transforms in the `*_display_reference` slots.
                for t in [
                    cs.to_reference(),
                    cs.from_reference(),
                    cs.to_display_reference(),
                    cs.from_display_reference(),
                ]
                .into_iter()
                .flatten()
                {
                    collect_styles(t, &mut styles);
                }
            }
            for vt in cfg.displays().view_transforms() {
                for t in [
                    vt.from_scene_reference(),
                    vt.to_scene_reference(),
                    vt.from_display_reference(),
                    vt.to_display_reference(),
                ]
                .into_iter()
                .flatten()
                {
                    collect_styles(t, &mut styles);
                }
            }
            for look in cfg.looks().all() {
                for t in [look.get_transform(), look.get_inverse_transform()]
                    .into_iter()
                    .flatten()
                {
                    collect_styles(t, &mut styles);
                }
            }
            for nt in cfg.named_transforms() {
                for t in [nt.forward(), nt.inverse()].into_iter().flatten() {
                    collect_styles(t, &mut styles);
                }
            }
            styles.sort();
            styles.dedup();
            for s in &styles {
                assert!(
                    get_builtin(s).is_some(),
                    "{}: builtin style {:?} unresolved",
                    entry.name,
                    s
                );
            }
        }
    }

    /// Every (display, view) pair of every embedded config gives finite output for
    /// representative scene-linear inputs.
    #[test]
    fn no_nan_on_typical_inputs() {
        use crate::transform::DisplayViewTransform;

        let inputs = [
            [0.18_f32, 0.18, 0.18],
            [0.5, 0.5, 0.5],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
        ];

        for entry in REGISTRY {
            let cfg = entry.config();
            let Some(src) = cfg.role_colorspace("scene_linear") else {
                continue;
            };
            let src = src.to_string();

            for display in cfg.displays().displays() {
                for view in cfg.get_views(display.name()) {
                    let dvt = DisplayViewTransform {
                        src: src.clone(),
                        display: display.name().to_string(),
                        view: view.name().to_string(),
                        ..DisplayViewTransform::default()
                    };
                    // Views that do not apply to a display are skipped (hosts filter them).
                    let Ok(proc) = cfg.processor_for_display_view_transform(&dvt) else {
                        continue;
                    };
                    for inp in &inputs {
                        let mut px = [*inp];
                        proc.apply_rgb(&mut px);
                        assert!(
                            px[0].iter().all(|v| v.is_finite()),
                            "{}: {} / {} on input {:?} produced {:?}",
                            entry.name,
                            display.name(),
                            view.name(),
                            inp,
                            px[0]
                        );
                    }
                }
            }
        }
    }
}
