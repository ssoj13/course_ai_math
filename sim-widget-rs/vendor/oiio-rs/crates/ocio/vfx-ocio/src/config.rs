//! OCIO configuration file parsing and management.
//!
//! This module handles loading and parsing `.ocio` configuration files
//! in YAML format. Supports OCIO v1 and v2 config formats.
//!
//! # Example
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use vfx_ocio::Config;
//!
//! // Load from file
//! let config = Config::from_file("aces_1.2/config.ocio")?;
//!
//! // Get color spaces
//! for cs in config.colorspaces() {
//!     println!("{}: {:?}", cs.name(), cs.encoding());
//! }
//!
//! // Create processor
//! let proc = config.processor("ACEScg", "sRGB")?;
//! # Ok(())
//! # }
//! ```

use regex::Regex;
use saphyr::{LoadableYamlNode, Yaml};
use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use vfx_core::pixel::{REC709_LUMA_B, REC709_LUMA_G, REC709_LUMA_R};

use crate::archive::{
    archive_config_to_path, config_archive_name, create_temp_archive_dir, extract_ocioz_archive,
};
use crate::colorspace::{ColorSpace, ColorSpaceDirection, Encoding};
use crate::context::Context;
use crate::display::{Display, DisplayManager, ReferenceSpaceType, View, ViewTransform};
use crate::error::{OcioError, OcioResult};
use crate::io_proxy::ConfigIOProxy;
use crate::look::{Look, LookManager};
use crate::processor::{OptimizationLevel, Processor};
use crate::role::Roles;
use crate::transform::*;
use vfx_icc::Profile as IccProfile;

/// The view half of [`Config::add_display_view`]: OCIO `addDisplayView`'s `view`,
/// `viewTransform`, `colorSpace`, `looks`, `rule` and `description`. `None` and empty strings
/// both mean "not set".
#[derive(Debug, Clone, Default)]
pub struct AddDisplayViewParams<'a> {
    /// View name (must not be empty).
    pub view: &'a str,
    /// Optional view transform.
    pub view_transform: Option<String>,
    /// Color space name (must not be empty).
    pub colorspace: &'a str,
    /// Optional looks.
    pub looks: Option<String>,
    /// Optional viewing rule.
    pub rule: Option<String>,
    /// Optional description.
    pub description: Option<String>,
}

/// OCIO configuration.
///
/// The main entry point for color management. A config defines:
/// - Color spaces and their transforms
/// - Roles (semantic mappings)
/// - Displays and views
/// - Looks (creative grades)
#[derive(Debug, Clone)]
pub struct Config {
    /// Config name (`name:`, OCIO `Config::getName`).
    name: String,
    /// Config description (`description:`, OCIO `Config::getDescription`); its own field, B23.
    description: String,
    /// Config version (1 or 2).
    version: ConfigVersion,
    /// Config minor version (e.g. 0 for 2.0, 1 for 2.1).
    version_minor: u32,
    /// Search paths for LUTs.
    search_paths: Vec<PathBuf>,
    /// Working directory (config file location).
    working_dir: PathBuf,
    /// Working directory was set explicitly by the user (not defaulted by loader).
    working_dir_is_default: bool,
    /// Config source (affects archiving rules).
    source: ConfigSource,
    /// Temporary directory for extracted archives (kept for lifetime).
    archive_dir: Option<PathBuf>,
    /// All color spaces.
    colorspaces: Vec<ColorSpace>,
    /// Role mappings.
    roles: Roles,
    /// Display/view configuration.
    displays: DisplayManager,
    /// Looks.
    looks: LookManager,
    /// Active displays (subset to show in UI).
    active_displays: Vec<String>,
    /// Active views (subset to show in UI).
    active_views: Vec<String>,
    /// Shared views (OCIO v2.3+). Displays reference these by name
    /// via [`Display::shared_view_refs`].
    shared_views: Vec<View>,
    /// Virtual display (OCIO v2.0+) - template for ICC profile displays.
    /// There may only be one virtual display in a config.
    virtual_display: Option<VirtualDisplay>,
    /// Viewing rules (OCIO v2.0+) - filter views by colorspace encoding.
    viewing_rules: Vec<ViewingRule>,
    /// Named transforms (OCIO v2.0+).
    named_transforms: Vec<NamedTransform>,
    /// Inactive named transforms (hidden from UI).
    #[allow(dead_code)]
    inactive_named_transforms: Vec<String>,
    /// Inactive color spaces (hidden from UI).
    #[allow(dead_code)]
    inactive_colorspaces: Vec<String>,
    /// File rules for automatic color space detection.
    file_rules: Vec<FileRule>,
    /// Environment variables declared in the config.
    environment_vars: Vec<(String, String)>,
    /// Environment/context.
    context: Context,
    /// Strict parsing mode.
    #[allow(dead_code)]
    strict_parsing: bool,
    /// Family separator character.
    family_separator: String,
    /// Environment mode (predefined vs load all).
    environment_mode: EnvironmentMode,
    /// Default luma coefficients (RGB).
    default_luma_coefs: [f32; 3],
    /// Default view transform name (OCIO v2).
    default_view_transform: Option<String>,
    /// Optional cache ID override.
    cache_id: Option<String>,
    /// Optional config IO proxy.
    io_proxy: Option<Arc<dyn ConfigIOProxy>>,
    /// Processor cache flags (OCIO parity).
    processor_cache_flags: u32,
}

pub mod types;
pub use types::*;

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

impl Config {
    /// Creates a new empty configuration.
    pub fn new() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            version: ConfigVersion::V1,
            version_minor: 0,
            search_paths: Vec::new(),
            working_dir: PathBuf::from("."),
            working_dir_is_default: true,
            source: ConfigSource::Raw,
            archive_dir: None,
            colorspaces: Vec::new(),
            roles: Roles::new(),
            displays: DisplayManager::new(),
            looks: LookManager::new(),
            active_displays: Vec::new(),
            active_views: Vec::new(),
            shared_views: Vec::new(),
            virtual_display: None,
            viewing_rules: Vec::new(),
            named_transforms: Vec::new(),
            inactive_named_transforms: Vec::new(),
            inactive_colorspaces: Vec::new(),
            file_rules: Vec::new(),
            environment_vars: Vec::new(),
            context: Context::new(),
            strict_parsing: false,
            family_separator: "/".to_string(),
            environment_mode: EnvironmentMode::LoadPredefined,
            default_luma_coefs: [REC709_LUMA_R, REC709_LUMA_G, REC709_LUMA_B],
            default_view_transform: None,
            cache_id: None,
            io_proxy: None,
            processor_cache_flags: 0,
        }
    }

    /// Loads configuration from a file, an `.ocioz` archive, or a built-in config URI
    /// (`ocio://cg-config-v4.0.0_aces-v2.0_ocio-v2.5`, `ocio://default`, ...), as OCIO's
    /// `Config::CreateFromFile` does.
    pub fn from_file(path: impl AsRef<Path>) -> OcioResult<Self> {
        let path = path.as_ref();
        if let Some(uri) = path.to_str()
            && crate::builtin::embedded::uri_name(uri).is_some()
        {
            return Self::create_from_builtin_config(uri);
        }
        if !path.exists() {
            return Err(OcioError::ConfigNotFound {
                path: path.to_path_buf(),
            });
        }

        if path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("ocioz"))
            .unwrap_or(false)
        {
            let temp_dir = create_temp_archive_dir()?;
            extract_ocioz_archive(path, &temp_dir)?;
            let config_path = temp_dir.join(config_archive_name());
            if !config_path.exists() {
                return Err(OcioError::ConfigNotFound { path: config_path });
            }
            let content = std::fs::read_to_string(&config_path)?;
            let mut cfg =
                Self::from_yaml_str_with_source(&content, temp_dir.clone(), ConfigSource::Archive)?;
            cfg.archive_dir = Some(temp_dir);
            return Ok(cfg);
        }

        let content = std::fs::read_to_string(path)?;
        let working_dir = path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));

        Self::from_yaml_str_with_source(&content, working_dir, ConfigSource::File)
    }

    /// Loads configuration from a file or config IO proxy.
    pub fn from_file_with_io_proxy(
        path: impl AsRef<Path>,
        proxy: Arc<dyn ConfigIOProxy>,
    ) -> OcioResult<Self> {
        let path = path.as_ref();
        if path.exists() {
            let mut cfg = Self::from_file(path)?;
            cfg.set_config_io_proxy(Some(proxy));
            return Ok(cfg);
        }

        let data = proxy
            .get_config_data(path.to_string_lossy().as_ref())
            .ok_or_else(|| OcioError::ConfigNotFound {
                path: path.to_path_buf(),
            })?;
        let content = String::from_utf8(data)
            .map_err(|e| OcioError::Yaml(format!("config data not UTF-8: {}", e)))?;
        let working_dir = path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let mut cfg =
            Self::from_yaml_str_with_source(&content, working_dir, ConfigSource::IOProxy)?;
        cfg.set_config_io_proxy(Some(proxy));
        Ok(cfg)
    }

    /// OCIO's `Config::CreateFromBuiltinConfig`: a built-in config by name or URI (any case,
    /// with or without `ocio://`; `default`, `cg-config-latest` and `studio-config-latest`
    /// resolve as in [`Self::resolve_config_path`]). See [`crate::builtin::embedded`].
    pub fn create_from_builtin_config(name: &str) -> OcioResult<Self> {
        crate::builtin::embedded::find_ocio_builtin(name)
            .map(|e| e.config())
            .ok_or_else(|| {
                let uri = crate::builtin::embedded::resolve_config_path(name);
                let name = crate::builtin::embedded::uri_name(&uri).unwrap_or(name);
                OcioError::BuiltinConfigNotFound {
                    name: name.to_string(),
                }
            })
    }

    /// Loads configuration from YAML string.
    pub fn from_yaml_str(yaml_str: &str, working_dir: PathBuf) -> OcioResult<Self> {
        Self::from_yaml_str_with_source(yaml_str, working_dir, ConfigSource::File)
    }

    /// Loads configuration from a stream (YAML string) with a default working dir.
    pub fn from_stream(yaml_str: &str) -> OcioResult<Self> {
        Self::from_yaml_str_with_source(yaml_str, PathBuf::from("."), ConfigSource::Stream)
    }

    /// Loads configuration from YAML string with explicit source.
    pub(crate) fn from_yaml_str_with_source(
        yaml_str: &str,
        working_dir: PathBuf,
        source: ConfigSource,
    ) -> OcioResult<Self> {
        let docs = Yaml::load_from_str(yaml_str).map_err(|e| OcioError::Yaml(format!("{}", e)))?;

        if docs.is_empty() {
            return Err(OcioError::Yaml("empty YAML document".into()));
        }

        let root = &docs[0];
        Self::from_yaml(root, working_dir, source)
    }

    /// Returns config name.
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns config version.
    #[inline]
    pub fn version(&self) -> ConfigVersion {
        self.version
    }

    /// Creates an editable copy (OCIO parity).
    #[inline]
    pub fn create_editable_copy(&self) -> Self {
        self.clone()
    }

    /// Returns the default luma coefficients (RGB).
    #[inline]
    pub fn default_luma_coefs(&self) -> [f32; 3] {
        self.default_luma_coefs
    }

    /// Sets the default luma coefficients (RGB).
    #[inline]
    pub fn set_default_luma_coefs(&mut self, coeffs: [f32; 3]) {
        self.default_luma_coefs = coeffs;
    }

    /// Sets an explicit cache ID.
    #[inline]
    pub fn set_cache_id(&mut self, cache_id: Option<String>) {
        self.cache_id = cache_id;
    }

    /// Returns the cache ID (computed if not explicitly set).
    pub fn cache_id(&self) -> String {
        if let Some(ref id) = self.cache_id {
            return id.clone();
        }
        self.cache_id_with_context(Some(&self.context))
    }

    /// Returns the cache ID for a given context (OCIO parity).
    pub fn cache_id_with_context(&self, context: Option<&Context>) -> String {
        if let Some(ref id) = self.cache_id {
            return id.clone();
        }

        use xxhash_rust::xxh3::xxh3_128;

        let serialized = self.serialize().unwrap_or_default();
        let hash = xxh3_128(serialized.as_bytes());
        let low = hash as u64;
        let high = (hash >> 64) as u64;
        let cache_id_no_context = format!("{:x}{:x}", low, high);

        let mut files = std::collections::BTreeSet::<String>::new();
        self.collect_file_references(&mut files);

        let mut file_hash = String::new();
        if let Some(ctx) = context {
            for file in files {
                if file.is_empty() {
                    continue;
                }
                file_hash.push_str(&file);
                file_hash.push('=');
                match ctx.resolve_file_location(&file) {
                    Some(resolved) => {
                        let resolved_str = resolved.to_string_lossy().to_string();
                        let fast = crate::path_utils::get_fast_file_hash(&resolved_str, ctx);
                        file_hash.push_str(&fast);
                        file_hash.push(' ');
                    }
                    None => {
                        file_hash.push_str("? ");
                    }
                }
            }
        }

        let file_hash_id = if file_hash.is_empty() {
            String::new()
        } else {
            let hash = xxh3_128(file_hash.as_bytes());
            let low = hash as u64;
            let high = (hash >> 64) as u64;
            format!("{:x}{:x}", low, high)
        };

        format!("{}:{}", cache_id_no_context, file_hash_id)
    }

    /// Returns the working directory.
    #[inline]
    pub fn working_dir(&self) -> &Path {
        &self.working_dir
    }

    /// Returns all search paths.
    #[inline]
    pub fn search_paths(&self) -> &[PathBuf] {
        &self.search_paths
    }

    /// Returns all color spaces.
    #[inline]
    pub fn colorspaces(&self) -> &[ColorSpace] {
        &self.colorspaces
    }

    /// Returns number of color spaces.
    #[inline]
    pub fn num_colorspaces(&self) -> usize {
        self.colorspaces.len()
    }

    /// Returns filtered color spaces by reference space and visibility.
    pub fn colorspaces_filtered(
        &self,
        reference_space: SearchReferenceSpaceType,
        visibility: ColorSpaceVisibility,
    ) -> Vec<&ColorSpace> {
        self.colorspaces
            .iter()
            .filter(|cs| match reference_space {
                SearchReferenceSpaceType::All => true,
                SearchReferenceSpaceType::Scene => {
                    cs.reference_space_type() == ReferenceSpaceType::Scene
                }
                SearchReferenceSpaceType::Display => {
                    cs.reference_space_type() == ReferenceSpaceType::Display
                }
            })
            .filter(|cs| match visibility {
                ColorSpaceVisibility::All => true,
                ColorSpaceVisibility::Active => !self.is_colorspace_inactive(cs.name()),
                ColorSpaceVisibility::Inactive => self.is_colorspace_inactive(cs.name()),
            })
            .collect()
    }

    /// Returns the color space name by index with filters.
    pub fn colorspace_name_by_index(
        &self,
        reference_space: SearchReferenceSpaceType,
        visibility: ColorSpaceVisibility,
        index: usize,
    ) -> Option<&str> {
        self.colorspaces_filtered(reference_space, visibility)
            .get(index)
            .map(|cs| cs.name())
    }

    /// Returns index for color space name with filters.
    pub fn index_for_colorspace(
        &self,
        name: &str,
        reference_space: SearchReferenceSpaceType,
        visibility: ColorSpaceVisibility,
    ) -> Option<usize> {
        self.colorspaces_filtered(reference_space, visibility)
            .iter()
            .position(|cs| cs.matches_name(name))
    }

    /// Gets a color space by name (or alias).
    ///
    /// Same order as OCIO's `Config::Impl::getColorSpace`: a colour space name or alias
    /// (case-insensitive) first, then a role. Roles used to be tried first, so a role that
    /// shadowed a colour space name answered with the role's target instead of that space.
    pub fn colorspace(&self, name: &str) -> Option<&ColorSpace> {
        if let Some(cs) = self.colorspaces.iter().find(|cs| cs.matches_name(name)) {
            return Some(cs);
        }
        let cs_name = self.roles.get(name)?;
        self.colorspaces.iter().find(|cs| cs.matches_name(cs_name))
    }

    /// Resolves an alias / role / colorspace / named-transform name to
    /// its canonical name. Mirrors OCIO C++ `Config::getCanonicalName`.
    /// Returns an empty string when the name doesn't resolve to any
    /// known entity.
    pub fn canonical_name(&self, name: &str) -> &str {
        if let Some(cs) = self.colorspace(name) {
            return cs.name();
        }
        if let Some(nt) = self
            .named_transforms
            .iter()
            .find(|nt| nt.matches_name(name))
        {
            return nt.name.as_str();
        }
        ""
    }

    /// Returns color space names.
    pub fn colorspace_names(&self) -> impl Iterator<Item = &str> {
        self.colorspaces.iter().map(|cs| cs.name())
    }

    /// Returns colorspaces filtered by category.
    pub fn colorspaces_by_category(&self, category: &str) -> Vec<&ColorSpace> {
        self.colorspaces
            .iter()
            .filter(|cs| cs.has_category(category))
            .collect()
    }

    /// Returns all unique categories across colorspaces.
    pub fn all_categories(&self) -> Vec<&str> {
        let mut cats: Vec<&str> = self
            .colorspaces
            .iter()
            .flat_map(|cs| cs.categories().iter().map(|s| s.as_str()))
            .collect();
        cats.sort();
        cats.dedup();
        cats
    }

    /// Returns the roles mapping.
    #[inline]
    pub fn roles(&self) -> &Roles {
        &self.roles
    }

    /// Returns the display manager.
    #[inline]
    pub fn displays(&self) -> &DisplayManager {
        &self.displays
    }

    /// Returns the look manager.
    #[inline]
    pub fn looks(&self) -> &LookManager {
        &self.looks
    }

    /// Returns active display names.
    #[inline]
    pub fn active_displays(&self) -> &[String] {
        &self.active_displays
    }

    /// Effective active-displays list, honouring the
    /// `OCIO_ACTIVE_DISPLAYS` env override (which takes precedence
    /// over the config-file value per OCIO C++ docs at
    /// `OpenColorIO.h:1083-1086`). Returns an empty `Vec` when the
    /// env var is set but resolves to an empty list.
    pub fn effective_active_displays(&self) -> Vec<String> {
        env_active_list("OCIO_ACTIVE_DISPLAYS", &self.active_displays)
    }

    /// Effective active-views list, honouring `OCIO_ACTIVE_VIEWS`
    /// (`OpenColorIO.h:1116-1119`).
    pub fn effective_active_views(&self) -> Vec<String> {
        env_active_list("OCIO_ACTIVE_VIEWS", &self.active_views)
    }

    /// Returns active view names.
    #[inline]
    pub fn active_views(&self) -> &[String] {
        &self.active_views
    }

    /// The displays OCIO's `getNumDisplays` / `getDisplay(i)` enumerate (`ComputeDisplays`,
    /// `Display.cpp:73-101`): the active list (`OCIO_ACTIVE_DISPLAYS`, else `active_displays`)
    /// in its own order, names matched case-insensitively against the config's displays; every
    /// display, in config order, when there is no active list or it names none of them. The
    /// whole list, active or not, is `displays().display_names()` (`getDisplayAll`).
    pub fn display_names(&self) -> Vec<&str> {
        let all: Vec<&str> = self.displays.display_names().collect();
        let active: Vec<&str> = self
            .effective_active_displays()
            .iter()
            .filter_map(|a| all.iter().find(|d| d.eq_ignore_ascii_case(a)).copied())
            .collect();
        if active.is_empty() { all } else { active }
    }

    /// The default display: the first of [`Self::display_names`] (OCIO `getDefaultDisplay`).
    pub fn default_display(&self) -> Option<&str> {
        self.display_names().first().copied()
    }

    /// Returns the default view for a display.
    ///
    /// Mirrors OCIO C++ `Config::getDefaultView`: walks the merged
    /// local + shared view list (via [`Self::get_views`]) and
    /// returns the first entry, which has already been filtered
    /// through `active_views` when that list is non-empty. Returns
    /// `None` when the display isn't registered or exposes no
    /// active views.
    pub fn default_view(&self, display: &str) -> Option<&str> {
        self.get_views(display).first().map(|v| v.name())
    }

    /// Gets the context.
    #[inline]
    pub fn context(&self) -> &Context {
        &self.context
    }

    /// Sets the current context.
    pub fn set_context(&mut self, context: Context) {
        self.context = context;
    }

    /// Gets mutable context.
    #[inline]
    pub fn context_mut(&mut self) -> &mut Context {
        &mut self.context
    }

    /// Sets the config IO proxy.
    #[inline]
    pub fn set_config_io_proxy(&mut self, proxy: Option<Arc<dyn ConfigIOProxy>>) {
        self.io_proxy = proxy.clone();
        self.context.set_config_io_proxy(proxy);
    }

    /// Returns the config IO proxy.
    #[inline]
    pub fn config_io_proxy(&self) -> Option<Arc<dyn ConfigIOProxy>> {
        self.io_proxy.clone()
    }

    /// Returns true if a config IO proxy is set.
    #[inline]
    pub fn has_config_io_proxy(&self) -> bool {
        self.io_proxy.is_some()
    }

    /// Resolves a file path using search paths (OCIO GetAbsoluteSearchPaths order).
    ///
    /// Uses file_exists_with_proxy for existence check (proxy first, then filesystem).
    /// When search_paths is empty, uses [working_dir] only.
    /// When non-empty, uses resolved search_paths (no separate working_dir first).
    pub fn resolve_file(&self, filename: &str) -> Option<PathBuf> {
        let path = PathBuf::from(filename);

        // Absolute path: check existence (proxy or filesystem) independent of search paths
        if path.is_absolute() {
            let proxy = self.io_proxy.as_deref();
            if crate::path_utils::file_exists_with_proxy(filename, proxy) {
                return Some(path);
            }
            return None;
        }

        // Relative path: OCIO GetAbsoluteSearchPaths order
        let bases: &[PathBuf] = if self.search_paths.is_empty() {
            std::slice::from_ref(&self.working_dir)
        } else {
            &self.search_paths
        };

        for base in bases {
            let candidate = base.join(filename);
            let candidate_str = candidate.to_string_lossy();
            let proxy = self.io_proxy.as_deref();
            if crate::path_utils::file_exists_with_proxy(&candidate_str, proxy) {
                return Some(candidate);
            }
        }

        None
    }

    /// Gets color space and rule from file rules.
    pub fn colorspace_from_filepath_with_rule(&self, filepath: &str) -> Option<(&str, &FileRule)> {
        if self.file_rules.is_empty() {
            return None;
        }

        let normalized = normalize_path(filepath);

        for rule in &self.file_rules {
            match &rule.kind {
                FileRuleKind::Basic { pattern, extension } => {
                    // Single source of truth: one combined regex
                    // `^(pattern)(\.extension)$`, ported from OCIO's
                    // `BuildRegularExpression`/`ConvertToRegularExpression`
                    // (FileRules.cpp), not a glob-match-then-separate-
                    // extension-check (that was two independent checks
                    // against different case-folding and glob rules, and a
                    // *third*, differently-buggy copy lived in
                    // `filepath_only_matches_default_rule` — collapsed onto
                    // this one, see `file_rule_basic_matches`).
                    if file_rule_basic_matches(&normalized, pattern, extension.as_deref()) {
                        return Some((&rule.colorspace, rule));
                    }
                }
                FileRuleKind::Regex { regex } => {
                    // The compiled `regex` is pre-anchored with `^(?:...)$`
                    // at load time (config/loading.rs) so this is a full-path
                    // match, matching OCIO's std::regex_match semantics
                    // (FileRules.cpp:492-493) rather than a substring search.
                    if regex.is_match(&normalized) {
                        return Some((&rule.colorspace, rule));
                    }
                }
                FileRuleKind::PathSearch => {
                    // `ColorSpaceNamePathSearch` (FileRules.cpp:480,
                    // PathUtils.cpp `ParseColorSpaceFromString`): the
                    // colorspace comes from the path itself, not from a
                    // `colorspace:` field on the rule (which is always
                    // empty for this kind) — borrowed from `self.colorspaces`
                    // rather than `rule.colorspace`.
                    if let Some(cs_name) =
                        parse_colorspace_from_string(&self.colorspaces, &normalized)
                    {
                        return Some((cs_name, rule));
                    }
                }
                FileRuleKind::Default => {
                    return Some((&rule.colorspace, rule));
                }
            }
        }

        None
    }

    /// Gets color space from file rules.
    pub fn colorspace_from_filepath(&self, filepath: &str) -> Option<&str> {
        self.colorspace_from_filepath_with_rule(filepath)
            .map(|(colorspace, _)| colorspace)
    }

    /// Adds a color space to the config.
    pub fn add_colorspace(&mut self, cs: ColorSpace) {
        self.colorspaces.push(cs);
    }

    /// Adds a color space to the config with OCIO parity checks.
    pub fn add_colorspace_checked(&mut self, cs: ColorSpace) -> OcioResult<()> {
        let name = cs.name().to_string();
        if name.is_empty() {
            return Err(OcioError::Validation(
                "Color space must have a non-empty name.".into(),
            ));
        }

        if self.roles.contains(&name) {
            return Err(OcioError::Validation(format!(
                "Cannot add '{}' color space, there is already a role with this name.",
                name
            )));
        }

        if let Some(nt) = self.named_transform(&name) {
            return Err(OcioError::Validation(format!(
                "Cannot add '{}' color space, there is already a named transform using this name: '{}'.",
                name,
                nt.name()
            )));
        }

        for alias in cs.aliases() {
            if self.roles.contains(alias) {
                return Err(OcioError::Validation(format!(
                    "Cannot add '{}' color space, it has an alias '{}' that conflicts with a role.",
                    name, alias
                )));
            }
            if let Some(nt) = self.named_transform(alias) {
                return Err(OcioError::Validation(format!(
                    "Cannot add '{}' color space, it has an alias '{}' that conflicts with named transform '{}'.",
                    name,
                    alias,
                    nt.name()
                )));
            }
        }

        self.colorspaces.push(cs);
        Ok(())
    }

    /// Adds a display to the config.
    pub fn add_display(&mut self, display: Display) {
        self.displays.add_display(display);
    }

    /// Adds a look to the config.
    pub fn add_look(&mut self, look: Look) {
        self.looks.add(look);
    }

    /// Removes a look by name.
    pub fn remove_look(&mut self, name: &str) -> Option<Look> {
        self.looks.remove(name)
    }

    /// Clears all looks.
    pub fn clear_looks(&mut self) {
        self.looks.clear();
    }

    /// Adds a view to a display, creating the display if needed.
    ///
    /// Reference: OCIO `Config::addDisplayView(display, view, viewTransform, colorSpace, looks,
    /// rule, description)`; everything after `display` travels in [`AddDisplayViewParams`].
    pub fn add_display_view(
        &mut self,
        display: &str,
        params: AddDisplayViewParams<'_>,
    ) -> OcioResult<()> {
        let AddDisplayViewParams {
            view,
            view_transform,
            colorspace,
            looks,
            rule,
            description,
        } = params;
        if display.is_empty() {
            return Err(OcioError::Validation(
                "View could not be added to display: display name is empty.".into(),
            ));
        }
        if view.is_empty() {
            return Err(OcioError::Validation(
                "View could not be added to display: view name is empty.".into(),
            ));
        }
        if colorspace.is_empty() {
            return Err(OcioError::Validation(
                "View could not be added to display: colorspace name is empty.".into(),
            ));
        }

        if self.displays.display(display).is_none() {
            self.displays.add_display(Display::new(display));
        }
        let display_entry = self
            .displays
            .display_mut(display)
            .expect("display must exist");

        if display_entry.has_shared_view_ref(view) {
            return Err(OcioError::Validation(format!(
                "There is already a shared view named '{}' in display '{}'.",
                view, display
            )));
        }
        if display_entry.has_view(view) {
            return Err(OcioError::Validation(format!(
                "There is already a view named '{}' in display '{}'.",
                view, display
            )));
        }

        let mut v = View::new(view, colorspace);
        if let Some(looks) = looks
            && !looks.is_empty()
        {
            v = v.with_look(looks);
        }
        if let Some(vt) = view_transform
            && !vt.is_empty()
        {
            v = v.with_view_transform(vt);
        }
        if let Some(rule) = rule
            && !rule.is_empty()
        {
            v = v.with_rule(rule);
        }
        if let Some(desc) = description
            && !desc.is_empty()
        {
            v = v.with_description(desc);
        }

        display_entry.add_view(v);
        Ok(())
    }

    /// Removes a view from a display.
    pub fn remove_display_view(&mut self, display: &str, view: &str) -> bool {
        self.displays
            .display_mut(display)
            .and_then(|d| d.remove_view(view))
            .is_some()
    }

    /// Adds a shared view reference to a display (OCIO v2.3+).
    pub fn add_display_shared_view(&mut self, display: &str, view: &str) -> OcioResult<()> {
        if display.is_empty() {
            return Err(OcioError::Validation(
                "Shared view could not be added to display: display name is empty.".into(),
            ));
        }
        if view.is_empty() {
            return Err(OcioError::Validation(
                "Shared view could not be added to display: view name is empty.".into(),
            ));
        }

        if self.displays.display(display).is_none() {
            self.displays.add_display(Display::new(display));
        }
        let display_entry = self
            .displays
            .display_mut(display)
            .expect("display must exist");

        if display_entry.has_view(view) {
            return Err(OcioError::Validation(format!(
                "There is already a view named '{}' in display '{}'.",
                view, display
            )));
        }
        if display_entry.has_shared_view_ref(view) {
            return Err(OcioError::Validation(format!(
                "There is already a shared view named '{}' in display '{}'.",
                view, display
            )));
        }

        display_entry.add_shared_view_ref(view.to_string());
        Ok(())
    }

    /// Sets a role mapping.
    pub fn set_role(&mut self, role: impl Into<String>, colorspace: impl Into<String>) {
        self.roles.define(role, colorspace);
    }

    // ========================================================================
    // Config creation methods
    // ========================================================================

    /// Creates a minimal "raw" configuration.
    ///
    /// This config only contains a "Raw" data color space with no transforms.
    /// Useful for applications that need to bypass color management.
    ///
    /// # Example
    ///
    /// ```
    /// use vfx_ocio::Config;
    ///
    /// let config = Config::create_raw();
    /// assert!(config.colorspace("Raw").is_some());
    /// ```
    pub fn create_raw() -> Self {
        use crate::colorspace::{ColorSpace, Encoding, Family};

        let mut config = Self::new();
        config.name = "Raw Config".to_string();
        config.version = ConfigVersion::V2;
        config.version_minor = 0;

        // Add a Raw/data color space
        let raw = ColorSpace::builder("Raw")
            .encoding(Encoding::Data)
            .family(Family::Utility)
            .description("Raw data bypass - no color processing")
            .is_data(true)
            .build();

        config.colorspaces.push(raw);
        config.roles.define(crate::role::names::REFERENCE, "Raw");
        config.roles.define(crate::role::names::DATA, "Raw");
        config.roles.define(crate::role::names::DEFAULT, "Raw");

        config
    }

    // ========================================================================
    // Version access
    // ========================================================================

    /// Gets the major version number.
    ///
    /// Returns 1 for OCIO v1.x configs, 2 for OCIO v2.x configs.
    pub fn major_version(&self) -> u32 {
        match self.version {
            ConfigVersion::V1 => 1,
            ConfigVersion::V2 => 2,
        }
    }

    /// Gets the minor version number.
    ///
    /// Currently returns 0 as we don't track minor versions.
    pub fn minor_version(&self) -> u32 {
        self.version_minor
    }

    /// Returns the family separator string.
    pub fn family_separator(&self) -> &str {
        &self.family_separator
    }

    /// Sets the family separator string.
    pub fn set_family_separator(&mut self, sep: impl Into<String>) {
        self.family_separator = sep.into();
    }

    /// Returns the default view transform name.
    pub fn default_view_transform(&self) -> Option<&str> {
        self.default_view_transform.as_deref()
    }

    /// Sets the default view transform name.
    pub fn set_default_view_transform(&mut self, name: impl Into<String>) {
        self.default_view_transform = Some(name.into());
    }

    /// Returns whether strict parsing is enabled.
    pub fn is_strict_parsing_enabled(&self) -> bool {
        self.strict_parsing
    }

    /// Enables/disables strict parsing.
    pub fn set_strict_parsing_enabled(&mut self, enabled: bool) {
        self.strict_parsing = enabled;
    }

    /// Returns processor cache flags (OCIO parity).
    pub fn processor_cache_flags(&self) -> u32 {
        self.processor_cache_flags
    }

    /// Sets processor cache flags (OCIO parity).
    pub fn set_processor_cache_flags(&mut self, flags: u32) {
        self.processor_cache_flags = flags;
    }

    /// Sets the config version.
    pub fn set_version(&mut self, major: u32, minor: u32) {
        self.version = if major >= 2 {
            ConfigVersion::V2
        } else {
            ConfigVersion::V1
        };
        self.version_minor = minor;
    }

    /// Bumps the config version to the latest known (OCIO v2.5).
    /// Mirrors OCIO C++ `Config::upgradeToLatestVersion`. v1 → v2
    /// configs adopt the v2 schema implicitly; v2 → v2.5 just
    /// re-stamps the minor version.
    pub fn upgrade_to_latest_version(&mut self) {
        self.version = ConfigVersion::V2;
        // Track upstream when bumping: keep aligned with the highest
        // `ocio_profile_version` field the loader accepts.
        self.version_minor = 5;
    }

    // ========================================================================
    // Config metadata
    // ========================================================================

    /// Sets the config name.
    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    /// Sets the config description (OCIO `Config::setDescription`). Independent of the name.
    pub fn set_description(&mut self, description: impl Into<String>) {
        self.description = description.into();
    }

    /// Gets the config description (OCIO `Config::getDescription`, the `description:` key).
    pub fn description(&self) -> &str {
        &self.description
    }

    // ========================================================================
    // Search path management
    // ========================================================================

    /// Adds a search path for LUT files.
    pub fn add_search_path(&mut self, path: impl Into<PathBuf>) {
        self.search_paths.push(path.into());
    }

    /// Sets all search paths.
    pub fn set_search_paths(&mut self, paths: Vec<PathBuf>) {
        self.search_paths = paths;
    }

    /// Clears all search paths.
    pub fn clear_search_paths(&mut self) {
        self.search_paths.clear();
    }

    /// Sets the working directory.
    pub fn set_working_dir(&mut self, dir: impl Into<PathBuf>) {
        self.working_dir = dir.into();
        self.working_dir_is_default = false;
    }

    // ========================================================================
    // File rules management
    // ========================================================================

    /// Adds a file rule.
    pub fn add_file_rule(&mut self, rule: FileRule) {
        self.file_rules.push(rule);
    }

    /// Gets all file rules.
    pub fn file_rules(&self) -> &[FileRule] {
        &self.file_rules
    }

    /// Sets all file rules.
    pub fn set_file_rules(&mut self, rules: Vec<FileRule>) {
        self.file_rules = rules;
    }

    /// Clears all file rules.
    pub fn clear_file_rules(&mut self) {
        self.file_rules.clear();
    }

    /// Returns all file references found in transforms.
    pub fn file_references(&self) -> Vec<String> {
        let mut files = std::collections::BTreeSet::new();
        self.collect_file_references(&mut files);
        files.into_iter().collect()
    }

    /// Returns true if the file path only matches the Default rule.
    ///
    /// OCIO ref `FileRules::Impl::filepathOnlyMatchesDefaultRule`
    /// (`FileRules.cpp:903-908`) does not re-derive matching — it calls the
    /// SAME `getColorSpaceFromFilepath` used everywhere else and checks
    /// whether the rule it landed on is the last one. Do the same here
    /// instead of re-implementing rule matching a second time (a second,
    /// independently-buggy copy of this logic used to live here — collapsed
    /// onto `colorspace_from_filepath_with_rule`).
    pub fn filepath_only_matches_default_rule(&self, file_path: &str) -> bool {
        if self.file_rules.is_empty() {
            return true;
        }
        match self.colorspace_from_filepath_with_rule(file_path) {
            Some((_, rule)) => std::ptr::eq(rule, self.file_rules.last().unwrap()),
            None => false,
        }
    }

    /// Returns true if the config can be archived (OCIO parity).
    pub fn is_archivable(&self) -> bool {
        if matches!(
            self.source,
            ConfigSource::Stream | ConfigSource::Archive | ConfigSource::IOProxy
        ) && self.working_dir_is_default
        {
            return false;
        }

        let working_dir = &self.working_dir;

        for path in &self.search_paths {
            if !is_archivable_path(working_dir, path) {
                return false;
            }
        }

        for file in self.file_references() {
            let path = PathBuf::from(file);
            if !is_archivable_path(working_dir, &path) {
                return false;
            }
        }

        true
    }

    /// Archives the config to a .ocioz file at the given path.
    pub fn archive_to_path(&self, output_path: impl AsRef<Path>) -> OcioResult<()> {
        let output_path = output_path.as_ref();
        if !self.is_archivable() {
            return Err(OcioError::Validation(
                "config is not archivable (invalid search paths or file references)".into(),
            ));
        }

        let contents = self.serialize()?;
        archive_config_to_path(
            output_path,
            &contents,
            &self.working_dir,
            &supported_lut_extensions(),
        )
    }

    // ========================================================================
    // Validation
    // ========================================================================

    /// Validates the configuration.
    ///
    /// Checks for:
    /// - At least one color space defined
    /// - Reference role is defined
    /// - All role color spaces exist
    /// - All display view color spaces exist
    ///
    /// Returns a list of validation errors/warnings.
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();

        // Check for color spaces
        if self.colorspaces.is_empty() {
            errors.push("Config has no color spaces defined".to_string());
        }

        // Check reference role
        if !self.roles.has_reference() {
            errors.push("Config is missing the 'reference' role".to_string());
        }

        // Check that all roles point to existing color spaces
        for (role, cs_name) in self.roles.iter() {
            if self.colorspace(cs_name).is_none() {
                errors.push(format!(
                    "Role '{}' references non-existent color space '{}'",
                    role, cs_name
                ));
            }
        }

        // Check display/view color spaces
        for display in self.displays.displays() {
            for view in display.views() {
                let cs_name = view.colorspace();
                if self.colorspace(cs_name).is_none() {
                    errors.push(format!(
                        "View '{}' in display '{}' references non-existent color space '{}'",
                        view.name(),
                        display.name(),
                        cs_name
                    ));
                }
            }
        }

        // Check look process spaces
        for look in self.looks.all() {
            if let Some(ps) = look.get_process_space()
                && !ps.is_empty()
                && self.colorspace(ps).is_none()
            {
                errors.push(format!(
                    "Look '{}' references non-existent process space '{}'",
                    look.name(),
                    ps
                ));
            }
        }

        // Check file rules (v2)
        if self.version == ConfigVersion::V2 && !self.file_rules.is_empty() {
            // Count Default rules
            let default_count = self
                .file_rules
                .iter()
                .filter(|r| matches!(r.kind, FileRuleKind::Default))
                .count();

            if default_count == 0 {
                errors.push(
                    "File rules must include a Default rule (OCIO v2 requirement)".to_string(),
                );
            } else if default_count > 1 {
                errors.push(format!(
                    "File rules must have exactly one Default rule, found {}",
                    default_count
                ));
            }

            // Check that Default rule is last
            let last_is_default = self
                .file_rules
                .last()
                .map(|r| matches!(r.kind, FileRuleKind::Default))
                .unwrap_or(false);
            if !last_is_default && default_count > 0 {
                errors.push("Default rule must be the last file rule".to_string());
            }

            // Check that all file rules have valid color spaces (including Default)
            for rule in &self.file_rules {
                // `ColorSpaceNamePathSearch` never carries a colorspace of
                // its own (ref: `setColorSpace` throws if you try to give it
                // one) — it resolves dynamically from the matched path, so
                // there is nothing to validate here.
                if matches!(rule.kind, FileRuleKind::PathSearch) {
                    continue;
                }
                // Default rule colorspace can be a role name or direct colorspace
                let cs_name = &rule.colorspace;
                let is_valid =
                    self.colorspace(cs_name).is_some() || self.roles.get(cs_name).is_some();

                if !is_valid {
                    errors.push(format!(
                        "File rule '{}' references non-existent color space or role '{}'",
                        rule.name, rule.colorspace
                    ));
                }
            }
        }

        // Check environment declarations (OCIO v2 behavior)
        for (name, value) in &self.environment_vars {
            let unresolved = unresolved_env_vars(value);
            if !unresolved.is_empty() {
                errors.push(format!(
                    "Unresolved context variable in environment declaration '{} = {}'.",
                    name, value
                ));
            }
        }

        errors
    }

    /// Checks if the config is valid.
    ///
    /// Returns true if `validate()` returns no errors.
    pub fn is_valid(&self) -> bool {
        self.validate().is_empty()
    }

    // ========================================================================
    // Serialization
    // ========================================================================

    /// Writes the config to a file.
    pub fn write_to_file(&self, path: impl AsRef<Path>) -> OcioResult<()> {
        let content = self.serialize()?;
        std::fs::write(path, content)?;
        Ok(())
    }

    // ========================================================================
    // Environment variable management
    // ========================================================================

    /// Sets an environment variable in the context.
    pub fn set_environment_var(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        let value = value.into();
        self.context.set_var(name.clone(), value.clone());
        if let Some(entry) = self.environment_vars.iter_mut().find(|(k, _)| k == &name) {
            *entry = (name, value);
        } else {
            self.environment_vars.push((name, value));
        }
    }

    /// Adds an environment variable (alias for set_environment_var).
    pub fn add_environment_var(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.set_environment_var(name, value);
    }

    /// Removes an environment variable by name.
    pub fn remove_environment_var(&mut self, name: &str) -> bool {
        if let Some(index) = self.environment_vars.iter().position(|(k, _)| k == name) {
            let (key, _) = self.environment_vars.remove(index);
            self.context.remove_var(&key);
            return true;
        }
        false
    }

    /// Clears all environment variables.
    pub fn clear_environment_vars(&mut self) {
        for (key, _) in &self.environment_vars {
            self.context.remove_var(key);
        }
        self.environment_vars.clear();
    }

    /// Gets an environment variable from the context.
    pub fn environment_var(&self, name: &str) -> Option<&str> {
        self.environment_vars
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// Returns the number of environment variables.
    pub fn num_environment_vars(&self) -> usize {
        self.environment_vars.len()
    }

    /// Gets environment variable name at index.
    pub fn environment_var_name_at(&self, index: usize) -> Option<&str> {
        self.environment_vars.get(index).map(|(k, _)| k.as_str())
    }

    /// Gets environment variable value at index.
    pub fn environment_var_value_at(&self, index: usize) -> Option<&str> {
        self.environment_vars.get(index).map(|(_, v)| v.as_str())
    }

    /// Returns the environment mode.
    pub fn environment_mode(&self) -> EnvironmentMode {
        self.environment_mode
    }

    /// Sets the environment mode.
    pub fn set_environment_mode(&mut self, mode: EnvironmentMode) {
        self.environment_mode = mode;
    }

    /// Loads environment variables into the context based on the current mode.
    pub fn load_environment(&mut self) {
        if self.environment_mode == EnvironmentMode::LoadAll {
            for (key, value) in std::env::vars() {
                self.context.set_var(key, value);
            }
        } else {
            for (name, value) in &self.environment_vars {
                if let Ok(resolved) = resolve_env_value(value) {
                    self.context.set_var(name.clone(), resolved);
                }
            }
        }
    }

    // ========================================================================
    // Color space management extensions
    // ========================================================================

    /// Returns color spaces filtered by family.
    pub fn colorspaces_by_family(&self, family: crate::colorspace::Family) -> Vec<&ColorSpace> {
        self.colorspaces
            .iter()
            .filter(|cs| cs.family() == family)
            .collect()
    }

    /// Returns color spaces filtered by encoding.
    pub fn colorspaces_by_encoding(
        &self,
        encoding: crate::colorspace::Encoding,
    ) -> Vec<&ColorSpace> {
        self.colorspaces
            .iter()
            .filter(|cs| cs.encoding() == encoding)
            .collect()
    }

    /// Returns all linear color spaces.
    pub fn linear_colorspaces(&self) -> Vec<&ColorSpace> {
        self.colorspaces
            .iter()
            .filter(|cs| cs.encoding().is_linear())
            .collect()
    }

    /// Returns all data/non-color color spaces.
    pub fn data_colorspaces(&self) -> Vec<&ColorSpace> {
        self.colorspaces.iter().filter(|cs| cs.is_data()).collect()
    }

    /// Checks if a color space is linear.
    pub fn is_colorspace_linear(&self, name: &str) -> bool {
        self.colorspace(name)
            .map(|cs| cs.encoding().is_linear())
            .unwrap_or(false)
    }

    /// Checks if a color space is linear with respect to a reference space.
    ///
    /// OIIO parity: `Config::isColorSpaceLinear(colorSpace, referenceSpaceType)`.
    ///
    /// The method checks whether the color space is linear based on:
    /// 1. The encoding attribute (if set to "scene-linear" or "display-linear")
    /// 2. Sampling the transform to detect linearity
    ///
    /// # Arguments
    ///
    /// * `colorspace` - Color space name to evaluate
    /// * `reference_space_type` - Evaluate linearity with respect to scene-referred
    ///   or display-referred reference space
    ///
    /// # Returns
    ///
    /// `true` if the color space is considered linear for the given reference space.
    pub fn is_colorspace_linear_for_reference(
        &self,
        colorspace: &str,
        reference_space_type: ReferenceSpaceType,
    ) -> bool {
        let cs = match self.colorspace(colorspace) {
            Some(cs) => cs,
            None => return false,
        };

        // Check encoding attribute first - this gives config authors ultimate control
        match cs.encoding() {
            Encoding::SceneLinear => {
                return reference_space_type == ReferenceSpaceType::Scene;
            }
            Encoding::DisplayLinear => {
                return reference_space_type == ReferenceSpaceType::Display;
            }
            Encoding::Data => {
                // Data spaces are not color spaces
                return false;
            }
            _ => {}
        }

        // No explicit encoding - need to check by sampling the transform
        // Create a processor from the colorspace to reference and check if it's identity-like
        let reference_cs: Option<&str> = match reference_space_type {
            ReferenceSpaceType::Scene => self
                .role_colorspace(crate::role::names::SCENE_LINEAR)
                .or_else(|| self.role_colorspace(crate::role::names::REFERENCE)),
            ReferenceSpaceType::Display => self
                .role_colorspace(crate::role::names::COMPOSITING_LINEAR)
                .or(Some("display")),
        };

        let reference_cs = match reference_cs {
            Some(r) => r,
            None => return false,
        };

        // If the colorspace IS the reference, it's linear
        if colorspace.eq_ignore_ascii_case(reference_cs) {
            return true;
        }

        // Try to create a processor and check if transform is effectively identity
        let proc = match self.processor(colorspace, reference_cs) {
            Ok(p) => p,
            Err(_) => return false,
        };

        // Sample transform at a few points to check linearity
        // If all samples transform to themselves (within tolerance), it's linear
        let test_values = [
            [0.0, 0.0, 0.0],
            [0.18, 0.18, 0.18],
            [1.0, 1.0, 1.0],
            [0.5, 0.5, 0.5],
        ];
        let tolerance = 1e-6_f32;

        for input in &test_values {
            let mut output = *input;
            proc.apply_rgb(std::slice::from_mut(&mut output));

            for (i, &o) in output.iter().enumerate() {
                if (o - input[i]).abs() > tolerance {
                    return false;
                }
            }
        }

        true
    }

    /// Checks if a color space is used anywhere in the config.
    pub fn is_colorspace_used(&self, name: &str) -> bool {
        if name.is_empty() {
            return false;
        }

        // Roles
        for (_, cs) in self.roles.iter() {
            if cs.eq_ignore_ascii_case(name) {
                return true;
            }
        }

        // Displays and shared views
        for display in self.displays.displays() {
            for view in display.views() {
                if view.colorspace().eq_ignore_ascii_case(name) {
                    return true;
                }
            }

            for shared_name in display.shared_view_refs() {
                if let Some(shared) = self
                    .shared_views
                    .iter()
                    .find(|sv| sv.name() == *shared_name)
                    && shared
                        .effective_colorspace(display.name())
                        .eq_ignore_ascii_case(name)
                {
                    return true;
                }
            }
        }

        // Looks
        for look in self.looks.all() {
            if let Some(ps) = look.get_process_space()
                && ps.eq_ignore_ascii_case(name)
            {
                return true;
            }
        }

        // File rules (resolve role names)
        for rule in &self.file_rules {
            let resolved = self
                .roles
                .get(&rule.colorspace)
                .unwrap_or(rule.colorspace.as_str());
            if resolved.eq_ignore_ascii_case(name) {
                return true;
            }
        }

        // Transforms
        fn collect_color_refs(transform: &Transform, refs: &mut Vec<String>) {
            match transform {
                Transform::Group(group) => {
                    for child in &group.transforms {
                        collect_color_refs(child, refs);
                    }
                }
                Transform::ColorSpace(cs) => {
                    refs.push(cs.src.clone());
                    refs.push(cs.dst.clone());
                }
                Transform::DisplayView(dv) => {
                    refs.push(dv.src.clone());
                }
                Transform::Look(lt) => {
                    refs.push(lt.src.clone());
                    refs.push(lt.dst.clone());
                }
                _ => {}
            }
        }

        let mut refs = Vec::new();
        for cs in &self.colorspaces {
            // Walk every reference-direction accessor; display-referred
            // colorspaces populate the `*_display_reference` pair and
            // would otherwise be invisible to the walk.
            if let Some(t) = cs.to_reference() {
                collect_color_refs(t, &mut refs);
            }
            if let Some(t) = cs.from_reference() {
                collect_color_refs(t, &mut refs);
            }
            if let Some(t) = cs.to_display_reference() {
                collect_color_refs(t, &mut refs);
            }
            if let Some(t) = cs.from_display_reference() {
                collect_color_refs(t, &mut refs);
            }
        }

        for name_ref in refs {
            if name_ref.eq_ignore_ascii_case(name) {
                return true;
            }
        }

        false
    }

    /// Removes a color space by name.
    ///
    /// Returns true if the color space was found and removed.
    pub fn remove_colorspace(&mut self, name: &str) -> bool {
        if let Some(idx) = self.colorspaces.iter().position(|cs| cs.matches_name(name)) {
            self.colorspaces.remove(idx);
            self.inactive_colorspaces
                .retain(|cs| !cs.eq_ignore_ascii_case(name));
            true
        } else {
            false
        }
    }

    /// Clears all color spaces.
    pub fn clear_colorspaces(&mut self) {
        self.colorspaces.clear();
        self.inactive_colorspaces.clear();
    }

    /// Gets the list of inactive (hidden) color spaces.
    /// OCIO ref: getInactiveColorSpaces().
    pub fn get_inactive_colorspaces(&self) -> &[String] {
        &self.inactive_colorspaces
    }

    /// Effective inactive-colorspaces list, honouring
    /// `OCIO_INACTIVE_COLORSPACES` (`OpenColorIO.h:683-689` — env
    /// takes precedence over the config-file value).
    pub fn effective_inactive_colorspaces(&self) -> Vec<String> {
        env_overridden_list("OCIO_INACTIVE_COLORSPACES", &self.inactive_colorspaces)
    }

    /// Sets the list of inactive (hidden) color spaces.
    /// OCIO ref: setInactiveColorSpaces().
    pub fn set_inactive_colorspaces(&mut self, names: Vec<String>) {
        self.inactive_colorspaces = names;
    }

    /// Adds a color space to the inactive list.
    pub fn add_inactive_colorspace(&mut self, name: impl Into<String>) {
        self.inactive_colorspaces.push(name.into());
    }

    /// Checks if a color space is inactive (hidden).
    /// OCIO ref: isInactiveColorSpace().
    pub fn is_colorspace_inactive(&self, name: &str) -> bool {
        self.inactive_colorspaces
            .iter()
            .any(|n| n.eq_ignore_ascii_case(name))
    }

    /// Returns only active (non-hidden) color spaces.
    pub fn active_colorspaces(&self) -> Vec<&ColorSpace> {
        self.colorspaces
            .iter()
            .filter(|cs| !self.is_colorspace_inactive(cs.name()))
            .collect()
    }

    // ========================================================================
    // Display management extensions
    // ========================================================================

    /// The number of [`Self::display_names`] (OCIO `getNumDisplays`).
    pub fn num_displays(&self) -> usize {
        self.display_names().len()
    }

    /// The display name at `index` in [`Self::display_names`] (OCIO `getDisplay`).
    pub fn display_name_at(&self, index: usize) -> Option<&str> {
        self.display_names().get(index).copied()
    }

    /// Adds a view to a display.
    pub fn add_view(&mut self, display: &str, view: View) {
        if let Some(d) = self.displays.display_mut(display) {
            d.add_view(view);
        }
    }

    /// Removes a display by name.
    pub fn remove_display(&mut self, name: &str) {
        self.displays.remove_display(name);
    }

    /// Clears all displays.
    pub fn clear_displays(&mut self) {
        self.displays.clear_displays();
    }

    /// Checks whether a display is temporary.
    pub fn is_display_temporary(&self, display: &str) -> bool {
        self.displays
            .display(display)
            .map(|d| d.is_temporary())
            .unwrap_or(false)
    }

    /// Sets whether a display is temporary.
    pub fn set_display_temporary(&mut self, display: &str, temporary: bool) {
        if let Some(d) = self.displays.display_mut(display) {
            d.set_temporary(temporary);
        }
    }

    /// Sets active displays list.
    pub fn set_active_displays(&mut self, displays: Vec<String>) {
        self.active_displays = displays;
    }

    /// Adds an active display.
    pub fn add_active_display(&mut self, display: impl Into<String>) {
        self.active_displays.push(display.into());
    }

    /// Removes an active display.
    pub fn remove_active_display(&mut self, display: &str) {
        self.active_displays.retain(|d| d != display);
    }

    /// Clears all active displays.
    pub fn clear_active_displays(&mut self) {
        self.active_displays.clear();
    }

    /// Sets active views list.
    pub fn set_active_views(&mut self, views: Vec<String>) {
        self.active_views = views;
    }

    /// Adds an active view.
    pub fn add_active_view(&mut self, view: impl Into<String>) {
        self.active_views.push(view.into());
    }

    /// Removes an active view.
    pub fn remove_active_view(&mut self, view: &str) {
        self.active_views.retain(|v| v != view);
    }

    /// Clears all active views.
    pub fn clear_active_views(&mut self) {
        self.active_views.clear();
    }

    // ========================================================================
    // Role management extensions
    // ========================================================================

    /// Returns the number of defined roles.
    #[inline]
    pub fn num_roles(&self) -> usize {
        self.roles.len()
    }

    /// Returns role name at index.
    pub fn role_name_at(&self, index: usize) -> Option<&str> {
        self.roles.iter().nth(index).map(|(name, _)| name)
    }

    /// Returns color space name for role at index.
    pub fn role_colorspace_at(&self, index: usize) -> Option<&str> {
        self.roles.iter().nth(index).map(|(_, cs)| cs)
    }

    /// Returns color space name assigned to a role.
    /// OCIO ref: Config::getRoleColorSpace(role).
    pub fn role_colorspace(&self, role: &str) -> Option<&str> {
        self.roles.get(role)
    }

    /// Checks if a role is defined.
    #[inline]
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.contains(role)
    }

    /// Removes a role.
    pub fn remove_role(&mut self, role: &str) -> Option<String> {
        self.roles.remove(role)
    }

    /// Clears all roles.
    pub fn clear_roles(&mut self) {
        self.roles.clear();
    }

    // ========================================================================
    // Look management extensions
    // ========================================================================

    /// Returns the number of looks.
    #[inline]
    pub fn num_looks(&self) -> usize {
        self.looks.len()
    }

    /// Returns look name at index.
    pub fn look_name_at(&self, index: usize) -> Option<&str> {
        self.looks.all().get(index).map(|l| l.name())
    }

    /// Checks if a look is defined.
    pub fn has_look(&self, name: &str) -> bool {
        self.looks.get(name).is_some()
    }

    /// Returns the look by name.
    pub fn look(&self, name: &str) -> Option<&Look> {
        self.looks.get(name)
    }

    // ========================================================================
    // View transform management (OCIO v2)
    // ========================================================================

    /// Returns the number of view transforms.
    #[inline]
    pub fn num_view_transforms(&self) -> usize {
        self.displays.num_view_transforms()
    }

    /// Returns all view transforms.
    #[inline]
    pub fn view_transforms(&self) -> &[ViewTransform] {
        self.displays.view_transforms()
    }

    /// Returns a view transform by name.
    pub fn view_transform(&self, name: &str) -> Option<&ViewTransform> {
        self.displays.view_transform(name)
    }

    /// Adds a view transform.
    pub fn add_view_transform(&mut self, vt: ViewTransform) {
        self.displays.add_view_transform(vt);
    }

    /// Removes a view transform by name.
    pub fn remove_view_transform(&mut self, name: &str) -> Option<ViewTransform> {
        self.displays.remove_view_transform(name)
    }

    /// Clears all view transforms.
    pub fn clear_view_transforms(&mut self) {
        self.displays.clear_view_transforms();
    }

    /// Returns view transform name at index.
    pub fn view_transform_name_at(&self, index: usize) -> Option<&str> {
        self.displays
            .view_transforms()
            .get(index)
            .map(|vt| vt.name())
    }

    // ========================================================================
    // Named transform support (OCIO v2)
    // ========================================================================

    /// Returns number of named transforms.
    pub fn num_named_transforms(&self) -> usize {
        self.named_transforms.len()
    }

    /// Returns number of named transforms with visibility filter.
    pub fn num_named_transforms_with_visibility(
        &self,
        visibility: NamedTransformVisibility,
    ) -> usize {
        self.named_transforms_with_visibility(visibility).len()
    }

    /// Returns all named transforms.
    pub fn named_transforms(&self) -> &[NamedTransform] {
        &self.named_transforms
    }

    /// Adds a named transform.
    pub fn add_named_transform(&mut self, nt: NamedTransform) {
        self.named_transforms.push(nt);
    }

    /// Removes a named transform by name.
    pub fn remove_named_transform(&mut self, name: &str) -> bool {
        if let Some(index) = self
            .named_transforms
            .iter()
            .position(|nt| nt.matches_name(name))
        {
            self.named_transforms.remove(index);
            return true;
        }
        false
    }

    /// Clears all named transforms.
    pub fn clear_named_transforms(&mut self) {
        self.named_transforms.clear();
    }

    /// Returns named transforms filtered by visibility.
    pub fn named_transforms_with_visibility(
        &self,
        visibility: NamedTransformVisibility,
    ) -> Vec<&NamedTransform> {
        self.named_transforms
            .iter()
            .filter(|nt| match visibility {
                NamedTransformVisibility::All => true,
                NamedTransformVisibility::Active => !self.is_named_transform_inactive(&nt.name),
                NamedTransformVisibility::Inactive => self.is_named_transform_inactive(&nt.name),
            })
            .collect()
    }

    /// Gets a named transform by name.
    ///
    /// Matches by primary name OR any alias, case-insensitively — was an
    /// exact case-sensitive primary-name-only comparison, unlike
    /// `Self::colorspace`'s `ColorSpace::matches_name`, even though
    /// `NamedTransform` carries the identical `aliases` field. A named
    /// transform referenced only by its alias (or by a differently-cased
    /// name) was unreachable through this lookup.
    pub fn named_transform(&self, name: &str) -> Option<&NamedTransform> {
        self.named_transforms
            .iter()
            .find(|nt| nt.matches_name(name))
    }

    /// Returns named transform name by index with visibility.
    pub fn named_transform_name_by_index(
        &self,
        visibility: NamedTransformVisibility,
        index: usize,
    ) -> Option<&str> {
        self.named_transforms_with_visibility(visibility)
            .get(index)
            .map(|nt| nt.name.as_str())
    }

    /// Returns index for a named transform (all visibility).
    pub fn index_for_named_transform(&self, name: &str) -> Option<usize> {
        self.named_transforms
            .iter()
            .position(|nt| nt.matches_name(name))
    }

    /// Gets the list of inactive (hidden) named transforms.
    pub fn get_inactive_named_transforms(&self) -> &[String] {
        &self.inactive_named_transforms
    }

    /// Sets the list of inactive (hidden) named transforms.
    pub fn set_inactive_named_transforms(&mut self, names: Vec<String>) {
        self.inactive_named_transforms = names;
    }

    /// Adds a named transform to the inactive list.
    pub fn add_inactive_named_transform(&mut self, name: impl Into<String>) {
        self.inactive_named_transforms.push(name.into());
    }

    /// Checks if a named transform is inactive (hidden).
    pub fn is_named_transform_inactive(&self, name: &str) -> bool {
        self.inactive_named_transforms
            .iter()
            .any(|n| n.eq_ignore_ascii_case(name))
    }

    // ========================================================================
    // Shared view support (OCIO v2)
    // ========================================================================

    /// Gets the number of shared views.
    /// Shared views are display-independent views defined at the config level.
    pub fn num_shared_views(&self) -> usize {
        self.shared_views.len()
    }

    /// Returns shared views (OCIO v2.3+).
    pub fn shared_views(&self) -> &[View] {
        &self.shared_views
    }

    /// Returns every view for `display` — local entries first, then
    /// each shared-view reference resolved against [`Self::shared_views`]
    /// (OCIO C++ `Config::Impl::getViews(display)`), then filtered AND
    /// ordered by the active views (`OCIO_ACTIVE_VIEWS`, else
    /// `active_views`) as `Config::Impl::getActiveViews` does: the views
    /// the active list names, in the active list's order, names compared
    /// case-insensitively; all views when the list is empty or names none
    /// of them.
    ///
    /// Returns an empty `Vec` when the display isn't registered. Lookup
    /// by view name should walk this list rather than calling
    /// [`Display::view`] directly, which only sees local entries.
    pub fn get_views(&self, display: &str) -> Vec<&View> {
        let Some(disp) = self.displays.display(display) else {
            return Vec::new();
        };
        let mut out: Vec<&View> = disp.views().iter().collect();
        for shared_name in disp.shared_view_refs() {
            if let Some(shared) = self
                .shared_views
                .iter()
                .find(|sv| sv.name() == *shared_name)
            {
                out.push(shared);
            }
        }
        // `IntersectStringVecsCaseIgnore(activeViews, views)`: the active list's order.
        let active = self.effective_active_views();
        let ordered: Vec<&View> = active
            .iter()
            .filter_map(|a| {
                out.iter()
                    .find(|v| v.name().eq_ignore_ascii_case(a))
                    .copied()
            })
            .collect();
        if ordered.is_empty() { out } else { ordered }
    }

    /// Resolves one `(display, view)` pair, as OCIO's `Config::Impl::getView`
    /// (`Config.cpp:785`): names compare case-insensitively, the active-views list does NOT
    /// filter (it only shapes the `getViews` listing), and the view is looked up among the
    /// shared views when `display` is empty or lists it as a shared-view reference, else among
    /// the display's own. `None` for an empty view or an unknown display.
    pub fn find_view(&self, display: &str, view: &str) -> Option<&View> {
        if view.is_empty() {
            return None;
        }
        let by_name = |v: &&View| v.name().eq_ignore_ascii_case(view);
        if display.is_empty() {
            return self.shared_views.iter().find(by_name);
        }
        let disp = self.displays.display(display)?;
        if disp
            .shared_view_refs()
            .iter()
            .any(|s| s.eq_ignore_ascii_case(view))
        {
            self.shared_views.iter().find(by_name)
        } else {
            disp.views().iter().find(by_name)
        }
    }

    /// Views for a display restricted to those applicable to
    /// `colorspace` per the config's `viewing_rules`. Mirrors OCIO
    /// C++ `Config::getNumViews(display, colorspaceName)`. Views with
    /// no rule attached are always applicable; views whose rule
    /// matches the colorspace's name, role, or encoding are kept;
    /// views with a rule that doesn't match are dropped. Empty
    /// `colorspace` parameter behaves as the unrestricted
    /// [`Self::get_views`] (no rule filter).
    pub fn get_views_for_colorspace(&self, display: &str, colorspace: &str) -> Vec<&View> {
        let mut out = self.get_views(display);
        if colorspace.is_empty() {
            return out;
        }
        out.retain(|v| self.is_view_applicable(v, colorspace));
        out
    }

    /// Adds a shared view.
    pub fn add_shared_view(&mut self, view: View) {
        self.shared_views.push(view);
    }

    /// Removes a shared view by name.
    pub fn remove_shared_view(&mut self, name: &str) -> bool {
        if let Some(index) = self.shared_views.iter().position(|v| v.name() == name) {
            self.shared_views.remove(index);
            return true;
        }
        false
    }

    /// Clears all shared views.
    pub fn clear_shared_views(&mut self) {
        self.shared_views.clear();
    }

    // ========================================================================
    // Virtual display support (OCIO v2)
    // ========================================================================

    /// Returns the virtual display if defined.
    /// Virtual display serves as a template for ICC-profile-instantiated displays.
    /// OCIO ref: Config::hasVirtualDisplay
    pub fn virtual_display(&self) -> Option<&VirtualDisplay> {
        self.virtual_display.as_ref()
    }

    /// Checks if a virtual display is defined.
    /// OCIO ref: Config::hasVirtualDisplay
    pub fn has_virtual_display(&self) -> bool {
        self.virtual_display.is_some()
    }

    /// Returns the number of views in the virtual display.
    /// OCIO ref: Config::getVirtualDisplayNumViews
    pub fn virtual_display_num_views(&self) -> usize {
        self.virtual_display.as_ref().map_or(0, |vd| vd.views.len())
    }

    /// Returns a view from the virtual display by index.
    /// OCIO ref: Config::getVirtualDisplayView
    pub fn virtual_display_view(&self, index: usize) -> Option<&VirtualDisplayView> {
        self.virtual_display
            .as_ref()
            .and_then(|vd| vd.views.get(index))
    }

    /// Sets the virtual display.
    /// OCIO ref: Config::addVirtualDisplayView
    pub fn set_virtual_display(&mut self, vd: VirtualDisplay) {
        self.virtual_display = Some(vd);
    }

    /// Adds a view to the virtual display.
    /// Creates virtual display if not present.
    /// OCIO ref: Config::addVirtualDisplayView
    pub fn add_virtual_display_view(&mut self, view: VirtualDisplayView) {
        if let Some(vd) = &mut self.virtual_display {
            vd.views.push(view);
        } else {
            self.virtual_display = Some(VirtualDisplay {
                views: vec![view],
                shared_view_refs: Vec::new(),
            });
        }
    }

    /// Removes a view from the virtual display by name.
    pub fn remove_virtual_display_view(&mut self, name: &str) -> bool {
        if let Some(vd) = &mut self.virtual_display
            && let Some(index) = vd.views.iter().position(|v| v.name == name)
        {
            vd.views.remove(index);
            if vd.views.is_empty() && vd.shared_view_refs.is_empty() {
                self.virtual_display = None;
            }
            return true;
        }
        false
    }

    /// Adds a shared view reference to the virtual display.
    /// OCIO ref: Config::addVirtualDisplaySharedView
    pub fn add_virtual_display_shared_view(&mut self, name: impl Into<String>) {
        if let Some(vd) = &mut self.virtual_display {
            vd.shared_view_refs.push(name.into());
        } else {
            self.virtual_display = Some(VirtualDisplay {
                views: Vec::new(),
                shared_view_refs: vec![name.into()],
            });
        }
    }

    /// Removes a shared view reference from the virtual display.
    pub fn remove_virtual_display_shared_view(&mut self, name: &str) -> bool {
        if let Some(vd) = &mut self.virtual_display
            && let Some(index) = vd.shared_view_refs.iter().position(|v| v == name)
        {
            vd.shared_view_refs.remove(index);
            if vd.views.is_empty() && vd.shared_view_refs.is_empty() {
                self.virtual_display = None;
            }
            return true;
        }
        false
    }

    /// Clears the virtual display.
    /// OCIO ref: Config::clearVirtualDisplay
    pub fn clear_virtual_display(&mut self) {
        self.virtual_display = None;
    }

    /// Instantiate a display from a monitor name (best-effort).
    /// OCIO ref: Config::instantiateDisplayFromMonitorName.
    pub fn instantiate_display_from_monitor_name(
        &mut self,
        monitor_name: &str,
    ) -> OcioResult<usize> {
        if monitor_name.trim().is_empty() {
            return Err(OcioError::Validation(
                "The system monitor name cannot be null.".into(),
            ));
        }
        let icc_path = resolve_icc_profile_path(monitor_name)?;
        let desc = IccProfile::from_file(&icc_path)?.description();
        self.instantiate_display_internal(Some(monitor_name), &desc, &icc_path)
    }

    /// Instantiate a display from an ICC profile file path.
    /// OCIO ref: Config::instantiateDisplayFromICCProfile.
    pub fn instantiate_display_from_icc_profile(&mut self, icc_path: &str) -> OcioResult<usize> {
        if icc_path.trim().is_empty() {
            return Err(OcioError::Validation(
                "The ICC profile filepath cannot be null.".into(),
            ));
        }
        let path = PathBuf::from(icc_path);
        let desc = IccProfile::from_file(&path)
            .map_err(|e| OcioError::Validation(format!("ICC profile load failed: {}", e)))?
            .description();
        self.instantiate_display_internal(None, &desc, &path)
    }

    fn instantiate_display_internal(
        &mut self,
        monitor_name: Option<&str>,
        monitor_description: &str,
        icc_path: &Path,
    ) -> OcioResult<usize> {
        if monitor_description.trim().is_empty() {
            return Err(OcioError::Validation(
                "The monitor description cannot be null.".into(),
            ));
        }

        if let Ok(val) = env::var("OCIO_ACTIVE_DISPLAYS")
            && !val.trim().is_empty()
        {
            return Err(OcioError::Validation(format!(
                "Cannot instantiate a virtual display because OCIO_ACTIVE_DISPLAYS is set to {}.",
                val
            )));
        }

        let mut color_space_name = monitor_description.to_string();
        if let Some(name) = monitor_name
            && !name.is_empty()
        {
            color_space_name.push_str(" [");
            color_space_name.push_str(name);
            color_space_name.push(']');
        }
        color_space_name = color_space_name.replace(['$', '%'], "_");

        let virtual_display = self.virtual_display.clone().unwrap_or_default();
        if virtual_display.views.is_empty() && virtual_display.shared_view_refs.is_empty() {
            return Err(OcioError::Validation(
                "The virtual display information to instantiate a display is missing.".into(),
            ));
        }

        let display_index = if let Some(display) = self.displays.display_mut(&color_space_name) {
            display.clear_views();
            display.clear_shared_view_refs();
            for v in &virtual_display.views {
                let mut view = View::new(
                    v.name.clone(),
                    if v.display_colorspace.is_empty()
                        || v.display_colorspace == "<USE_DISPLAY_NAME>"
                    {
                        color_space_name.clone()
                    } else {
                        v.display_colorspace.clone()
                    },
                );
                if let Some(looks) = &v.looks {
                    view = view.with_look(looks.clone());
                }
                if let Some(vt) = &v.view_transform {
                    view = view.with_view_transform(vt.clone());
                }
                if let Some(rule) = &v.rule {
                    view = view.with_rule(rule.clone());
                }
                if let Some(desc) = &v.description {
                    view = view.with_description(desc.clone());
                }
                display.add_view(view);
            }
            for sv in &virtual_display.shared_view_refs {
                display.add_shared_view_ref(sv.clone());
            }
            display.set_temporary(true);
            self.displays
                .display_names()
                .position(|d| d.eq_ignore_ascii_case(&color_space_name))
                .unwrap_or(0)
        } else {
            let mut display = Display::new(color_space_name.clone());
            display.set_temporary(true);
            for v in &virtual_display.views {
                let mut view = View::new(
                    v.name.clone(),
                    if v.display_colorspace.is_empty()
                        || v.display_colorspace == "<USE_DISPLAY_NAME>"
                    {
                        color_space_name.clone()
                    } else {
                        v.display_colorspace.clone()
                    },
                );
                if let Some(looks) = &v.looks {
                    view = view.with_look(looks.clone());
                }
                if let Some(vt) = &v.view_transform {
                    view = view.with_view_transform(vt.clone());
                }
                if let Some(rule) = &v.rule {
                    view = view.with_rule(rule.clone());
                }
                if let Some(desc) = &v.description {
                    view = view.with_description(desc.clone());
                }
                display.add_view(view);
            }
            for sv in &virtual_display.shared_view_refs {
                display.add_shared_view_ref(sv.clone());
            }
            let idx = self.displays.displays().len();
            self.displays.add_display(display);
            idx
        };

        let mut cs = ColorSpace::builder(&color_space_name)
            .reference_space_type(ReferenceSpaceType::Display)
            .encoding(Encoding::Sdr)
            .build();
        cs.set_description(format!("Profile description: {}", monitor_description));
        let file_transform = FileTransform {
            src: icc_path.to_path_buf(),
            ccc_id: None,
            cdl_style: CdlStyle::default(),
            interpolation: Interpolation::Best,
            direction: TransformDirection::Forward,
        };
        cs.set_transform(
            ColorSpaceDirection::FromReference,
            Some(Transform::FileTransform(file_transform)),
        );

        if let Err(e) = self.add_colorspace_checked(cs) {
            return Err(OcioError::Validation(format!(
                "Failed to add display color space: {}",
                e
            )));
        }

        if !self.active_displays.is_empty()
            && !self
                .active_displays
                .iter()
                .any(|d| d.eq_ignore_ascii_case(&color_space_name))
        {
            self.active_displays.push(color_space_name.clone());
        }

        if !self.active_views.is_empty()
            && let Some(display) = self.displays.display(&color_space_name)
        {
            for view in display.views() {
                if !self
                    .active_views
                    .iter()
                    .any(|v| v.eq_ignore_ascii_case(view.name()))
                {
                    self.active_views.push(view.name().to_string());
                }
            }
            for sv in display.shared_view_refs() {
                if !self.active_views.iter().any(|v| v.eq_ignore_ascii_case(sv)) {
                    self.active_views.push(sv.clone());
                }
            }
        }

        Ok(display_index)
    }

    /// Gets the number of viewing rules.
    pub fn num_viewing_rules(&self) -> usize {
        self.viewing_rules.len()
    }

    /// Returns viewing rules (OCIO v2.0+).
    pub fn viewing_rules(&self) -> &[ViewingRule] {
        &self.viewing_rules
    }

    /// Sets viewing rules (OCIO v2.0+).
    pub fn set_viewing_rules(&mut self, rules: Vec<ViewingRule>) {
        self.viewing_rules = rules;
    }

    /// Adds a viewing rule.
    pub fn add_viewing_rule(&mut self, rule: ViewingRule) {
        self.viewing_rules.push(rule);
    }

    /// Clears all viewing rules.
    pub fn clear_viewing_rules(&mut self) {
        self.viewing_rules.clear();
    }

    /// Gets a viewing rule by name.
    pub fn viewing_rule(&self, name: &str) -> Option<&ViewingRule> {
        self.viewing_rules.iter().find(|r| r.name == name)
    }

    /// Checks if a view is applicable for a given colorspace based on viewing rules.
    ///
    /// `View` (this parameter's type) and `crate::display::View` are the
    /// SAME type (`config.rs` imports `View` from `crate::display`) — this
    /// used to be a second, independently-diverged copy of
    /// [`Self::is_display_view_applicable`] (case-sensitive, no role-name
    /// resolution, no colorspaces/encodings fallthrough). Delegating rather
    /// than maintaining two copies of the same match against the same type.
    pub fn is_view_applicable(&self, view: &View, colorspace_name: &str) -> bool {
        self.is_display_view_applicable(view, colorspace_name)
    }

    /// Returns filtered shared views applicable for a given colorspace.
    pub fn applicable_views(&self, colorspace_name: &str) -> Vec<&View> {
        self.shared_views
            .iter()
            .filter(|v| self.is_view_applicable(v, colorspace_name))
            .collect()
    }

    /// Checks if a display view is applicable for a given colorspace based on viewing rules.
    ///
    /// If the view has no rule, it's always applicable.
    /// If the view has a rule, checks if the colorspace matches the rule's criteria.
    pub fn is_display_view_applicable(
        &self,
        view: &crate::display::View,
        colorspace_name: &str,
    ) -> bool {
        // No rule means always applicable
        let rule_name = match view.rule() {
            Some(r) => r,
            None => return true,
        };

        // A rule the config does not define matches nothing (`getFilteredViews` includes a
        // view only when `FindRule` finds its rule, `Config.cpp:866-908`).
        let Some(rule) = self.viewing_rule(rule_name) else {
            return false;
        };

        // Find the colorspace
        let cs = match self.colorspace(colorspace_name) {
            Some(cs) => cs,
            None => return true, // Unknown colorspace, assume applicable
        };

        // Check colorspace list match (supports role names). Ported from
        // `Config::Impl::getFilteredViews` (`Config.cpp:846-908`): unlike an
        // earlier version of this function, the colorspaces check does NOT
        // return `false` outright on a miss — the reference falls through to
        // ALSO check `encodings` on the SAME rule (`if (!added &&
        // !viewEncoding.empty())`), so a rule naming both is not mutually
        // exclusive; only when EITHER list matches is the view included.
        let cs_name_lower = colorspace_name.to_lowercase();
        let colorspace_matched = rule.colorspaces.iter().any(|rule_cs| {
            // Rule can use role names, so resolve them first.
            let resolved = self.roles.get(rule_cs).unwrap_or(rule_cs.as_str());
            resolved.to_lowercase() == cs_name_lower
        });
        if colorspace_matched {
            return true;
        }

        // Check encoding match.
        let encoding = cs.encoding();
        if encoding != Encoding::Unknown {
            let enc_str = encoding.as_str().to_lowercase();
            if rule.encodings.iter().any(|e| e.to_lowercase() == enc_str) {
                return true;
            }
        }

        // Neither list matched (including the case where both are empty —
        // ref includes an unconditional view only when it has NO rule at
        // all, handled above; a named rule that matches nothing excludes
        // the view, it does not fall back to "matches everything").
        false
    }

    /// The names of [`Self::get_views_for_colorspace`] (OCIO `getNumViews(display, colorspace)`
    /// and `getView(display, colorspace, index)`).
    pub fn get_display_views_for_colorspace(
        &self,
        display_name: &str,
        colorspace_name: &str,
    ) -> Vec<&str> {
        self.get_views_for_colorspace(display_name, colorspace_name)
            .into_iter()
            .map(|v| v.name())
            .collect()
    }

    /// Returns the number of filtered views for a display applicable to a colorspace.
    pub fn num_display_views_for_colorspace(
        &self,
        display_name: &str,
        colorspace_name: &str,
    ) -> usize {
        self.get_display_views_for_colorspace(display_name, colorspace_name)
            .len()
    }

    /// Gets a view by index from filtered views for a display/colorspace.
    ///
    /// Returns None if index is out of bounds or display doesn't exist.
    pub fn get_display_view_for_colorspace(
        &self,
        display_name: &str,
        colorspace_name: &str,
        index: usize,
    ) -> Option<&str> {
        self.get_display_views_for_colorspace(display_name, colorspace_name)
            .get(index)
            .copied()
    }

    /// Gets the rule name for a display/view combination.
    ///
    /// Returns None if display, view, or rule doesn't exist.
    pub fn get_display_view_rule(&self, display_name: &str, view_name: &str) -> Option<&str> {
        self.displays.display(display_name)?.view(view_name)?.rule()
    }

    // ========================================================================
    // Additional OCIO compatibility methods
    // ========================================================================

    /// Gets the reference color space name.
    ///
    /// Returns the color space assigned to the "reference" role.
    pub fn reference_space(&self) -> Option<&str> {
        self.roles.reference()
    }

    /// Gets the scene-linear color space name.
    ///
    /// Returns the color space assigned to the "scene_linear" role.
    pub fn scene_linear_space(&self) -> Option<&str> {
        self.roles.scene_linear()
    }

    /// Gets the data color space name.
    ///
    /// Returns the color space assigned to the "data" role.
    pub fn data_space(&self) -> Option<&str> {
        self.roles.data()
    }

    /// Resolves a colour space name, alias or role to the colour space's own name, as OCIO's
    /// `getColorSpace(name)->getName()` does (lookup order in [`Self::colorspace`]).
    ///
    /// A role used to answer with the role's target string, which may itself be an alias.
    pub fn resolve_colorspace_name(&self, name: &str) -> Option<&str> {
        self.colorspace(name).map(|cs| cs.name())
    }

    /// Creates a display/view processor using display and view names.
    ///
    /// This is a convenience method that combines source color space,
    /// display, and view into a single processor.
    pub fn create_display_view_processor(
        &self,
        src: &str,
        display: &str,
        view: &str,
    ) -> OcioResult<Processor> {
        self.display_processor(src, display, view)
    }

    /// Returns the inverse direction processor for a color conversion.
    pub fn inverse_processor(&self, src: &str, dst: &str) -> OcioResult<Processor> {
        // Simply swap src and dst
        self.processor(dst, src)
    }

    /// OCIO's `ResolveConfigPath`: `ocio://default` and `ocio://cg-config-latest` become
    /// `ocio://cg-config-v4.0.0_aces-v2.0_ocio-v2.5`, `ocio://studio-config-latest` becomes
    /// `ocio://studio-config-v4.0.0_aces-v2.0_ocio-v2.5` (names compared case-insensitively);
    /// every other path is returned unchanged. Never fails (OCIO's is `noexcept`).
    pub fn resolve_config_path(uri: &str) -> OcioResult<String> {
        Ok(crate::builtin::embedded::resolve_config_path(uri))
    }

    /// Clears the processor cache.
    ///
    /// This should be called if LUT files on disk have changed since
    /// the Config was created.
    ///
    /// OCIO ref: Config::clearProcessorCache().
    ///
    /// Note: In Rust, processors are not cached at the Config level
    /// by default. This method is provided for API compatibility.
    /// Use `ProcessorCache` for explicit caching.
    pub fn clear_processor_cache(&mut self) {
        // Currently a no-op since we don't cache at Config level.
        // ProcessorCache should be used for explicit caching.
        // If we add internal caching later, clear it here.
    }

    /// Checks if this is an OCIO v2 config.
    #[inline]
    pub fn is_v2(&self) -> bool {
        self.version == ConfigVersion::V2
    }

    /// Checks if this is an OCIO v1 config.
    #[inline]
    pub fn is_v1(&self) -> bool {
        self.version == ConfigVersion::V1
    }

    // ========================================================================
    // Builder support
    // ========================================================================

    /// Creates a Config from a validated [`ConfigBuilder`] (internal use).
    #[doc(hidden)]
    pub fn from_builder(builder: crate::config_builder::ConfigBuilder) -> Self {
        let crate::config_builder::ConfigBuilder {
            name,
            description,
            version,
            version_minor,
            search_paths,
            working_dir,
            colorspaces,
            roles,
            displays,
            looks,
            active_displays,
            active_views,
            shared_views,
            viewing_rules,
            named_transforms,
            inactive_colorspaces,
            file_rules,
            environment_vars,
            context,
        } = builder;
        Self {
            name,
            description: sanitize_description(&description),
            version,
            version_minor,
            search_paths,
            working_dir,
            working_dir_is_default: true,
            source: ConfigSource::Raw,
            archive_dir: None,
            colorspaces,
            roles,
            displays,
            looks,
            active_displays,
            active_views,
            shared_views,
            virtual_display: None,
            viewing_rules,
            named_transforms,
            inactive_named_transforms: Vec::new(),
            inactive_colorspaces,
            file_rules,
            environment_vars,
            context,
            strict_parsing: false,
            family_separator: "/".to_string(),
            environment_mode: EnvironmentMode::LoadPredefined,
            default_luma_coefs: [REC709_LUMA_R, REC709_LUMA_G, REC709_LUMA_B],
            default_view_transform: None,
            cache_id: None,
            io_proxy: None,
            processor_cache_flags: 0,
        }
    }

    fn collect_file_references(&self, files: &mut std::collections::BTreeSet<String>) {
        fn collect_from_transform(
            transform: &Transform,
            files: &mut std::collections::BTreeSet<String>,
        ) {
            match transform {
                Transform::Group(t) => {
                    for child in &t.transforms {
                        collect_from_transform(child, files);
                    }
                }
                Transform::FileTransform(t) => {
                    files.insert(t.src.to_string_lossy().to_string());
                }
                _ => {}
            }
        }

        for cs in &self.colorspaces {
            // All four reference-direction accessors so file references
            // hidden inside display-referred colorspaces are picked up
            // for archiving / dependency tracking.
            if let Some(t) = cs.to_reference() {
                collect_from_transform(t, files);
            }
            if let Some(t) = cs.from_reference() {
                collect_from_transform(t, files);
            }
            if let Some(t) = cs.to_display_reference() {
                collect_from_transform(t, files);
            }
            if let Some(t) = cs.from_display_reference() {
                collect_from_transform(t, files);
            }
        }

        for look in self.looks.all() {
            if let Some(t) = look.get_transform() {
                collect_from_transform(t, files);
            }
            if let Some(t) = look.get_inverse_transform() {
                collect_from_transform(t, files);
            }
        }

        for nt in &self.named_transforms {
            if let Some(t) = nt.forward.as_ref() {
                collect_from_transform(t, files);
            }
            if let Some(t) = nt.inverse.as_ref() {
                collect_from_transform(t, files);
            }
        }

        for vt in self.view_transforms() {
            // Walk both scene and display reference pairs — a
            // display-referred view transform stores its body in the
            // `*_display_reference` slots, not the scene ones.
            if let Some(t) = vt.from_scene_reference() {
                collect_from_transform(t, files);
            }
            if let Some(t) = vt.to_scene_reference() {
                collect_from_transform(t, files);
            }
            if let Some(t) = vt.from_display_reference() {
                collect_from_transform(t, files);
            }
            if let Some(t) = vt.to_display_reference() {
                collect_from_transform(t, files);
            }
        }
    }
}

mod parsing;
use parsing::*;
/// `Processor` construction methods (STRUCT.2.next Step 5 split).
mod factory;
/// YAML → `Config` parsing methods (STRUCT.2.next Step 4 split).
mod loading;

mod saving;

/// The env-var value as a list when it is set and not blank, else the config-file list. OCIO
/// reads the variable once, trimmed, and an empty value is no override (`Config.cpp:346-368`,
/// `5371`). `OCIO_INACTIVE_COLORSPACES` is a plain comma list (`Config.cpp:5373`).
fn env_overridden_list(var: &str, fallback: &[String]) -> Vec<String> {
    match std::env::var(var) {
        Ok(s) if !s.trim().is_empty() => s.split(',').map(|p| p.trim().to_string()).collect(),
        _ => fallback.to_vec(),
    }
}

/// [`env_overridden_list`] for `OCIO_ACTIVE_DISPLAYS` / `OCIO_ACTIVE_VIEWS`, which OCIO splits
/// env-style (`,` or `:`, quoted names; `SplitStringEnvStyle`). OCIO fails to create the config
/// on a malformed value; here the value is reported and ignored.
fn env_active_list(var: &str, fallback: &[String]) -> Vec<String> {
    match std::env::var(var) {
        Ok(s) if !s.trim().is_empty() => match crate::parse_utils::split_string_env_style(&s) {
            Ok(list) => list,
            Err(e) => {
                tracing::warn!("{var}: {e} Using the config's list.");
                fallback.to_vec()
            }
        },
        _ => fallback.to_vec(),
    }
}

#[cfg(test)]
mod tests;
