//! Standard color space roles.
//!
//! Roles provide a consistent way to reference color spaces by their purpose
//! rather than their specific name. This enables config portability.
//!
//! # Standard Roles
//!
//! - `reference` - Scene-referred linear reference space (required)
//! - `default` - Default input color space
//! - `data` - Non-color data (normals, masks, etc.)
//! - `scene_linear` - Scene-referred linear working space
//! - `rendering` - Space for rendering calculations
//! - `compositing_linear` - Linear compositing space
//! - `color_timing` - Color grading/timing space
//! - `texture_paint` - Texture painting space
//! - `matte_paint` - Matte painting space
//! - `color_picking` - Color picker display space

use std::collections::HashMap;

/// Standard OCIO role names.
pub mod names {
    /// Scene-referred linear reference (required).
    pub const REFERENCE: &str = "reference";
    /// Default input color space.
    pub const DEFAULT: &str = "default";
    /// Non-color data (normals, masks).
    pub const DATA: &str = "data";
    /// Scene-referred linear working space.
    pub const SCENE_LINEAR: &str = "scene_linear";
    /// Rendering calculations space.
    pub const RENDERING: &str = "rendering";
    /// Compositing log space.
    pub const COMPOSITING_LOG: &str = "compositing_log";
    /// Linear compositing space.
    pub const COMPOSITING_LINEAR: &str = "compositing_linear";
    /// Color grading space.
    pub const COLOR_TIMING: &str = "color_timing";
    /// Texture painting space.
    pub const TEXTURE_PAINT: &str = "texture_paint";
    /// Matte painting space.
    pub const MATTE_PAINT: &str = "matte_paint";
    /// Color picker display space.
    pub const COLOR_PICKING: &str = "color_picking";
    /// ACES interchange scene-referred.
    pub const ACES_INTERCHANGE: &str = "aces_interchange";
    /// CIE XYZ interchange (D65).
    pub const CIE_XYZ_D65_INTERCHANGE: &str = "cie_xyz_d65_interchange";
}

/// Role to color space mapping.
///
/// This struct manages the mapping between role names and actual color space names
/// defined in the config.
#[derive(Debug, Clone, Default)]
pub struct Roles {
    /// Role name -> color space name mapping.
    mapping: HashMap<String, String>,
}

impl Roles {
    /// Creates an empty roles mapping.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Defines a role mapping.
    ///
    /// # Arguments
    ///
    /// * `role` - Role name (e.g., "scene_linear")
    /// * `colorspace` - Color space name this role maps to
    #[inline]
    pub fn define(&mut self, role: impl Into<String>, colorspace: impl Into<String>) {
        self.mapping.insert(role.into(), colorspace.into());
    }

    /// Removes a role mapping.
    pub fn remove(&mut self, role: &str) -> Option<String> {
        self.mapping.remove(role)
    }

    /// Clears all role mappings.
    pub fn clear(&mut self) {
        self.mapping.clear();
    }

    /// Gets the color space name for a role.
    ///
    /// Role names match case-insensitively, as OCIO's `LookupRole` does (it lowercases both the
    /// stored keys and the query, `Config.cpp` `LookupRole`). Returns `None` if the role is not
    /// defined.
    pub fn get(&self, role: &str) -> Option<&str> {
        if let Some(cs) = self.mapping.get(role) {
            return Some(cs.as_str());
        }
        self.mapping
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(role))
            .map(|(_, v)| v.as_str())
    }

    /// Checks if a role is defined (case-insensitively, see [`Self::get`]).
    #[inline]
    pub fn contains(&self, role: &str) -> bool {
        self.get(role).is_some()
    }

    /// Returns all defined roles.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.mapping.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Number of defined roles.
    #[inline]
    pub fn len(&self) -> usize {
        self.mapping.len()
    }

    /// Checks if no roles are defined.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.mapping.is_empty()
    }

    /// Checks if the required reference role is defined.
    #[inline]
    pub fn has_reference(&self) -> bool {
        self.contains(names::REFERENCE)
    }

    /// Gets the reference color space name.
    #[inline]
    pub fn reference(&self) -> Option<&str> {
        self.get(names::REFERENCE)
    }

    /// Gets the scene_linear color space name.
    #[inline]
    pub fn scene_linear(&self) -> Option<&str> {
        self.get(names::SCENE_LINEAR)
    }

    /// Gets the data color space name.
    #[inline]
    pub fn data(&self) -> Option<&str> {
        self.get(names::DATA)
    }

    /// Gets the default input color space name.
    #[inline]
    pub fn default_input(&self) -> Option<&str> {
        self.get(names::DEFAULT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn define_and_get() {
        let mut roles = Roles::new();
        roles.define("scene_linear", "ACEScg");
        roles.define("reference", "ACES2065-1");

        assert_eq!(roles.get("scene_linear"), Some("ACEScg"));
        assert_eq!(roles.get("reference"), Some("ACES2065-1"));
        assert_eq!(roles.get("unknown"), None);
    }

    #[test]
    fn has_reference() {
        let mut roles = Roles::new();
        assert!(!roles.has_reference());

        roles.define("reference", "Linear");
        assert!(roles.has_reference());
    }

    #[test]
    fn iterate_roles() {
        let mut roles = Roles::new();
        roles.define("a", "A");
        roles.define("b", "B");

        let pairs: Vec<_> = roles.iter().collect();
        assert_eq!(pairs.len(), 2);
    }
}
