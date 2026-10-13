//! Color space definition and properties.
//!
//! A color space in OCIO defines:
//! - How to convert to/from the reference space
//! - Categorization (family, encoding, etc.)
//! - Metadata (description, aliases)
//!
//! # Example
//!
//! ```
//! use vfx_ocio::{ColorSpace, Encoding, Family};
//!
//! let cs = ColorSpace::builder("ACEScg")
//!     .family(Family::Scene)
//!     .encoding(Encoding::SceneLinear)
//!     .description("ACES CG working space")
//!     .build();
//!
//! assert_eq!(cs.name(), "ACEScg");
//! assert_eq!(cs.encoding(), Encoding::SceneLinear);
//! ```

use crate::display::ReferenceSpaceType;
use crate::transform::Transform;
use std::collections::HashMap;

/// Color encoding type.
///
/// Indicates the data encoding/interpretation of pixel values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Encoding {
    /// Scene-referred linear.
    SceneLinear,
    /// Display-referred linear.
    DisplayLinear,
    /// Logarithmic encoding.
    Log,
    /// OETF-encoded (sRGB, Rec.709, etc.).
    Sdr,
    /// HDR display encoding (PQ, HLG).
    Hdr,
    /// Non-color data (normals, masks).
    Data,
    /// Unknown/unspecified encoding.
    #[default]
    Unknown,
}

impl Encoding {
    /// Parses encoding from OCIO config string.
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "scene-linear" | "scene_linear" | "linear" => Self::SceneLinear,
            "display-linear" | "display_linear" => Self::DisplayLinear,
            "log" => Self::Log,
            "sdr-video" | "sdr_video" | "sdr" => Self::Sdr,
            "hdr-video" | "hdr_video" | "hdr" => Self::Hdr,
            "data" => Self::Data,
            _ => Self::Unknown,
        }
    }

    /// Returns OCIO config string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SceneLinear => "scene-linear",
            Self::DisplayLinear => "display-linear",
            Self::Log => "log",
            Self::Sdr => "sdr-video",
            Self::Hdr => "hdr-video",
            Self::Data => "data",
            Self::Unknown => "",
        }
    }

    /// Checks if this is a linear encoding.
    #[inline]
    pub fn is_linear(&self) -> bool {
        matches!(self, Self::SceneLinear | Self::DisplayLinear)
    }
}

/// Color space family/category.
///
/// Groups related color spaces for UI organization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Family {
    /// Scene-referred spaces (linear, log).
    Scene,
    /// Display-referred spaces.
    Display,
    /// Input/camera spaces.
    Input,
    /// Output/delivery spaces.
    Output,
    /// Utility spaces.
    Utility,
    /// ACES spaces.
    Aces,
    /// Custom/uncategorized.
    #[default]
    Other,
}

impl Family {
    /// Parses family from config string.
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "scene" | "scene-referred" => Self::Scene,
            "display" | "display-referred" => Self::Display,
            "input" | "camera" => Self::Input,
            "output" | "delivery" => Self::Output,
            "utility" | "utilities" => Self::Utility,
            "aces" => Self::Aces,
            _ => Self::Other,
        }
    }

    /// Returns display string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Scene => "Scene",
            Self::Display => "Display",
            Self::Input => "Input",
            Self::Output => "Output",
            Self::Utility => "Utility",
            Self::Aces => "ACES",
            Self::Other => "",
        }
    }
}

/// Re-export BitDepth from vfx-core.
pub use vfx_core::BitDepth;

/// Color space definition.
///
/// Represents a named color space with transforms to/from reference space.
#[derive(Debug, Clone)]
pub struct ColorSpace {
    /// Unique name.
    name: String,
    /// Alternative names.
    aliases: Vec<String>,
    /// Categories for filtering (OCIO v2).
    categories: Vec<String>,
    /// Human-readable description.
    description: String,
    /// Family/category.
    family: Family,
    /// The family string as the config spells it (OCIO's family is free text, e.g.
    /// `Input/ARRI`; [`Family`] only classifies it).
    family_name: String,
    /// Encoding type.
    encoding: Encoding,
    /// Encoding string name (preserves unknown/explicit strings).
    encoding_name: String,
    /// Bit depth hint.
    bit_depth: BitDepth,
    /// Whether this is for non-color data.
    is_data: bool,
    /// Reference space type (scene or display).
    reference_space_type: ReferenceSpaceType,
    /// Equality group for compare/merge.
    equality_group: String,
    /// Interop ID (OCIO v2).
    interop_id: String,
    /// Interchange attributes (OCIO v2).
    interchange_attributes: HashMap<String, String>,
    /// Transform from this space to reference.
    to_reference: Option<Transform>,
    /// Transform from reference to this space.
    from_reference: Option<Transform>,
    /// Transform from this space to display reference (OCIO v2).
    to_display_reference: Option<Transform>,
    /// Transform from display reference to this space (OCIO v2).
    from_display_reference: Option<Transform>,
    /// Allocation variables for GPU optimization.
    allocation: AllocationInfo,
}

/// GPU allocation hints.
#[derive(Debug, Clone, Default)]
pub struct AllocationInfo {
    /// Allocation type (uniform or log).
    pub alloc_type: AllocationType,
    /// Min/max range.
    pub vars: Vec<f32>,
}

/// The allocation of a colour space: the same enum as `AllocationTransform`'s (OCIO has one
/// `Allocation` enum for both).
pub use crate::transform::AllocationType;

impl ColorSpace {
    /// Creates a new color space with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            aliases: Vec::new(),
            categories: Vec::new(),
            description: String::new(),
            family: Family::default(),
            family_name: String::new(),
            encoding: Encoding::default(),
            encoding_name: String::new(),
            // OCIO: BIT_DEPTH_UNKNOWN until the config says otherwise.
            bit_depth: BitDepth::Unknown,
            is_data: false,
            reference_space_type: ReferenceSpaceType::Scene,
            equality_group: String::new(),
            interop_id: String::new(),
            interchange_attributes: HashMap::new(),
            to_reference: None,
            from_reference: None,
            to_display_reference: None,
            from_display_reference: None,
            allocation: AllocationInfo::default(),
        }
    }

    /// Creates a builder for constructing color spaces.
    #[inline]
    pub fn builder(name: impl Into<String>) -> ColorSpaceBuilder {
        ColorSpaceBuilder::new(name)
    }

    /// Creates an editable copy (OCIO parity).
    #[inline]
    pub fn create_editable_copy(&self) -> Self {
        self.clone()
    }

    /// Returns the color space name.
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Sets the color space name.
    #[inline]
    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    /// Returns all aliases.
    #[inline]
    pub fn aliases(&self) -> &[String] {
        &self.aliases
    }

    /// Returns the number of aliases.
    #[inline]
    pub fn num_aliases(&self) -> usize {
        self.aliases.len()
    }

    /// Returns alias by index.
    #[inline]
    pub fn alias(&self, index: usize) -> Option<&str> {
        self.aliases.get(index).map(|s| s.as_str())
    }

    /// Checks if the colorspace has a given alias (case-insensitive).
    #[inline]
    pub fn has_alias(&self, alias: &str) -> bool {
        self.aliases.iter().any(|a| a.eq_ignore_ascii_case(alias))
    }

    /// Adds an alias (case-insensitive uniqueness).
    pub fn add_alias(&mut self, alias: impl Into<String>) {
        let alias = alias.into();
        if !self.has_alias(&alias) {
            self.aliases.push(alias);
        }
    }

    /// Removes an alias (case-insensitive).
    pub fn remove_alias(&mut self, alias: &str) {
        self.aliases.retain(|a| !a.eq_ignore_ascii_case(alias));
    }

    /// Clears all aliases.
    #[inline]
    pub fn clear_aliases(&mut self) {
        self.aliases.clear();
    }

    /// Returns all categories.
    #[inline]
    pub fn categories(&self) -> &[String] {
        &self.categories
    }

    /// Checks if colorspace has a specific category.
    /// OCIO ref: ColorSpace::hasCategory
    #[inline]
    pub fn has_category(&self, category: &str) -> bool {
        self.categories
            .iter()
            .any(|c| c.eq_ignore_ascii_case(category))
    }

    /// Adds a category. Does nothing if already present (case-insensitive).
    /// OCIO ref: ColorSpace::addCategory
    pub fn add_category(&mut self, category: impl Into<String>) {
        let cat = category.into();
        if !self.has_category(&cat) {
            self.categories.push(cat);
        }
    }

    /// Removes a category. Does nothing if not present (case-insensitive).
    /// OCIO ref: ColorSpace::removeCategory
    pub fn remove_category(&mut self, category: &str) {
        self.categories
            .retain(|c| !c.eq_ignore_ascii_case(category));
    }

    /// Returns the number of categories.
    /// OCIO ref: ColorSpace::getNumCategories
    #[inline]
    pub fn num_categories(&self) -> usize {
        self.categories.len()
    }

    /// Returns category by index.
    /// OCIO ref: ColorSpace::getCategory
    #[inline]
    pub fn category(&self, index: usize) -> Option<&str> {
        self.categories.get(index).map(|s| s.as_str())
    }

    /// Clears all categories.
    /// OCIO ref: ColorSpace::clearCategories
    pub fn clear_categories(&mut self) {
        self.categories.clear();
    }

    /// Returns the description.
    #[inline]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Sets the description.
    #[inline]
    pub fn set_description(&mut self, desc: impl Into<String>) {
        self.description = desc.into();
    }

    /// Returns the family/category.
    #[inline]
    pub fn family(&self) -> Family {
        self.family
    }

    /// Sets the family/category.
    #[inline]
    pub fn set_family(&mut self, family: Family) {
        self.family = family;
        self.family_name = family.as_str().to_string();
    }

    /// Returns the family string (OCIO `ColorSpace::getFamily`).
    #[inline]
    pub fn family_name(&self) -> &str {
        &self.family_name
    }

    /// Sets the family string (OCIO `ColorSpace::setFamily`); [`Self::family`] classifies it.
    pub fn set_family_name(&mut self, family: impl Into<String>) {
        self.family_name = family.into();
        self.family = Family::parse(&self.family_name);
    }

    /// Returns the encoding type.
    #[inline]
    pub fn encoding(&self) -> Encoding {
        self.encoding
    }

    /// Returns the encoding name string.
    #[inline]
    pub fn encoding_name(&self) -> &str {
        &self.encoding_name
    }

    /// Sets the encoding type.
    #[inline]
    pub fn set_encoding(&mut self, encoding: Encoding) {
        self.encoding = encoding;
        if encoding != Encoding::Unknown {
            self.encoding_name = encoding.as_str().to_string();
        }
    }

    /// Sets the encoding name string (preserves unknown values).
    #[inline]
    pub fn set_encoding_name(&mut self, name: impl Into<String>) {
        self.encoding_name = name.into();
        if !self.encoding_name.is_empty() {
            self.encoding = Encoding::parse(&self.encoding_name);
        }
    }

    /// Returns the bit depth hint.
    #[inline]
    pub fn bit_depth(&self) -> BitDepth {
        self.bit_depth
    }

    /// Sets the bit depth hint.
    #[inline]
    pub fn set_bit_depth(&mut self, depth: BitDepth) {
        self.bit_depth = depth;
    }

    /// Checks if this is a data (non-color) space.
    #[inline]
    pub fn is_data(&self) -> bool {
        self.is_data
    }

    /// Sets whether this is data (non-color) space. Independent of the encoding, as in OCIO
    /// (`isdata` and `encoding` are two keys; a data space need not say `encoding: data`).
    #[inline]
    pub fn set_is_data(&mut self, is_data: bool) {
        self.is_data = is_data;
    }

    /// Returns the reference space type (scene or display).
    #[inline]
    pub fn reference_space_type(&self) -> ReferenceSpaceType {
        self.reference_space_type
    }

    /// Sets the reference space type.
    #[inline]
    pub fn set_reference_space_type(&mut self, reference_space_type: ReferenceSpaceType) {
        self.reference_space_type = reference_space_type;
    }

    /// Returns the equality group.
    #[inline]
    pub fn equality_group(&self) -> &str {
        &self.equality_group
    }

    /// Sets the equality group.
    #[inline]
    pub fn set_equality_group(&mut self, group: impl Into<String>) {
        self.equality_group = group.into();
    }

    /// Returns the interop ID.
    #[inline]
    pub fn interop_id(&self) -> &str {
        &self.interop_id
    }

    /// Sets the interop ID.
    #[inline]
    pub fn set_interop_id(&mut self, interop_id: impl Into<String>) {
        self.interop_id = interop_id.into();
    }

    /// Returns an interchange attribute.
    #[inline]
    pub fn interchange_attribute(&self, name: &str) -> Option<&str> {
        self.interchange_attributes.get(name).map(|s| s.as_str())
    }

    /// Sets an interchange attribute.
    #[inline]
    pub fn set_interchange_attribute(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.interchange_attributes
            .insert(name.into(), value.into());
    }

    /// Removes an interchange attribute.
    #[inline]
    pub fn remove_interchange_attribute(&mut self, name: &str) {
        self.interchange_attributes.remove(name);
    }

    /// Clears interchange attributes.
    #[inline]
    pub fn clear_interchange_attributes(&mut self) {
        self.interchange_attributes.clear();
    }

    /// Returns interchange attribute names.
    #[inline]
    pub fn interchange_attribute_names(&self) -> impl Iterator<Item = &str> {
        self.interchange_attributes.keys().map(|k| k.as_str())
    }

    /// Returns the transform to reference space.
    #[inline]
    pub fn to_reference(&self) -> Option<&Transform> {
        self.to_reference.as_ref()
    }

    /// Returns the transform from reference space.
    #[inline]
    pub fn from_reference(&self) -> Option<&Transform> {
        self.from_reference.as_ref()
    }

    /// Returns the transform to display reference (OCIO v2).
    #[inline]
    pub fn to_display_reference(&self) -> Option<&Transform> {
        self.to_display_reference.as_ref()
    }

    /// Returns the transform from display reference (OCIO v2).
    #[inline]
    pub fn from_display_reference(&self) -> Option<&Transform> {
        self.from_display_reference.as_ref()
    }

    /// Returns a transform by direction.
    pub fn transform(&self, dir: ColorSpaceDirection) -> Option<&Transform> {
        match dir {
            ColorSpaceDirection::ToReference => self.to_reference.as_ref(),
            ColorSpaceDirection::FromReference => self.from_reference.as_ref(),
            ColorSpaceDirection::ToDisplayReference => self.to_display_reference.as_ref(),
            ColorSpaceDirection::FromDisplayReference => self.from_display_reference.as_ref(),
        }
    }

    /// Sets a transform by direction.
    pub fn set_transform(&mut self, dir: ColorSpaceDirection, transform: Option<Transform>) {
        match dir {
            ColorSpaceDirection::ToReference => self.to_reference = transform,
            ColorSpaceDirection::FromReference => self.from_reference = transform,
            ColorSpaceDirection::ToDisplayReference => self.to_display_reference = transform,
            ColorSpaceDirection::FromDisplayReference => self.from_display_reference = transform,
        }
    }

    /// Returns GPU allocation info.
    #[inline]
    pub fn allocation(&self) -> &AllocationInfo {
        &self.allocation
    }

    /// Sets allocation info.
    #[inline]
    pub fn set_allocation(&mut self, alloc: AllocationInfo) {
        self.allocation = alloc;
    }

    /// Returns allocation vars.
    #[inline]
    pub fn allocation_vars(&self) -> &[f32] {
        &self.allocation.vars
    }

    /// Sets allocation vars.
    #[inline]
    pub fn set_allocation_vars(&mut self, vars: impl Into<Vec<f32>>) {
        self.allocation.vars = vars.into();
    }

    /// Checks if a name or alias matches.
    pub fn matches_name(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
            || self.aliases.iter().any(|a| a.eq_ignore_ascii_case(name))
    }
}

/// Builder for constructing color spaces.
#[derive(Debug)]
pub struct ColorSpaceBuilder {
    inner: ColorSpace,
}

impl ColorSpaceBuilder {
    /// Creates a new builder.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            inner: ColorSpace::new(name),
        }
    }

    /// Adds an alias.
    pub fn alias(mut self, alias: impl Into<String>) -> Self {
        self.inner.add_alias(alias);
        self
    }

    /// Adds a category.
    pub fn category(mut self, category: impl Into<String>) -> Self {
        self.inner.categories.push(category.into());
        self
    }

    /// Sets the description.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.inner.description = desc.into();
        self
    }

    /// Sets the family.
    pub fn family_name(mut self, family: impl Into<String>) -> Self {
        self.inner.set_family_name(family);
        self
    }

    /// Sets the family.
    pub fn family(mut self, family: Family) -> Self {
        self.inner.family = family;
        self
    }

    /// Sets the encoding.
    pub fn encoding(mut self, encoding: Encoding) -> Self {
        self.inner.set_encoding(encoding);
        self
    }

    /// Sets the encoding name string.
    pub fn encoding_name(mut self, name: impl Into<String>) -> Self {
        self.inner.set_encoding_name(name);
        self
    }

    /// Sets the bit depth.
    pub fn bit_depth(mut self, depth: BitDepth) -> Self {
        self.inner.bit_depth = depth;
        self
    }

    /// Marks as data (non-color) space.
    pub fn is_data(mut self, is_data: bool) -> Self {
        self.inner.set_is_data(is_data);
        self
    }

    /// Sets reference space type.
    pub fn reference_space_type(mut self, reference_space_type: ReferenceSpaceType) -> Self {
        self.inner.reference_space_type = reference_space_type;
        self
    }

    /// Sets equality group.
    pub fn equality_group(mut self, group: impl Into<String>) -> Self {
        self.inner.equality_group = group.into();
        self
    }

    /// Sets interop ID.
    pub fn interop_id(mut self, interop_id: impl Into<String>) -> Self {
        self.inner.interop_id = interop_id.into();
        self
    }

    /// Sets interchange attribute.
    pub fn interchange_attribute(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.inner
            .interchange_attributes
            .insert(name.into(), value.into());
        self
    }

    /// Sets the transform to reference space.
    pub fn to_reference(mut self, transform: Transform) -> Self {
        self.inner.to_reference = Some(transform);
        self
    }

    /// Sets the transform from reference space.
    pub fn from_reference(mut self, transform: Transform) -> Self {
        self.inner.from_reference = Some(transform);
        self
    }

    /// Sets the transform to display reference (OCIO v2).
    pub fn to_display_reference(mut self, transform: Transform) -> Self {
        self.inner.to_display_reference = Some(transform);
        self
    }

    /// Sets the transform from display reference (OCIO v2).
    pub fn from_display_reference(mut self, transform: Transform) -> Self {
        self.inner.from_display_reference = Some(transform);
        self
    }

    /// Sets allocation info.
    pub fn allocation(mut self, alloc: AllocationInfo) -> Self {
        self.inner.allocation = alloc;
        self
    }

    /// Sets allocation vars.
    pub fn allocation_vars(mut self, vars: impl Into<Vec<f32>>) -> Self {
        self.inner.allocation.vars = vars.into();
        self
    }

    /// Builds the color space.
    pub fn build(self) -> ColorSpace {
        self.inner
    }
}

/// Color space transform direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorSpaceDirection {
    /// To reference space.
    ToReference,
    /// From reference space.
    FromReference,
    /// To display reference space.
    ToDisplayReference,
    /// From display reference space.
    FromDisplayReference,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_colorspace() {
        let cs = ColorSpace::builder("ACEScg")
            .alias("ACES - ACEScg")
            .family(Family::Scene)
            .encoding(Encoding::SceneLinear)
            .description("ACES CG working space")
            .build();

        assert_eq!(cs.name(), "ACEScg");
        assert_eq!(cs.family(), Family::Scene);
        assert_eq!(cs.encoding(), Encoding::SceneLinear);
        assert!(cs.encoding().is_linear());
        assert!(cs.matches_name("acescg"));
        assert!(cs.matches_name("ACES - ACEScg"));
    }

    #[test]
    fn encoding_parse() {
        assert_eq!(Encoding::parse("scene-linear"), Encoding::SceneLinear);
        assert_eq!(Encoding::parse("log"), Encoding::Log);
        assert_eq!(Encoding::parse("data"), Encoding::Data);
    }

    #[test]
    fn data_colorspace() {
        // `isdata` is its own key, independent of `encoding`, as in OCIO.
        let cs = ColorSpace::builder("Raw").is_data(true).build();
        assert!(cs.is_data());
        let cs = ColorSpace::builder("Raw").encoding(Encoding::Data).build();
        assert!(!cs.is_data());
    }

    #[test]
    fn categories() {
        let cs = ColorSpace::builder("ACEScg")
            .category("scene_linear")
            .category("working_space")
            .build();

        assert_eq!(cs.categories().len(), 2);
        assert!(cs.has_category("scene_linear"));
        assert!(cs.has_category("SCENE_LINEAR")); // case-insensitive
        assert!(!cs.has_category("display"));
    }

    #[test]
    fn category_mutation() {
        let mut cs = ColorSpace::new("Test");
        assert_eq!(cs.num_categories(), 0);

        // add_category
        cs.add_category("scene_linear");
        assert_eq!(cs.num_categories(), 1);
        assert!(cs.has_category("scene_linear"));

        // add_category ignores duplicates (case-insensitive)
        cs.add_category("SCENE_LINEAR");
        assert_eq!(cs.num_categories(), 1);

        // category by index
        assert_eq!(cs.category(0), Some("scene_linear"));
        assert_eq!(cs.category(1), None);

        // remove_category (case-insensitive)
        cs.add_category("display");
        assert_eq!(cs.num_categories(), 2);
        cs.remove_category("DISPLAY");
        assert_eq!(cs.num_categories(), 1);
        assert!(!cs.has_category("display"));

        // clear_categories
        cs.add_category("output");
        cs.clear_categories();
        assert_eq!(cs.num_categories(), 0);
    }
}
