//! Type definitions for the OCIO config (enums + small structs).
//!
//! Extracted from `config.rs` during the STRUCT.2 split (TODO6.md).
//! Contains: EnvironmentMode, SearchReferenceSpaceType, ColorSpaceVisibility,
//! NamedTransformVisibility, ConfigVersion, FileRule, NamedTransform (+ impl),
//! ViewingRule, VirtualDisplay, VirtualDisplayView, FileRuleKind.
//!
//! Re-exported via `pub use types::*;` so external paths
//! (`crate::config::ConfigVersion`, etc.) keep working unchanged.
//! The big `Config` struct itself stays in `config.rs` because it has 30+
//! private fields that would each need `pub(super)` markers — not worth
//! the churn for a cosmetic split.

use super::*;

/// Controls which environment variables are loaded into the config context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EnvironmentMode {
    /// Load only variables defined in the config's environment section.
    #[default]
    LoadPredefined,
    /// Load all system environment variables (may reduce performance).
    LoadAll,
}

/// Filter for searching color spaces by reference space type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchReferenceSpaceType {
    /// Scene-referred spaces only.
    #[default]
    Scene,
    /// Display-referred spaces only.
    Display,
    /// Both scene and display spaces.
    All,
}

/// Filter for color space visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorSpaceVisibility {
    /// Only active (non-inactive) color spaces.
    #[default]
    Active,
    /// Only inactive color spaces.
    Inactive,
    /// All color spaces regardless of status.
    All,
}

/// Filter for named transform visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NamedTransformVisibility {
    /// Only active (non-inactive) named transforms.
    #[default]
    Active,
    /// Only inactive named transforms.
    Inactive,
    /// All named transforms regardless of status.
    All,
}

/// Config format version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConfigVersion {
    /// OCIO v1.x.
    #[default]
    V1,
    /// OCIO v2.x.
    V2,
}

/// Source used to create a config (affects archiving rules).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigSource {
    Raw,
    File,
    Stream,
    Archive,
    IOProxy,
}

/// File rule for automatic color space assignment.
#[derive(Debug, Clone, Default)]
pub struct FileRule {
    /// Rule name.
    pub name: String,
    /// Assigned color space.
    pub colorspace: String,
    /// Rule matching kind.
    pub kind: FileRuleKind,
    /// Custom key-value pairs (OCIO `FileRules::getNumCustomKeys` /
    /// `setCustomKey`). Free-form metadata attached per rule.
    pub custom_keys: Vec<(String, String)>,
}

/// Named transform definition (OCIO v2.0+).
/// Standalone transforms that can be referenced by name.
#[derive(Debug, Clone, Default)]
pub struct NamedTransform {
    /// Transform name.
    pub name: String,
    /// Alternate names (OCIO `NamedTransform::m_aliases`).
    pub aliases: Vec<String>,
    /// Family grouping.
    pub family: Option<String>,
    /// Description.
    pub description: Option<String>,
    /// Category tags (OCIO `NamedTransform::m_categories`) — drive UI
    /// filtering.
    pub categories: Vec<String>,
    /// Encoding hint (OCIO `NamedTransform::m_encoding`). Free-form
    /// string, parallel to `ColorSpace::encoding`.
    pub encoding: Option<String>,
    /// Forward transform.
    pub forward: Option<Transform>,
    /// Inverse transform.
    pub inverse: Option<Transform>,
}

impl NamedTransform {
    /// Creates a new named transform.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            aliases: Vec::new(),
            family: None,
            description: None,
            categories: Vec::new(),
            encoding: None,
            forward: None,
            inverse: None,
        }
    }

    /// Returns the aliases slice (OCIO `getAliases`).
    #[inline]
    pub fn aliases(&self) -> &[String] {
        &self.aliases
    }

    /// Checks whether `name` refers to this named transform, by its
    /// primary name OR any alias, case-insensitively — the same contract
    /// `ColorSpace::matches_name` has, so a config lookup by name behaves
    /// consistently whether it lands on a colorspace or a named transform.
    #[inline]
    pub fn matches_name(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
            || self.aliases.iter().any(|a| a.eq_ignore_ascii_case(name))
    }

    /// Number of aliases (OCIO `getNumAliases`).
    #[inline]
    pub fn num_aliases(&self) -> usize {
        self.aliases.len()
    }

    /// Indexed alias accessor (OCIO `getAlias(idx)`).
    #[inline]
    pub fn alias(&self, index: usize) -> Option<&str> {
        self.aliases.get(index).map(|s| s.as_str())
    }

    /// Case-insensitive alias presence check (OCIO `hasAlias`).
    pub fn has_alias(&self, name: &str) -> bool {
        self.aliases.iter().any(|a| a.eq_ignore_ascii_case(name))
    }

    /// Appends an alias, deduplicating case-insensitively (OCIO
    /// `addAlias`).
    pub fn add_alias(&mut self, alias: impl Into<String>) {
        let alias = alias.into();
        if !self.has_alias(&alias) {
            self.aliases.push(alias);
        }
    }

    /// Removes an alias by case-insensitive match. Returns `true`
    /// when a removal happened (OCIO `removeAlias`).
    pub fn remove_alias(&mut self, name: &str) -> bool {
        if let Some(i) = self
            .aliases
            .iter()
            .position(|a| a.eq_ignore_ascii_case(name))
        {
            self.aliases.remove(i);
            return true;
        }
        false
    }

    /// Drops every alias (OCIO `clearAliases`).
    #[inline]
    pub fn clear_aliases(&mut self) {
        self.aliases.clear();
    }

    /// Returns the categories slice (OCIO `getCategories`).
    #[inline]
    pub fn categories(&self) -> &[String] {
        &self.categories
    }

    /// Number of categories (OCIO `getNumCategories`).
    #[inline]
    pub fn num_categories(&self) -> usize {
        self.categories.len()
    }

    /// Indexed category accessor (OCIO `getCategory(idx)`).
    #[inline]
    pub fn category(&self, index: usize) -> Option<&str> {
        self.categories.get(index).map(|s| s.as_str())
    }

    /// Case-insensitive category membership (OCIO `hasCategory`).
    pub fn has_category(&self, name: &str) -> bool {
        self.categories.iter().any(|c| c.eq_ignore_ascii_case(name))
    }

    /// Appends a category, deduplicating case-insensitively
    /// (OCIO `addCategory`).
    pub fn add_category(&mut self, name: impl Into<String>) {
        let name = name.into();
        if !self.has_category(&name) {
            self.categories.push(name);
        }
    }

    /// Removes a category by case-insensitive match.
    pub fn remove_category(&mut self, name: &str) -> bool {
        if let Some(i) = self
            .categories
            .iter()
            .position(|c| c.eq_ignore_ascii_case(name))
        {
            self.categories.remove(i);
            return true;
        }
        false
    }

    /// Drops every category (OCIO `clearCategories`).
    #[inline]
    pub fn clear_categories(&mut self) {
        self.categories.clear();
    }

    /// Returns the encoding hint (OCIO `getEncoding`).
    #[inline]
    pub fn encoding(&self) -> Option<&str> {
        self.encoding.as_deref()
    }

    /// Sets the encoding hint (OCIO `setEncoding`). Pass `""` to
    /// clear.
    pub fn set_encoding(&mut self, encoding: impl Into<String>) {
        let s = encoding.into();
        self.encoding = if s.is_empty() { None } else { Some(s) };
    }

    /// Creates an editable copy (OCIO parity).
    #[inline]
    pub fn create_editable_copy(&self) -> Self {
        self.clone()
    }

    /// Returns the name.
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Sets the name.
    #[inline]
    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    /// Returns the family.
    #[inline]
    pub fn family(&self) -> Option<&str> {
        self.family.as_deref()
    }

    /// Sets the family.
    #[inline]
    pub fn set_family(&mut self, family: Option<String>) {
        self.family = family;
    }

    /// Returns the description.
    #[inline]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Sets the description.
    #[inline]
    pub fn set_description(&mut self, description: Option<String>) {
        self.description = description;
    }

    /// Returns the forward transform.
    #[inline]
    pub fn forward(&self) -> Option<&Transform> {
        self.forward.as_ref()
    }

    /// Returns the inverse transform.
    #[inline]
    pub fn inverse(&self) -> Option<&Transform> {
        self.inverse.as_ref()
    }

    /// Sets the forward transform.
    #[inline]
    pub fn set_forward(&mut self, transform: Option<Transform>) {
        self.forward = transform;
    }

    /// Sets the inverse transform.
    #[inline]
    pub fn set_inverse(&mut self, transform: Option<Transform>) {
        self.inverse = transform;
    }
}

/// Viewing rule definition (OCIO v2.0+).
/// Rules filter which views are applicable based on colorspace encoding.
#[derive(Debug, Clone)]
pub struct ViewingRule {
    /// Rule name (referenced by shared_views).
    pub name: String,
    /// List of colorspace names this rule applies to.
    ///
    /// Config authoring convention is one or the other, not both, but the
    /// matcher (`Config::is_display_view_applicable`) does not enforce that:
    /// per OCIO's own `Config::Impl::getFilteredViews` (`Config.cpp:846-908`),
    /// a rule with both non-empty is not "colorspaces XOR encodings" — a
    /// colorspaces miss falls through to also try `encodings` on the same
    /// rule, and the view is included if EITHER matches.
    pub colorspaces: Vec<String>,
    /// List of encoding types this rule applies to (e.g., "log", "scene-linear").
    /// See `colorspaces`' doc comment for how the two combine.
    pub encodings: Vec<String>,
    /// Custom key-value pairs.
    pub custom_keys: Vec<(String, String)>,
}

/// Virtual display definition (OCIO v2.0+).
/// Template for creating displays from ICC profiles.
/// There may only be one virtual display per config.
/// OCIO ref: Config::addVirtualDisplayView, instantiateDisplayFromICCProfile
#[derive(Debug, Clone, Default)]
pub struct VirtualDisplay {
    /// Views defined in this virtual display.
    /// These serve as templates for ICC-instantiated displays.
    pub views: Vec<VirtualDisplayView>,
    /// References to shared views.
    pub shared_view_refs: Vec<String>,
}

/// A view within a virtual display.
/// OCIO ref: Config::addVirtualDisplayView
#[derive(Debug, Clone)]
pub struct VirtualDisplayView {
    /// View name.
    pub name: String,
    /// View transform name (optional).
    pub view_transform: Option<String>,
    /// Display colorspace. Use "<USE_DISPLAY_NAME>" to use the ICC profile's colorspace.
    pub display_colorspace: String,
    /// Looks to apply (optional).
    pub looks: Option<String>,
    /// Viewing rule name (optional).
    pub rule: Option<String>,
    /// Description (optional).
    pub description: Option<String>,
}

/// File rule matching behavior.
#[derive(Debug, Clone, Default)]
pub enum FileRuleKind {
    /// Basic rule: glob pattern + optional extension.
    Basic {
        /// Glob pattern.
        pattern: String,
        /// Optional extension matcher.
        extension: Option<String>,
    },
    /// Regex rule: regex pattern.
    Regex {
        /// Compiled regex pattern.
        regex: Regex,
    },
    /// The special `ColorSpaceNamePathSearch` rule (OCIO
    /// `FileRules::FilePathSearchRuleName`, `FileRules.cpp:25`): resolves
    /// the colorspace by scanning the path for the right-most substring
    /// that names a known colorspace (or alias), ties broken by the
    /// longest match (`PathUtils.cpp::ParseColorSpaceFromString`). Takes
    /// no pattern/extension/regex/colorspace of its own — the matched
    /// colorspace comes from the path itself.
    PathSearch,
    /// Default rule (fallback).
    #[default]
    Default,
}
