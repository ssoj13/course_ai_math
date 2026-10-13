//! Configuration validation utilities.
//!
//! Provides validation for OCIO configurations to detect common issues:
//! - Missing color space references
//! - Circular dependencies
//! - Invalid transform chains
//! - Missing LUT files
//!
//! # Example
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use vfx_ocio::{Config, validate};
//!
//! let config = Config::from_file("config.ocio")?;
//! let issues = validate::check(&config);
//!
//! for issue in &issues {
//!     println!("{}: {}", issue.severity, issue.message);
//! }
//! # Ok(())
//! # }
//! ```

use crate::config::Config;
use crate::transform::Transform;
use std::collections::HashSet;
use std::path::Path;

/// Severity level for validation issues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Informational message.
    Info,
    /// Warning - config works but may have issues.
    Warning,
    /// Error - config has problems that may cause failures.
    Error,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Info => write!(f, "INFO"),
            Self::Warning => write!(f, "WARN"),
            Self::Error => write!(f, "ERROR"),
        }
    }
}

/// A validation issue found in the config.
#[derive(Debug, Clone)]
pub struct Issue {
    /// Severity level.
    pub severity: Severity,
    /// Issue category.
    pub category: IssueCategory,
    /// Human-readable message.
    pub message: String,
    /// Related element (color space name, role, etc.).
    pub context: Option<String>,
}

/// Categories of validation issues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueCategory {
    /// Missing color space reference.
    MissingColorSpace,
    /// Missing role definition.
    MissingRole,
    /// Missing display/view.
    MissingDisplay,
    /// Missing LUT/transform file.
    MissingFile,
    /// Circular reference in transforms.
    CircularReference,
    /// Invalid transform configuration.
    InvalidTransform,
    /// Unused color space.
    UnusedColorSpace,
    /// Duplicate definition.
    Duplicate,
    /// Missing or malformed look.
    InvalidLook,
    /// Missing or malformed viewing rule.
    InvalidViewingRule,
    /// Missing shared view reference.
    MissingSharedView,
    /// A transform references a color space, look, or named transform that
    /// doesn't exist.
    DanglingTransformReference,
    /// An `environment:` default value is an unresolvable context-variable
    /// expression under predefined-mode.
    InvalidEnvironmentDefault,
    /// A v2.2+ interchange-role requirement is unmet.
    MissingInterchangeRole,
    /// A file rule references a color space, role, or named transform that
    /// doesn't exist.
    InvalidFileRule,
    /// A ViewTransforms requirement is unmet (no scene-referred VT, missing
    /// default VT, or a display-referred color space with no VTs at all).
    InvalidViewTransforms,
    /// The active-displays list (config or env override) names a display
    /// that doesn't exist.
    InvalidActiveDisplays,
    /// An inactive-colorspaces entry names something that's neither a
    /// color space nor a named transform.
    InvalidInactiveEntry,
    /// A `FileTransform` source path, or every configured `search_path`
    /// entry, still contains an unresolved context-variable token after
    /// resolution.
    UnresolvedFileContextVariable,
}

/// Validates a config and returns all issues found.
pub fn check(config: &Config) -> Vec<Issue> {
    let mut issues = Vec::new();

    check_roles(config, &mut issues);
    check_displays(config, &mut issues);
    check_colorspaces(config, &mut issues);
    check_files(config, &mut issues);
    check_looks(config, &mut issues);
    check_viewing_rules(config, &mut issues);
    check_transform_references(config, &mut issues);
    check_environment_defaults(config, &mut issues);
    check_interchange_roles(config, &mut issues);
    check_file_rule_references(config, &mut issues);
    check_view_transforms(config, &mut issues);
    check_active_displays(config, &mut issues);
    check_inactive_entries(config, &mut issues);
    check_file_transform_context_variable_resolution(config, &mut issues);

    issues
}

/// Checks role definitions.
fn check_roles(config: &Config, issues: &mut Vec<Issue>) {
    let required_roles = ["reference", "default"];

    for role in required_roles {
        if config.roles().get(role).is_none() {
            issues.push(Issue {
                severity: Severity::Warning,
                category: IssueCategory::MissingRole,
                message: format!("recommended role '{}' is not defined", role),
                context: Some(role.to_string()),
            });
        }
    }

    // Check that all role targets exist
    for (role, cs_name) in config.roles().iter() {
        if config.colorspace(cs_name).is_none() {
            issues.push(Issue {
                severity: Severity::Error,
                category: IssueCategory::MissingColorSpace,
                message: format!(
                    "role '{}' references non-existent color space '{}'",
                    role, cs_name
                ),
                context: Some(role.to_string()),
            });
        }
    }
}

/// Checks display/view definitions.
fn check_displays(config: &Config, issues: &mut Vec<Issue>) {
    if config.displays().displays().is_empty() {
        issues.push(Issue {
            severity: Severity::Warning,
            category: IssueCategory::MissingDisplay,
            message: "no displays defined".to_string(),
            context: None,
        });
        return;
    }

    for display in config.displays().displays() {
        // OCIO `Config::validate()`: `if(views.empty() && sharedViews.empty())` —
        // a display defined entirely through `views: [<shared views>]` is valid
        // and must NOT be flagged here (fixed: this used to only look at
        // `display.views()`, so any display relying purely on shared views was
        // a false positive).
        if display.views().is_empty() && display.shared_view_refs().is_empty() {
            issues.push(Issue {
                severity: Severity::Warning,
                category: IssueCategory::MissingDisplay,
                message: format!("display '{}' has no views", display.name()),
                context: Some(display.name().to_string()),
            });
        }

        for view in display.views() {
            let cs_name = view.effective_colorspace(display.name());
            if !cs_name.is_empty() && config.resolve_colorspace_name(cs_name).is_none() {
                issues.push(Issue {
                    severity: Severity::Error,
                    category: IssueCategory::MissingColorSpace,
                    message: format!(
                        "view '{}' in display '{}' references non-existent color space '{}'",
                        view.name(),
                        display.name(),
                        cs_name
                    ),
                    context: Some(format!("{}:{}", display.name(), view.name())),
                });
            }
        }

        // Confirm each shared-view reference names a view actually defined in
        // `config.shared_views()` (OCIO `Config::Impl::validateSharedView`).
        for shared_ref in display.shared_view_refs() {
            if !config
                .shared_views()
                .iter()
                .any(|v| v.name().eq_ignore_ascii_case(shared_ref))
            {
                issues.push(Issue {
                    severity: Severity::Error,
                    category: IssueCategory::MissingSharedView,
                    message: format!(
                        "display '{}' references shared view '{}' which is not defined",
                        display.name(),
                        shared_ref
                    ),
                    context: Some(format!("{}:{}", display.name(), shared_ref)),
                });
            }
        }
    }

    // Shared views themselves must target a real color space too (OCIO
    // `Config::Impl::validateView`, called with `isSharedView = true`).
    for view in config.shared_views() {
        // Shared views legitimately use `<USE_DISPLAY_NAME>`
        // (`view.use_display_name()`), which only resolves once hosted by a
        // display — nothing to check against a fixed colorspace name here.
        if !view.use_display_name()
            && !view.colorspace().is_empty()
            && config.resolve_colorspace_name(view.colorspace()).is_none()
        {
            issues.push(Issue {
                severity: Severity::Error,
                category: IssueCategory::MissingColorSpace,
                message: format!(
                    "shared view '{}' references non-existent color space '{}'",
                    view.name(),
                    view.colorspace()
                ),
                context: Some(view.name().to_string()),
            });
        }
    }
}

/// Checks look definitions (OCIO `Config::validate()`'s LOOKS section,
/// `Config.cpp:1880-1930`): every look must be named and its process space
/// must resolve to an existing color space, directly or through a role.
fn check_looks(config: &Config, issues: &mut Vec<Issue>) {
    for look in config.looks().all() {
        if look.name().trim().is_empty() {
            issues.push(Issue {
                severity: Severity::Error,
                category: IssueCategory::InvalidLook,
                message: "a look does not specify a name".to_string(),
                context: None,
            });
            continue;
        }

        match look.get_process_space() {
            None => issues.push(Issue {
                severity: Severity::Error,
                category: IssueCategory::InvalidLook,
                message: format!("look '{}' does not specify a process space", look.name()),
                context: Some(look.name().to_string()),
            }),
            Some(process_space) if process_space.trim().is_empty() => issues.push(Issue {
                severity: Severity::Error,
                category: IssueCategory::InvalidLook,
                message: format!("look '{}' does not specify a process space", look.name()),
                context: Some(look.name().to_string()),
            }),
            Some(process_space) => {
                if config.resolve_colorspace_name(process_space).is_none() {
                    issues.push(Issue {
                        severity: Severity::Error,
                        category: IssueCategory::MissingColorSpace,
                        message: format!(
                            "look '{}' specifies process space '{}', which is not defined",
                            look.name(),
                            process_space
                        ),
                        context: Some(look.name().to_string()),
                    });
                }
            }
        }
    }
}

/// Checks viewing rules (OCIO `ViewingRule::validate`, `ViewingRules.cpp:66-111`):
/// each `colorspaces` entry must resolve (directly or via role); a rule can't
/// mix `colorspaces` and `encodings`; a rule must have at least one of them.
fn check_viewing_rules(config: &Config, issues: &mut Vec<Issue>) {
    for rule in config.viewing_rules() {
        for cs_name in &rule.colorspaces {
            if config.resolve_colorspace_name(cs_name).is_none() {
                issues.push(Issue {
                    severity: Severity::Error,
                    category: IssueCategory::MissingColorSpace,
                    message: format!(
                        "viewing rule '{}' refers to color space '{}', which is not defined",
                        rule.name, cs_name
                    ),
                    context: Some(rule.name.clone()),
                });
            }
        }

        if rule.colorspaces.is_empty() && rule.encodings.is_empty() {
            issues.push(Issue {
                severity: Severity::Error,
                category: IssueCategory::InvalidViewingRule,
                message: format!(
                    "viewing rule '{}' must have either a color space or an encoding",
                    rule.name
                ),
                context: Some(rule.name.clone()),
            });
        } else if !rule.colorspaces.is_empty() && !rule.encodings.is_empty() {
            issues.push(Issue {
                severity: Severity::Error,
                category: IssueCategory::InvalidViewingRule,
                message: format!(
                    "viewing rule '{}' cannot refer to both a color space and an encoding",
                    rule.name
                ),
                context: Some(rule.name.clone()),
            });
        }
    }
}

/// Predefined-context-variable-mode check (OCIO `Config::validate()`,
/// `Config.cpp:1380-1415`). Only applies to v2+ configs whose `Context`
/// environment mode is `LoadPredefined`. Each `environment:` entry's default
/// value, if it itself contains a context-variable token, must be EXACTLY
/// one of `$NAME`/`${NAME}`/`%NAME%` for its OWN name (`ENV = $ENV` is the
/// only legal self-reference — "no default value, an OS env var must
/// exist"); anything else containing `$`/`%` is an unresolved reference to
/// something else, which predefined mode can't fall back to resolving.
fn check_environment_defaults(config: &Config, issues: &mut Vec<Issue>) {
    if config.major_version() < 2
        || config.environment_mode() != crate::EnvironmentMode::LoadPredefined
    {
        return;
    }

    for i in 0..config.num_environment_vars() {
        let (Some(name), Some(value)) = (
            config.environment_var_name_at(i),
            config.environment_var_value_at(i),
        ) else {
            continue;
        };

        if !contains_context_variables(value) {
            continue;
        }

        let self_ref_dollar = format!("${name}");
        let self_ref_braced = format!("${{{name}}}");
        let self_ref_percent = format!("%{name}%");
        let is_valid =
            value == self_ref_dollar || value == self_ref_braced || value == self_ref_percent;

        if !is_valid {
            issues.push(Issue {
                severity: Severity::Error,
                category: IssueCategory::InvalidEnvironmentDefault,
                message: format!(
                    "unresolved context variable in environment declaration '{name} = {value}'"
                ),
                context: Some(name.to_string()),
            });
        }
    }
}

/// Mirrors OCIO's `ContainsContextVariables` (`ContextVariableUtils.cpp:56-76`):
/// a bare `$` anywhere counts; a `%` only counts if there's a SECOND,
/// distinct `%` later in the string (i.e. `%NAME%`, not a lone `%`).
fn contains_context_variables(s: &str) -> bool {
    if s.contains('$') {
        return true;
    }
    if let Some(first) = s.find('%')
        && let Some(last) = s.rfind('%')
        && first != last
    {
        return true;
    }
    false
}

/// v2.2+ interchange-role requirements (OCIO `Config::validate()`,
/// `Config.cpp:1529-1627`): `scene_linear`/`compositing_log`/`color_timing`
/// roles are required; `aces_interchange` is required when the config has
/// any scene-referred color space (and must itself point at one);
/// `cie_xyz_d65_interchange` is required when it has any display-referred
/// color space (and must itself point at one). OCIO logs these rather than
/// throwing ("to make it possible for upgradeToLatestVersion to always
/// result in a config that does not fail validation") — matched here with
/// `Severity::Warning` rather than `Error`, the same non-fatal treatment.
fn check_interchange_roles(config: &Config, issues: &mut Vec<Issue>) {
    let version_ge_2_2 =
        config.major_version() > 2 || (config.major_version() == 2 && config.minor_version() >= 2);
    if !version_ge_2_2 {
        return;
    }

    let has_scene_referred = config
        .colorspaces()
        .iter()
        .any(|cs| cs.reference_space_type() == crate::display::ReferenceSpaceType::Scene);
    let has_display_referred = config
        .colorspaces()
        .iter()
        .any(|cs| cs.reference_space_type() == crate::display::ReferenceSpaceType::Display);

    let warn = |issues: &mut Vec<Issue>, message: String| {
        issues.push(Issue {
            severity: Severity::Warning,
            category: IssueCategory::MissingInterchangeRole,
            message,
            context: None,
        });
    };

    for (role, label) in [
        ("scene_linear", "scene_linear"),
        ("compositing_log", "compositing_log"),
        ("color_timing", "color_timing"),
    ] {
        if config.roles().get(role).is_none() {
            warn(
                issues,
                format!("the {label} role is required for a config version 2.2 or higher"),
            );
        }
    }

    match config.roles().get("aces_interchange") {
        None if has_scene_referred => warn(
            issues,
            "the aces_interchange role is required when there are scene-referred color \
             spaces and the config version is 2.2 or higher"
                .to_string(),
        ),
        Some(cs_name) => {
            let is_scene = config
                .colorspace(cs_name)
                .map(|cs| cs.reference_space_type() == crate::display::ReferenceSpaceType::Scene)
                .unwrap_or(false);
            if !is_scene {
                warn(
                    issues,
                    "the aces_interchange role must be a scene-referred color space".to_string(),
                );
            }
        }
        None => {}
    }

    match config.roles().get("cie_xyz_d65_interchange") {
        None if has_display_referred => warn(
            issues,
            "the cie_xyz_d65_interchange role is required when there are display-referred \
             color spaces and the config version is 2.2 or higher"
                .to_string(),
        ),
        Some(cs_name) => {
            let is_display = config
                .colorspace(cs_name)
                .map(|cs| cs.reference_space_type() == crate::display::ReferenceSpaceType::Display)
                .unwrap_or(false);
            if !is_display {
                warn(
                    issues,
                    "the cie_xyz_d65_interchange role must be a display-referred color space"
                        .to_string(),
                );
            }
        }
        None => {}
    }
}

/// FileRules validate equivalent (OCIO `FileRules::Impl::validate`,
/// `FileRules.cpp:665-685` + each `Rule::validate`, `:506-523`): every rule
/// except `ColorSpaceNamePathSearch` must name a color space, role, or
/// named transform that exists. Skipped for v1 configs with 2 or fewer
/// rules (the two OCIO auto-creates — `Default` + `ColorSpaceNamePathSearch`
/// — which a v1 config isn't required to define), matching the reference's
/// own carve-out so this doesn't spuriously fail v1 configs.
fn check_file_rule_references(config: &Config, issues: &mut Vec<Issue>) {
    let rules = config.file_rules();
    if config.major_version() == 1 && rules.len() <= 2 {
        return;
    }

    for rule in rules {
        if matches!(rule.kind, crate::config::types::FileRuleKind::PathSearch) {
            continue;
        }
        if config.resolve_colorspace_name(&rule.colorspace).is_none()
            && config.named_transform(&rule.colorspace).is_none()
        {
            issues.push(Issue {
                severity: Severity::Error,
                category: IssueCategory::InvalidFileRule,
                message: format!(
                    "file rule '{}' references '{}', which is neither a color space nor a \
                     named transform",
                    rule.name, rule.colorspace
                ),
                context: Some(rule.name.clone()),
            });
        }
    }
}

/// ViewTransforms requirements (OCIO `Config::validate()`, `Config.cpp:1932-1967`):
/// if any view transforms are defined, at least one must be scene-referred;
/// if none are defined but a display-referred color space exists, that's an
/// error (nothing could ever reach it); if a default view transform name is
/// set, it must name an existing SCENE-referred view transform.
fn check_view_transforms(config: &Config, issues: &mut Vec<Issue>) {
    let has_scene_vt = config
        .view_transforms()
        .iter()
        .any(|vt| vt.reference_space_type() == crate::display::ReferenceSpaceType::Scene);

    if !config.view_transforms().is_empty() {
        if !has_scene_vt {
            issues.push(Issue {
                severity: Severity::Error,
                category: IssueCategory::InvalidViewTransforms,
                message: "if there are view_transforms, at least one must use the scene \
                          reference space"
                    .to_string(),
                context: None,
            });
        }
    } else {
        let has_display_referred = config
            .colorspaces()
            .iter()
            .any(|cs| cs.reference_space_type() == crate::display::ReferenceSpaceType::Display);
        if has_display_referred {
            issues.push(Issue {
                severity: Severity::Error,
                category: IssueCategory::InvalidViewTransforms,
                message: "if there are display-referred color spaces, there must be \
                          view_transforms"
                    .to_string(),
                context: None,
            });
        }
    }

    if let Some(default_name) = config.default_view_transform() {
        let matches = config.view_transforms().iter().any(|vt| {
            vt.name().eq_ignore_ascii_case(default_name)
                && vt.reference_space_type() == crate::display::ReferenceSpaceType::Scene
        });
        if !matches {
            issues.push(Issue {
                severity: Severity::Error,
                category: IssueCategory::InvalidViewTransforms,
                message: format!(
                    "default view transform is defined as '{default_name}' but this does not \
                     correspond to an existing scene-referred view transform"
                ),
                context: Some(default_name.to_string()),
            });
        }
    }
}

/// Active-displays validity (OCIO `Config::validate()`, `Config.cpp:1740-1809`):
/// both the config-file `active_displays` list and the `OCIO_ACTIVE_DISPLAYS`
/// env override, if set, must name only displays that actually exist. A
/// single empty-string entry is OCIO's "use all displays" sentinel, not a
/// name to validate.
fn check_active_displays(config: &Config, issues: &mut Vec<Issue>) {
    let display_names: Vec<&str> = config
        .displays()
        .displays()
        .iter()
        .map(|d| d.name())
        .collect();

    let check_list = |source: &str, list: &[String], issues: &mut Vec<Issue>| {
        if list.is_empty() || (list.len() == 1 && list[0].is_empty()) {
            return;
        }
        let matched = list
            .iter()
            .filter(|name| display_names.iter().any(|d| d.eq_ignore_ascii_case(name)))
            .count();
        if matched == 0 {
            issues.push(Issue {
                severity: Severity::Error,
                category: IssueCategory::InvalidActiveDisplays,
                message: format!("the list of active displays {list:?} from {source} is invalid"),
                context: None,
            });
        } else if matched != list.len() {
            issues.push(Issue {
                severity: Severity::Error,
                category: IssueCategory::InvalidActiveDisplays,
                message: format!(
                    "the list of active displays {list:?} from {source} contains invalid \
                     display name(s)"
                ),
                context: None,
            });
        }
    };

    check_list("the config file", config.active_displays(), issues);
    if let Ok(env_val) = std::env::var("OCIO_ACTIVE_DISPLAYS")
        && !env_val.trim().is_empty()
    {
        match crate::parse_utils::split_string_env_style(&env_val) {
            Ok(env_list) => check_list("the OCIO_ACTIVE_DISPLAYS env var", &env_list, issues),
            Err(e) => issues.push(Issue {
                severity: Severity::Error,
                category: IssueCategory::InvalidActiveDisplays,
                message: e,
                context: None,
            }),
        }
    }
}

/// Confirms all inactive color spaces or named transforms exist (OCIO
/// `Config::validate()`, `Config.cpp:1630-1645`): builds the effective
/// inactive-names list (env override honored, matching the reference's
/// `buildInactiveNamesList(INACTIVE_ALL)`) and checks each entry against
/// both color spaces and named transforms. OCIO itself only `LogInfo`s this
/// rather than throwing, so this is `Severity::Info`, not `Error` — same
/// non-fatal treatment.
fn check_inactive_entries(config: &Config, issues: &mut Vec<Issue>) {
    for name in config.effective_inactive_colorspaces() {
        if config.resolve_colorspace_name(&name).is_none()
            && config.named_transform(&name).is_none()
        {
            issues.push(Issue {
                severity: Severity::Info,
                category: IssueCategory::InvalidInactiveEntry,
                message: format!(
                    "inactive '{name}' is neither a color space nor a named transform"
                ),
                context: Some(name),
            });
        }
    }
}

/// Context-variable file-resolution completeness (OCIO `Config::validate()`,
/// `Config.cpp:1984-2074`): collects every `FileTransform::src` referenced
/// by an internal transform (`GetFileReferences`, `Config.cpp:155-172` —
/// recurses `GroupTransform`, same shape as [`walk_transform_refs`] but
/// collecting paths, not color space/look names). If any exist: at least
/// one configured `search_path` entry must resolve (via [`crate::Context::resolve`])
/// to something with no `$`/`%` remaining — a config with FileTransforms
/// but an empty or entirely-unresolvable search path can never load them.
/// Then each individual file path must itself resolve cleanly too (a
/// resolved search path doesn't guarantee every individual file's own
/// context-variable references resolve).
fn check_file_transform_context_variable_resolution(config: &Config, issues: &mut Vec<Issue>) {
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    let mut collect = |t: &Transform| collect_file_transform_paths(t, &mut files);

    for cs in config.colorspaces() {
        for transform in [
            cs.to_reference(),
            cs.from_reference(),
            cs.to_display_reference(),
            cs.from_display_reference(),
        ]
        .into_iter()
        .flatten()
        {
            collect(transform);
        }
    }
    for look in config.looks().all() {
        if let Some(t) = look.get_transform() {
            collect(t);
        }
        if let Some(t) = look.get_inverse_transform() {
            collect(t);
        }
    }
    for nt in config.named_transforms() {
        if let Some(t) = &nt.forward {
            collect(t);
        }
        if let Some(t) = &nt.inverse {
            collect(t);
        }
    }

    if files.is_empty() {
        return;
    }

    let ctx = config.context();
    let num_paths = ctx.num_search_paths();
    let found_one = (0..num_paths).any(|i| {
        let Some(p) = ctx.search_path_at(i) else {
            return false;
        };
        if p.is_empty() {
            return false;
        }
        let resolved = ctx.resolve(p);
        !resolved.is_empty() && !resolved.contains('$') && !resolved.contains('%')
    });

    if !found_one {
        let message = if num_paths == 0 {
            "the search_path must not be empty if there are FileTransforms".to_string()
        } else {
            "no configured search_path entry could be resolved for the FileTransforms present"
                .to_string()
        };
        issues.push(Issue {
            severity: Severity::Error,
            category: IssueCategory::UnresolvedFileContextVariable,
            message,
            context: None,
        });
    }

    for file in &files {
        let file_str = file.to_string_lossy();
        let resolved = ctx.resolve(&file_str);
        if resolved.is_empty() || resolved.contains('$') || resolved.contains('%') {
            issues.push(Issue {
                severity: Severity::Error,
                category: IssueCategory::UnresolvedFileContextVariable,
                message: format!(
                    "the file transform source cannot be resolved: '{file_str}' (resolves to \
                     '{resolved}')"
                ),
                context: Some(file_str.to_string()),
            });
        }
    }
}

/// Recurses through `t` (descending into `GroupTransform`) collecting every
/// `FileTransform::src` it carries. Mirrors OCIO's `GetFileReferences`
/// (`Config.cpp:155-172`).
fn collect_file_transform_paths(t: &Transform, files: &mut Vec<std::path::PathBuf>) {
    match t {
        Transform::Group(g) => {
            for sub in &g.transforms {
                collect_file_transform_paths(sub, files);
            }
        }
        Transform::FileTransform(ft) => {
            files.push(ft.src.clone());
        }
        _ => {}
    }
}

/// The generic "every transform's colorspace/look/named-transform reference
/// resolves" sweep (the highest-value remaining `validate()` gap):
/// OCIO's own `Config::validate()` TRANSFORMS section (`Config.cpp:1814-1878`)
/// walks `getAllInternalTransforms()` (every colorspace's to/from-reference
/// transform, every look's forward/inverse transform, every named
/// transform's forward/inverse) through `GetColorSpaceReferences`
/// (`Config.cpp:177-208`, which only checks `ColorSpaceTransform`/
/// `DisplayViewTransform`/`LookTransform` src/dst — OCIO does NOT chase
/// look-name strings there). This crate extends that scope to also check
/// the look names embedded in `LookTransform::looks` and
/// `DisplayViewTransform::looks_override` resolve to defined looks, since
/// dangling look references are exactly the kind of silent-wrong-output bug
/// this whole bughunt pass has been hunting (`RunLookTokens`'s
/// inverse-resolution bugs were found in this same area).
///
/// Context-variable-aware: a name is resolved through [`crate::Context::resolve`]
/// first (matching OCIO's `context->resolveStringVar`), and a name that
/// still contains `$`/`%` tokens after resolution is reported as an unknown
/// context variable rather than a missing color space (OCIO:
/// `Config.cpp:1840-1849`, "references ... using an unknown context
/// variable").
fn check_transform_references(config: &Config, issues: &mut Vec<Issue>) {
    for cs in config.colorspaces() {
        for (transform, slot) in [
            (cs.to_reference(), "to_reference"),
            (cs.from_reference(), "from_reference"),
            (cs.to_display_reference(), "to_display_reference"),
            (cs.from_display_reference(), "from_display_reference"),
        ] {
            if let Some(t) = transform {
                let ctx = format!("color space '{}'.{}", cs.name(), slot);
                walk_transform_refs(t, config, &ctx, issues);
            }
        }
    }

    for look in config.looks().all() {
        if let Some(t) = look.get_transform() {
            walk_transform_refs(
                t,
                config,
                &format!("look '{}'.transform", look.name()),
                issues,
            );
        }
        if let Some(t) = look.get_inverse_transform() {
            walk_transform_refs(
                t,
                config,
                &format!("look '{}'.inverse_transform", look.name()),
                issues,
            );
        }
    }

    for nt in config.named_transforms() {
        if let Some(t) = &nt.forward {
            walk_transform_refs(
                t,
                config,
                &format!("named transform '{}'.forward", nt.name),
                issues,
            );
        }
        if let Some(t) = &nt.inverse {
            walk_transform_refs(
                t,
                config,
                &format!("named transform '{}'.inverse", nt.name),
                issues,
            );
        }
    }
}

/// Recurses through `t` (descending into `GroupTransform`) collecting and
/// checking every color space / look reference it carries.
fn walk_transform_refs(t: &Transform, config: &Config, ctx: &str, issues: &mut Vec<Issue>) {
    match t {
        Transform::Group(g) => {
            for sub in &g.transforms {
                walk_transform_refs(sub, config, ctx, issues);
            }
        }
        Transform::ColorSpace(cst) => {
            check_colorspace_ref(&cst.src, config, ctx, issues);
            check_colorspace_ref(&cst.dst, config, ctx, issues);
        }
        Transform::Look(lt) => {
            check_colorspace_ref(&lt.src, config, ctx, issues);
            check_colorspace_ref(&lt.dst, config, ctx, issues);
            for (name, _forward) in crate::look::parse_looks(&lt.looks) {
                check_look_ref(name, config, ctx, issues);
            }
        }
        Transform::DisplayView(dvt) => {
            check_colorspace_ref(&dvt.src, config, ctx, issues);
            if dvt.looks_override_enabled {
                for (name, _forward) in crate::look::parse_looks(&dvt.looks_override) {
                    check_look_ref(name, config, ctx, issues);
                }
            }
        }
        _ => {}
    }
}

/// Checks one color-space-or-role-or-named-transform reference, matching
/// OCIO's own fallback order (`Config.cpp:1832-1876`): color space, then
/// role, then named transform; only an empty name skips the check
/// entirely (an unset `src`/`dst` is a separate, structural problem this
/// pass doesn't attempt to also flag here).
fn check_colorspace_ref(name: &str, config: &Config, ctx: &str, issues: &mut Vec<Issue>) {
    if name.is_empty() {
        return;
    }
    let resolved = config.context().resolve(name);
    if resolved.contains('$') || resolved.contains('%') {
        issues.push(Issue {
            severity: Severity::Error,
            category: IssueCategory::DanglingTransformReference,
            message: format!(
                "{ctx} references color space '{name}' using an unknown context variable \
                 (resolves to '{resolved}')"
            ),
            context: Some(ctx.to_string()),
        });
        return;
    }
    if config.resolve_colorspace_name(&resolved).is_none()
        && config.named_transform(&resolved).is_none()
    {
        issues.push(Issue {
            severity: Severity::Error,
            category: IssueCategory::DanglingTransformReference,
            message: format!("{ctx} references '{resolved}', which is not defined"),
            context: Some(ctx.to_string()),
        });
    }
}

/// Checks one look-name reference (from a `LookTransform::looks` /
/// `DisplayViewTransform::looks_override` list) resolves to a defined look.
fn check_look_ref(name: &str, config: &Config, ctx: &str, issues: &mut Vec<Issue>) {
    if name.is_empty() {
        return;
    }
    let resolved = config.context().resolve(name);
    if config.looks().get(&resolved).is_none() {
        issues.push(Issue {
            severity: Severity::Error,
            category: IssueCategory::DanglingTransformReference,
            message: format!("{ctx} references look '{resolved}', which is not defined"),
            context: Some(ctx.to_string()),
        });
    }
}

/// Checks color space definitions.
fn check_colorspaces(config: &Config, issues: &mut Vec<Issue>) {
    let mut names: HashSet<&str> = HashSet::new();

    for cs in config.colorspaces() {
        // Check for duplicates
        if !names.insert(cs.name()) {
            issues.push(Issue {
                severity: Severity::Error,
                category: IssueCategory::Duplicate,
                message: format!("duplicate color space name: '{}'", cs.name()),
                context: Some(cs.name().to_string()),
            });
        }

        // Check aliases don't conflict
        for alias in cs.aliases() {
            if names.contains(alias.as_str()) {
                issues.push(Issue {
                    severity: Severity::Warning,
                    category: IssueCategory::Duplicate,
                    message: format!(
                        "alias '{}' for '{}' conflicts with existing name",
                        alias,
                        cs.name()
                    ),
                    context: Some(cs.name().to_string()),
                });
            }
        }

        // Check data color spaces don't have transforms — covers
        // every reference-direction accessor (scene + display).
        if cs.is_data()
            && (cs.to_reference().is_some()
                || cs.from_reference().is_some()
                || cs.to_display_reference().is_some()
                || cs.from_display_reference().is_some())
        {
            issues.push(Issue {
                severity: Severity::Warning,
                category: IssueCategory::InvalidTransform,
                message: format!(
                    "data color space '{}' has transforms defined (will be ignored)",
                    cs.name()
                ),
                context: Some(cs.name().to_string()),
            });
        }
    }
}

/// Checks for missing transform files.
fn check_files(config: &Config, issues: &mut Vec<Issue>) {
    // Check search paths exist
    for path in config.search_paths() {
        if !path.exists() {
            issues.push(Issue {
                severity: Severity::Warning,
                category: IssueCategory::MissingFile,
                message: format!("search path does not exist: {}", path.display()),
                context: Some(path.display().to_string()),
            });
        }
    }

    // Check FileTransform LUT files exist
    let search_paths = config.search_paths();
    let proxy = config.config_io_proxy();
    for cs in config.colorspaces() {
        // Validate FileTransforms in every reference-direction slot.
        // Display-referred colorspaces (sRGB - Display, …) hold
        // their bodies in the `*_display_reference` accessors, and
        // skipping them would let broken LUT paths through unnoticed.
        if let Some(transform) = cs.to_reference() {
            check_transform_files(
                transform,
                search_paths,
                cs.name(),
                "to_reference",
                proxy.clone(),
                issues,
            );
        }
        if let Some(transform) = cs.from_reference() {
            check_transform_files(
                transform,
                search_paths,
                cs.name(),
                "from_reference",
                proxy.clone(),
                issues,
            );
        }
        if let Some(transform) = cs.to_display_reference() {
            check_transform_files(
                transform,
                search_paths,
                cs.name(),
                "to_display_reference",
                proxy.clone(),
                issues,
            );
        }
        if let Some(transform) = cs.from_display_reference() {
            check_transform_files(
                transform,
                search_paths,
                cs.name(),
                "from_display_reference",
                proxy.clone(),
                issues,
            );
        }
    }
}

/// Recursively checks transform tree for FileTransform paths.
fn check_transform_files(
    transform: &Transform,
    search_paths: &[std::path::PathBuf],
    cs_name: &str,
    direction: &str,
    proxy: Option<std::sync::Arc<dyn crate::ConfigIOProxy>>,
    issues: &mut Vec<Issue>,
) {
    match transform {
        Transform::FileTransform(ft) => {
            check_file_exists(&ft.src, search_paths, cs_name, direction, proxy, issues);
        }
        Transform::Group(g) => {
            for t in &g.transforms {
                check_transform_files(t, search_paths, cs_name, direction, proxy.clone(), issues);
            }
        }
        // Other transform types don't reference external files
        _ => {}
    }
}

/// Checks if a file exists in any search path.
fn check_file_exists(
    src: &Path,
    search_paths: &[std::path::PathBuf],
    cs_name: &str,
    direction: &str,
    proxy: Option<std::sync::Arc<dyn crate::ConfigIOProxy>>,
    issues: &mut Vec<Issue>,
) {
    if let Some(proxy) = proxy
        && proxy.get_lut_data(src.to_string_lossy().as_ref()).is_some()
    {
        return;
    }

    // If absolute path, check directly
    if src.is_absolute() {
        if !src.exists() {
            issues.push(Issue {
                severity: Severity::Error,
                category: IssueCategory::MissingFile,
                message: format!(
                    "LUT file not found: {} (in {}.{})",
                    src.display(),
                    cs_name,
                    direction
                ),
                context: Some(format!("{}:{}", cs_name, src.display())),
            });
        }
        return;
    }

    // Check in search paths
    for base in search_paths {
        if base.join(src).exists() {
            return; // Found
        }
    }

    // Not found in any search path
    issues.push(Issue {
        severity: Severity::Error,
        category: IssueCategory::MissingFile,
        message: format!(
            "LUT file not found in search paths: {} (in {}.{})",
            src.display(),
            cs_name,
            direction
        ),
        context: Some(format!("{}:{}", cs_name, src.display())),
    });
}

/// Returns true if there are any errors.
pub fn has_errors(issues: &[Issue]) -> bool {
    issues.iter().any(|i| i.severity == Severity::Error)
}

/// Returns true if there are any warnings or errors.
pub fn has_warnings(issues: &[Issue]) -> bool {
    issues
        .iter()
        .any(|i| i.severity == Severity::Warning || i.severity == Severity::Error)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Predefined-mode environment defaults: `ENV = $ENV` (self-reference)
    /// is the only legal case; anything else containing a context-variable
    /// token is unresolved (OCIO `Config.cpp:1380-1415`).
    #[test]
    fn environment_default_self_reference_is_not_flagged() {
        let mut config = Config::new();
        config.set_version(2, 0);
        config.set_environment_mode(crate::EnvironmentMode::LoadPredefined);
        config.add_environment_var("SHOW", "$SHOW");

        let issues = check(&config);
        assert!(
            !issues
                .iter()
                .any(|i| i.category == IssueCategory::InvalidEnvironmentDefault),
            "ENV = $ENV self-reference was wrongly flagged: {issues:?}"
        );
    }

    #[test]
    fn environment_default_referencing_another_var_is_flagged() {
        let mut config = Config::new();
        config.set_version(2, 0);
        config.set_environment_mode(crate::EnvironmentMode::LoadPredefined);
        config.add_environment_var("SHOW", "$OTHER_VAR");

        let issues = check(&config);
        assert!(
            issues
                .iter()
                .any(|i| i.category == IssueCategory::InvalidEnvironmentDefault),
            "expected an InvalidEnvironmentDefault issue, got: {issues:?}"
        );
    }

    #[test]
    fn environment_default_is_not_checked_under_load_all_mode() {
        // The predefined-mode check only applies when the environment mode
        // is LoadPredefined (OCIO: "Only the 'predefined' mode imposes to
        // have all the context variables explicitly defined").
        let mut config = Config::new();
        config.set_version(2, 0);
        config.set_environment_mode(crate::EnvironmentMode::LoadAll);
        config.add_environment_var("SHOW", "$OTHER_VAR");

        let issues = check(&config);
        assert!(
            !issues
                .iter()
                .any(|i| i.category == IssueCategory::InvalidEnvironmentDefault),
            "LoadAll mode must not be checked: {issues:?}"
        );
    }

    #[test]
    fn interchange_roles_missing_on_v2_2_config_are_flagged() {
        let mut config = Config::new();
        config.set_version(2, 2);

        let issues = check(&config);
        for role in ["scene_linear", "compositing_log", "color_timing"] {
            assert!(
                issues
                    .iter()
                    .any(|i| i.category == IssueCategory::MissingInterchangeRole
                        && i.message.contains(role)),
                "expected a MissingInterchangeRole issue for '{role}', got: {issues:?}"
            );
        }
    }

    #[test]
    fn interchange_roles_not_required_below_v2_2() {
        let mut config = Config::new();
        config.set_version(2, 0);

        let issues = check(&config);
        assert!(
            !issues
                .iter()
                .any(|i| i.category == IssueCategory::MissingInterchangeRole),
            "v2.0 must not require v2.2 interchange roles: {issues:?}"
        );
    }

    #[test]
    fn file_rule_referencing_undefined_colorspace_is_flagged() {
        use crate::config::types::{FileRule, FileRuleKind};

        let mut config = Config::new();
        config.set_version(2, 0);
        config.add_file_rule(FileRule {
            name: "MyRule".into(),
            colorspace: "NoSuchSpace".into(),
            kind: FileRuleKind::Basic {
                pattern: "*".into(),
                extension: Some("exr".into()),
            },
            custom_keys: vec![],
        });

        let issues = check(&config);
        assert!(
            issues
                .iter()
                .any(|i| i.category == IssueCategory::InvalidFileRule
                    && i.message.contains("MyRule")),
            "expected an InvalidFileRule issue naming MyRule, got: {issues:?}"
        );
    }

    #[test]
    fn view_transforms_present_but_none_scene_referred_is_flagged() {
        use crate::display::{ReferenceSpaceType, ViewTransform};

        let mut config = Config::new();
        config.add_view_transform(ViewTransform::with_reference_space(
            "Display VT",
            ReferenceSpaceType::Display,
        ));

        let issues = check(&config);
        assert!(
            issues
                .iter()
                .any(|i| i.category == IssueCategory::InvalidViewTransforms),
            "expected an InvalidViewTransforms issue (no scene-referred VT), got: {issues:?}"
        );
    }

    #[test]
    fn display_referred_colorspace_without_any_view_transforms_is_flagged() {
        use crate::colorspace::ColorSpace;
        use crate::display::ReferenceSpaceType;

        let mut config = Config::new();
        config.add_colorspace(
            ColorSpace::builder("Display Space")
                .reference_space_type(ReferenceSpaceType::Display)
                .build(),
        );

        let issues = check(&config);
        assert!(
            issues
                .iter()
                .any(|i| i.category == IssueCategory::InvalidViewTransforms),
            "expected an InvalidViewTransforms issue (display-referred CS, no VTs), got: {issues:?}"
        );
    }

    #[test]
    fn default_view_transform_naming_an_undefined_vt_is_flagged() {
        use crate::display::{ReferenceSpaceType, ViewTransform};

        let mut config = Config::new();
        config.add_view_transform(ViewTransform::with_reference_space(
            "Real VT",
            ReferenceSpaceType::Scene,
        ));
        config.set_default_view_transform("NoSuchVT");

        let issues = check(&config);
        assert!(
            issues
                .iter()
                .any(|i| i.category == IssueCategory::InvalidViewTransforms
                    && i.message.contains("NoSuchVT")),
            "expected an InvalidViewTransforms issue naming NoSuchVT, got: {issues:?}"
        );
    }

    #[test]
    fn active_displays_naming_an_undefined_display_is_flagged() {
        let mut config = Config::new();
        config.add_display(crate::Display::new("Real Display"));
        config.add_active_display("Ghost Display");

        let issues = check(&config);
        assert!(
            issues
                .iter()
                .any(|i| i.category == IssueCategory::InvalidActiveDisplays),
            "expected an InvalidActiveDisplays issue, got: {issues:?}"
        );
    }

    #[test]
    fn inactive_entry_naming_neither_colorspace_nor_named_transform_is_flagged() {
        let mut config = Config::new();
        config.set_inactive_colorspaces(vec!["GhostSpace".into()]);

        let issues = check(&config);
        assert!(
            issues
                .iter()
                .any(|i| i.category == IssueCategory::InvalidInactiveEntry
                    && i.message.contains("GhostSpace")),
            "expected an InvalidInactiveEntry issue naming GhostSpace, got: {issues:?}"
        );
    }

    #[test]
    fn inactive_entry_naming_a_real_colorspace_is_not_flagged() {
        use crate::colorspace::ColorSpace;

        let mut config = Config::new();
        config.add_colorspace(ColorSpace::new("sRGB"));
        config.set_inactive_colorspaces(vec!["sRGB".into()]);

        let issues = check(&config);
        assert!(
            !issues
                .iter()
                .any(|i| i.category == IssueCategory::InvalidInactiveEntry),
            "a real color space was wrongly flagged as an invalid inactive entry: {issues:?}"
        );
    }

    #[test]
    fn active_displays_naming_a_real_display_is_not_flagged() {
        let mut config = Config::new();
        config.add_display(crate::Display::new("Real Display"));
        config.add_active_display("Real Display");

        let issues = check(&config);
        assert!(
            !issues
                .iter()
                .any(|i| i.category == IssueCategory::InvalidActiveDisplays),
            "a real display name was wrongly flagged: {issues:?}"
        );
    }

    #[test]
    fn check_empty_config() {
        let config = Config::new();
        let issues = check(&config);

        // Should warn about missing roles and displays
        assert!(
            issues
                .iter()
                .any(|i| i.category == IssueCategory::MissingRole)
        );
        assert!(
            issues
                .iter()
                .any(|i| i.category == IssueCategory::MissingDisplay)
        );
    }

    #[test]
    fn severity_display() {
        assert_eq!(format!("{}", Severity::Info), "INFO");
        assert_eq!(format!("{}", Severity::Warning), "WARN");
        assert_eq!(format!("{}", Severity::Error), "ERROR");
    }

    #[test]
    fn has_errors_check() {
        let issues = vec![Issue {
            severity: Severity::Warning,
            category: IssueCategory::MissingRole,
            message: "test".into(),
            context: None,
        }];
        assert!(!has_errors(&issues));
        assert!(has_warnings(&issues));

        let issues_with_error = vec![Issue {
            severity: Severity::Error,
            category: IssueCategory::MissingColorSpace,
            message: "test".into(),
            context: None,
        }];
        assert!(has_errors(&issues_with_error));
    }

    /// A display defined purely through a shared-view reference (no own
    /// `views`) must NOT be flagged as having no views — this was a false
    /// positive before `check_displays` started looking at
    /// `shared_view_refs()` too (OCIO `Config::validate()`:
    /// `if(views.empty() && sharedViews.empty())`).
    #[test]
    fn display_with_only_a_shared_view_is_not_flagged_as_viewless() {
        use crate::colorspace::ColorSpace;

        let mut config = Config::new();
        config.add_colorspace(ColorSpace::new("sRGB"));
        config.add_shared_view(crate::display::View::new("Film", "sRGB"));
        config
            .add_display_shared_view("sRGB Monitor", "Film")
            .unwrap();

        let issues = check(&config);
        assert!(
            !issues
                .iter()
                .any(|i| i.category == IssueCategory::MissingDisplay
                    && i.message.contains("sRGB Monitor")),
            "display relying solely on a shared view was wrongly flagged: {issues:?}"
        );
    }

    /// A display referencing a shared view that was never defined in
    /// `config.shared_views()` must be caught (OCIO
    /// `Config::Impl::validateSharedView`) — `add_display_shared_view` alone
    /// does not check the referenced view actually exists.
    #[test]
    fn display_referencing_undefined_shared_view_is_flagged() {
        let mut config = Config::new();
        config
            .add_display_shared_view("sRGB Monitor", "NoSuchView")
            .unwrap();

        let issues = check(&config);
        assert!(
            issues
                .iter()
                .any(|i| i.category == IssueCategory::MissingSharedView),
            "expected a MissingSharedView issue, got: {issues:?}"
        );
    }

    /// A look with an empty process space, or one pointing at a
    /// non-existent color space, must be flagged (OCIO `Config::validate()`
    /// LOOKS section, `Config.cpp:1880-1930`).
    #[test]
    fn look_with_undefined_process_space_is_flagged() {
        use crate::look::Look;

        let mut config = Config::new();
        config.add_look(Look::new("Show LUT").process_space("NoSuchSpace"));

        let issues = check(&config);
        assert!(
            issues
                .iter()
                .any(|i| i.category == IssueCategory::MissingColorSpace
                    && i.message.contains("Show LUT")),
            "expected a MissingColorSpace issue naming the look, got: {issues:?}"
        );
    }

    #[test]
    fn look_with_valid_process_space_is_not_flagged() {
        use crate::colorspace::ColorSpace;
        use crate::look::Look;

        let mut config = Config::new();
        config.add_colorspace(ColorSpace::new("ACEScct"));
        config.add_look(Look::new("Show LUT").process_space("ACEScct"));

        let issues = check(&config);
        assert!(
            !issues
                .iter()
                .any(|i| i.category == IssueCategory::InvalidLook
                    || (i.category == IssueCategory::MissingColorSpace
                        && i.message.contains("Show LUT"))),
            "valid look wrongly flagged: {issues:?}"
        );
    }

    /// Viewing rules: a rule naming a nonexistent color space is an error; a
    /// rule with neither `colorspaces` nor `encodings` is an error; a rule
    /// with BOTH is also an error (OCIO `ViewingRule::validate`,
    /// `ViewingRules.cpp:66-111` — authoring-time exclusivity, distinct from
    /// `is_display_view_applicable`'s runtime fallthrough for configs that
    /// already violate it).
    #[test]
    fn viewing_rule_bad_colorspace_is_flagged() {
        use crate::config::types::ViewingRule;

        let mut config = Config::new();
        config.add_viewing_rule(ViewingRule {
            name: "log rule".into(),
            colorspaces: vec!["NoSuchSpace".into()],
            encodings: vec![],
            custom_keys: vec![],
        });

        let issues = check(&config);
        assert!(
            issues
                .iter()
                .any(|i| i.category == IssueCategory::MissingColorSpace
                    && i.message.contains("log rule")),
            "expected a MissingColorSpace issue naming the rule, got: {issues:?}"
        );
    }

    #[test]
    fn viewing_rule_with_neither_colorspaces_nor_encodings_is_flagged() {
        use crate::config::types::ViewingRule;

        let mut config = Config::new();
        config.add_viewing_rule(ViewingRule {
            name: "empty rule".into(),
            colorspaces: vec![],
            encodings: vec![],
            custom_keys: vec![],
        });

        let issues = check(&config);
        assert!(
            issues
                .iter()
                .any(|i| i.category == IssueCategory::InvalidViewingRule),
            "expected an InvalidViewingRule issue, got: {issues:?}"
        );
    }

    /// A `ColorSpaceTransform` inside a colorspace's `to_reference` slot
    /// referencing an undefined color space must be caught — the generic
    /// "every transform's colorspace reference resolves" sweep, ported from
    /// OCIO's `Config::validate()` TRANSFORMS section + `GetColorSpaceReferences`
    /// (`Config.cpp:177-208,1814-1878`).
    #[test]
    fn colorspace_transform_referencing_undefined_colorspace_is_flagged() {
        use crate::colorspace::ColorSpace;
        use crate::transform::{ColorSpaceTransform, Transform, TransformDirection};

        let mut config = Config::new();
        config.add_colorspace(
            ColorSpace::builder("sRGB")
                .to_reference(Transform::ColorSpace(ColorSpaceTransform {
                    src: "NoSuchSpace".into(),
                    dst: "sRGB".into(),
                    data_bypass: true,
                    direction: TransformDirection::Forward,
                }))
                .build(),
        );

        let issues = check(&config);
        assert!(
            issues
                .iter()
                .any(|i| i.category == IssueCategory::DanglingTransformReference
                    && i.message.contains("NoSuchSpace")),
            "expected a DanglingTransformReference issue naming NoSuchSpace, got: {issues:?}"
        );
    }

    /// A dangling reference nested inside a `GroupTransform` must be found
    /// too — the recursion into `GroupTransform` (matching
    /// `GetColorSpaceReferences`'s own `GroupTransform` recursion) is not
    /// optional/best-effort.
    #[test]
    fn dangling_reference_nested_in_group_transform_is_flagged() {
        use crate::colorspace::ColorSpace;
        use crate::transform::{
            ColorSpaceTransform, GroupTransform, Transform, TransformDirection,
        };

        let mut config = Config::new();
        config.add_colorspace(
            ColorSpace::builder("sRGB")
                .to_reference(Transform::Group(GroupTransform {
                    name: String::new(),
                    transforms: vec![Transform::ColorSpace(ColorSpaceTransform {
                        src: "GhostSpace".into(),
                        dst: "sRGB".into(),
                        data_bypass: true,
                        direction: TransformDirection::Forward,
                    })],
                    direction: TransformDirection::Forward,
                }))
                .build(),
        );

        let issues = check(&config);
        assert!(
            issues
                .iter()
                .any(|i| i.category == IssueCategory::DanglingTransformReference
                    && i.message.contains("GhostSpace")),
            "expected a DanglingTransformReference issue naming GhostSpace, got: {issues:?}"
        );
    }

    /// A `LookTransform` referencing a defined color space but an
    /// UNDEFINED look name (in its comma-separated `looks` string) must be
    /// caught. This is scope this crate adds beyond OCIO's own
    /// `GetColorSpaceReferences` (which only checks src/dst, not look
    /// names) because dangling look references are exactly the class of
    /// silent-wrong-output bug this bughunt pass has repeatedly found in
    /// look resolution (`RunLookTokens`).
    #[test]
    fn look_transform_referencing_undefined_look_name_is_flagged() {
        use crate::colorspace::ColorSpace;
        use crate::transform::{LookTransform, Transform, TransformDirection};

        let mut config = Config::new();
        config.add_colorspace(ColorSpace::new("ACEScct"));
        config.add_colorspace(
            ColorSpace::builder("sRGB")
                .to_reference(Transform::Look(LookTransform {
                    src: "ACEScct".into(),
                    dst: "sRGB".into(),
                    looks: "NoSuchLook".into(),
                    skip_color_space_conversion: false,
                    direction: TransformDirection::Forward,
                }))
                .build(),
        );

        let issues = check(&config);
        assert!(
            issues
                .iter()
                .any(|i| i.category == IssueCategory::DanglingTransformReference
                    && i.message.contains("NoSuchLook")),
            "expected a DanglingTransformReference issue naming NoSuchLook, got: {issues:?}"
        );
    }

    /// A `ColorSpaceTransform` referencing a ROLE (not a color space name
    /// directly) must NOT be flagged — matches OCIO's own fallback order
    /// (color space, then role, then named transform).
    #[test]
    fn colorspace_transform_referencing_a_role_is_not_flagged() {
        use crate::colorspace::ColorSpace;
        use crate::transform::{ColorSpaceTransform, Transform, TransformDirection};

        let mut config = Config::new();
        config.add_colorspace(ColorSpace::new("ACEScct"));
        config.set_role("compositing_log", "ACEScct");
        config.add_colorspace(
            ColorSpace::builder("sRGB")
                .to_reference(Transform::ColorSpace(ColorSpaceTransform {
                    src: "compositing_log".into(),
                    dst: "sRGB".into(),
                    data_bypass: true,
                    direction: TransformDirection::Forward,
                }))
                .build(),
        );

        let issues = check(&config);
        assert!(
            !issues
                .iter()
                .any(|i| i.category == IssueCategory::DanglingTransformReference),
            "a role reference was wrongly flagged as dangling: {issues:?}"
        );
    }

    #[test]
    fn viewing_rule_with_both_colorspaces_and_encodings_is_flagged() {
        use crate::colorspace::ColorSpace;
        use crate::config::types::ViewingRule;

        let mut config = Config::new();
        config.add_colorspace(ColorSpace::new("sRGB"));
        config.add_viewing_rule(ViewingRule {
            name: "mixed rule".into(),
            colorspaces: vec!["sRGB".into()],
            encodings: vec!["log".into()],
            custom_keys: vec![],
        });

        let issues = check(&config);
        assert!(
            issues
                .iter()
                .any(|i| i.category == IssueCategory::InvalidViewingRule),
            "expected an InvalidViewingRule issue for mixing colorspaces+encodings, got: {issues:?}"
        );
    }
}
