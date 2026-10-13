//! Look definitions for creative color transforms.
//!
//! Looks are named creative transforms that can be applied on top of the
//! display pipeline. They're typically used for:
//! - Show/shot-specific color grades
//! - Creative LUTs
//! - Per-sequence adjustments
//!
//! # Example
//!
//! ```
//! use vfx_ocio::Look;
//!
//! let look = Look::new("Show LUT")
//!     .process_space("ACEScct")
//!     .description("Main show look");
//!
//! assert_eq!(look.name(), "Show LUT");
//! assert_eq!(look.get_process_space(), Some("ACEScct"));
//! ```

use std::collections::HashMap;

use crate::transform::Transform;

/// A named creative look/grade.
#[derive(Debug, Clone)]
pub struct Look {
    /// Look name.
    name: String,
    /// Process space (color space where transform is applied).
    process_space: Option<String>,
    /// Description.
    description: String,
    /// Forward transform.
    transform: Option<Transform>,
    /// Inverse transform (optional, for reversibility).
    inverse_transform: Option<Transform>,
    /// Interchange attributes (OCIO `Look::m_interchangeAttributes`).
    /// Typically holds entries like `amf_transform_ids` for AMF round-
    /// tripping. Persisted through YAML `interchange:` blocks.
    interchange_attributes: HashMap<String, String>,
}

impl Look {
    /// Creates a new look with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            process_space: None,
            description: String::new(),
            transform: None,
            inverse_transform: None,
            interchange_attributes: HashMap::new(),
        }
    }

    /// Returns an interchange attribute by key (OCIO
    /// `Look::getInterchangeAttribute`).
    #[inline]
    pub fn interchange_attribute(&self, name: &str) -> Option<&str> {
        self.interchange_attributes.get(name).map(|s| s.as_str())
    }

    /// Sets an interchange attribute (OCIO
    /// `Look::setInterchangeAttribute`).
    #[inline]
    pub fn set_interchange_attribute(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.interchange_attributes
            .insert(name.into(), value.into());
    }

    /// Returns interchange attribute names (OCIO
    /// `Look::getInterchangeAttributes`).
    #[inline]
    pub fn interchange_attribute_names(&self) -> impl Iterator<Item = &str> {
        self.interchange_attributes.keys().map(|k| k.as_str())
    }

    /// Removes an interchange attribute by name.
    #[inline]
    pub fn remove_interchange_attribute(&mut self, name: &str) {
        self.interchange_attributes.remove(name);
    }

    /// Clears all interchange attributes.
    #[inline]
    pub fn clear_interchange_attributes(&mut self) {
        self.interchange_attributes.clear();
    }

    /// Creates an editable copy (OCIO parity).
    #[inline]
    pub fn create_editable_copy(&self) -> Self {
        self.clone()
    }

    /// Sets the process space.
    pub fn process_space(mut self, space: impl Into<String>) -> Self {
        self.process_space = Some(space.into());
        self
    }

    /// Sets the process space.
    pub fn set_process_space(&mut self, space: impl Into<String>) {
        self.process_space = Some(space.into());
    }

    /// Clears the process space.
    pub fn clear_process_space(&mut self) {
        self.process_space = None;
    }

    /// Sets the description.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Sets the description.
    pub fn set_description(&mut self, desc: impl Into<String>) {
        self.description = desc.into();
    }

    /// Sets the forward transform.
    pub fn transform(mut self, t: Transform) -> Self {
        self.transform = Some(t);
        self
    }

    /// Sets the forward transform.
    pub fn set_transform(&mut self, t: Option<Transform>) {
        self.transform = t;
    }

    /// Sets the inverse transform.
    pub fn inverse_transform(mut self, t: Transform) -> Self {
        self.inverse_transform = Some(t);
        self
    }

    /// Sets the inverse transform.
    pub fn set_inverse_transform(&mut self, t: Option<Transform>) {
        self.inverse_transform = t;
    }

    /// Returns the look name.
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Sets the look name.
    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    /// Returns the process space name.
    #[inline]
    pub fn get_process_space(&self) -> Option<&str> {
        self.process_space.as_deref()
    }

    /// Returns the description.
    #[inline]
    pub fn get_description(&self) -> &str {
        &self.description
    }

    /// Returns the forward transform.
    #[inline]
    pub fn get_transform(&self) -> Option<&Transform> {
        self.transform.as_ref()
    }

    /// Returns the inverse transform.
    #[inline]
    pub fn get_inverse_transform(&self) -> Option<&Transform> {
        self.inverse_transform.as_ref()
    }

    /// Checks if this look has an explicit inverse.
    #[inline]
    pub fn has_inverse(&self) -> bool {
        self.inverse_transform.is_some()
    }
}

/// Collection of looks.
#[derive(Debug, Clone, Default)]
pub struct LookManager {
    /// All looks.
    looks: Vec<Look>,
}

impl LookManager {
    /// Creates an empty look manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a look.
    pub fn add(&mut self, look: Look) {
        self.looks.push(look);
    }

    /// Removes a look by name.
    pub fn remove(&mut self, name: &str) -> Option<Look> {
        if let Some(index) = self
            .looks
            .iter()
            .position(|l| l.name.eq_ignore_ascii_case(name))
        {
            return Some(self.looks.remove(index));
        }
        None
    }

    /// Clears all looks.
    pub fn clear(&mut self) {
        self.looks.clear();
    }

    /// Gets a look by name.
    pub fn get(&self, name: &str) -> Option<&Look> {
        self.looks
            .iter()
            .find(|l| l.name.eq_ignore_ascii_case(name))
    }

    /// Returns all looks.
    #[inline]
    pub fn all(&self) -> &[Look] {
        &self.looks
    }

    /// Returns look names.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.looks.iter().map(|l| l.name.as_str())
    }

    /// Number of looks.
    #[inline]
    pub fn len(&self) -> usize {
        self.looks.len()
    }

    /// Checks if empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.looks.is_empty()
    }
}

/// Parses a comma-separated look string into individual look names.
///
/// Supports:
/// - Single look: `"ShowLUT"`
/// - Multiple looks: `"ShowLUT, ShotGrade"`
/// - Look with direction: `"+ShowLUT"` (forward), `"-ShowLUT"` (inverse)
///
/// Returns tuples of (name, is_forward).
pub fn parse_looks(looks_str: &str) -> Vec<(&str, bool)> {
    looks_str
        .split(',')
        .filter_map(|s| {
            let s = s.trim();
            if s.is_empty() {
                return None;
            }
            if let Some(name) = s.strip_prefix('-') {
                Some((name.trim(), false))
            } else if let Some(name) = s.strip_prefix('+') {
                Some((name.trim(), true))
            } else {
                Some((s, true))
            }
        })
        .collect()
}

/// Parses a look string with OCIO's `|` fall-back options (`LookParseResult::parse`): each
/// option is a [`parse_looks`] list, and the first option whose files can be located is used
/// (see `Config::looks_result_space`). `"a, b | c"` is the options `[a, b]` and `[c]`; an empty
/// string has none.
pub fn parse_look_options(looks_str: &str) -> Vec<Vec<(&str, bool)>> {
    if looks_str.trim().is_empty() {
        return Vec::new();
    }
    looks_str.split('|').map(parse_looks).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn look_options_split_on_bar() {
        assert!(parse_look_options("  ").is_empty());
        let o = parse_look_options("a, -b | c");
        assert_eq!(o, vec![vec![("a", true), ("b", false)], vec![("c", true)]]);
    }

    #[test]
    fn build_look() {
        let look = Look::new("Film Grade")
            .process_space("ACEScct")
            .description("Main film look");

        assert_eq!(look.name(), "Film Grade");
        assert_eq!(look.get_process_space(), Some("ACEScct"));
    }

    #[test]
    fn look_manager() {
        let mut mgr = LookManager::new();
        mgr.add(Look::new("Look A"));
        mgr.add(Look::new("Look B"));

        assert_eq!(mgr.len(), 2);
        assert!(mgr.get("look a").is_some());
    }

    #[test]
    fn parse_looks_single() {
        let looks = parse_looks("ShowLUT");
        assert_eq!(looks, vec![("ShowLUT", true)]);
    }

    #[test]
    fn parse_looks_multiple() {
        let looks = parse_looks("ShowLUT, ShotGrade, Burnin");
        assert_eq!(looks.len(), 3);
        assert_eq!(looks[0], ("ShowLUT", true));
        assert_eq!(looks[1], ("ShotGrade", true));
    }

    #[test]
    fn parse_looks_with_direction() {
        let looks = parse_looks("+Forward, -Inverse");
        assert_eq!(looks[0], ("Forward", true));
        assert_eq!(looks[1], ("Inverse", false));
    }
}
