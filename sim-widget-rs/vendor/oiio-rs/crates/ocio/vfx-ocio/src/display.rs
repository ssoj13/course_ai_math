//! Display and View management.
//!
//! OCIO organizes output transforms through displays and views:
//! - **Display**: A physical or virtual display device (monitor, projector)
//! - **View**: A viewing condition for that display (SDR, HDR, Raw, etc.)
//!
//! # Example
//!
//! ```
//! use vfx_ocio::{Display, View};
//!
//! let mut display = Display::new("sRGB Monitor");
//! display.add_view(View::new("Film", "sRGB")
//!     .with_look("Show LUT"));
//! display.add_view(View::new("Raw", "Raw"));
//!
//! assert_eq!(display.views().len(), 2);
//! ```

use std::collections::HashMap;

use crate::transform::Transform;

/// A view within a display.
///
/// Views define how to transform from the working space to the display.
/// `use_display_name` is OCIO `View::m_useDisplayName` — `true` means
/// the destination colorspace name is the display's own name. Shared
/// views written as `display_colorspace: <USE_DISPLAY_NAME>` set this
/// flag at parse time.
#[derive(Debug, Clone)]
pub struct View {
    /// View name (e.g., "Film", "Raw", "Log").
    name: String,
    /// Target color space name. Ignored when [`Self::use_display_name`]
    /// is `true`.
    colorspace: String,
    /// `true` when the colorspace should be resolved to the host
    /// display's name. Mirrors OCIO `View::m_useDisplayName`.
    use_display_name: bool,
    /// Optional look to apply.
    looks: Option<String>,
    /// Optional view transform (OCIO v2).
    view_transform: Option<String>,
    /// Rule for matching input files.
    rule: Option<String>,
    /// Description.
    description: String,
}

impl View {
    /// Creates a new view.
    ///
    /// # Arguments
    ///
    /// * `name` - View name
    /// * `colorspace` - Target color space name
    pub fn new(name: impl Into<String>, colorspace: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            colorspace: colorspace.into(),
            use_display_name: false,
            looks: None,
            view_transform: None,
            rule: None,
            description: String::new(),
        }
    }

    /// Marks this view as one whose destination colorspace equals the
    /// hosting display's name. Mirrors OCIO C++
    /// `View::useDisplayNameForColorspace`.
    pub fn with_use_display_name(mut self, flag: bool) -> Self {
        self.use_display_name = flag;
        self
    }

    /// Adds a look to apply.
    pub fn with_look(mut self, looks: impl Into<String>) -> Self {
        self.looks = Some(looks.into());
        self
    }

    /// Sets the view transform (OCIO v2).
    pub fn with_view_transform(mut self, vt: impl Into<String>) -> Self {
        self.view_transform = Some(vt.into());
        self
    }

    /// Sets a rule.
    pub fn with_rule(mut self, rule: impl Into<String>) -> Self {
        self.rule = Some(rule.into());
        self
    }

    /// Sets description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Returns the view name.
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the target color space.
    ///
    /// When [`Self::use_display_name`] is `true` this returns the raw
    /// stored value (usually empty for shared views); call
    /// [`Self::effective_colorspace`] to get the resolved name when
    /// you have the hosting display.
    #[inline]
    pub fn colorspace(&self) -> &str {
        &self.colorspace
    }

    /// Returns the `use_display_name` flag.
    #[inline]
    pub fn use_display_name(&self) -> bool {
        self.use_display_name
    }

    /// Returns the colorspace this view targets, resolving the
    /// `<USE_DISPLAY_NAME>` semantic when needed. Returns the
    /// passed-in `display` name when [`Self::use_display_name`] is
    /// `true`, otherwise the stored [`Self::colorspace`].
    #[inline]
    pub fn effective_colorspace<'a>(&'a self, display: &'a str) -> &'a str {
        if self.use_display_name {
            display
        } else {
            &self.colorspace
        }
    }

    /// Returns the look(s) to apply.
    #[inline]
    pub fn looks(&self) -> Option<&str> {
        self.looks.as_deref()
    }

    /// Returns the view transform name.
    #[inline]
    pub fn view_transform(&self) -> Option<&str> {
        self.view_transform.as_deref()
    }

    /// Returns the rule.
    #[inline]
    pub fn rule(&self) -> Option<&str> {
        self.rule.as_deref()
    }

    /// Returns the description.
    #[inline]
    pub fn description(&self) -> &str {
        &self.description
    }
}

/// A display device configuration.
///
/// Contains multiple views for different viewing conditions.
#[derive(Debug, Clone)]
pub struct Display {
    /// Display name (e.g., "sRGB", "Rec.709", "DCI-P3").
    name: String,
    /// Available views for this display.
    views: Vec<View>,
    /// References to shared views (OCIO v2.3+).
    shared_view_refs: Vec<String>,
    /// Default view name.
    default_view: Option<String>,
    /// Temporary display flag.
    temporary: bool,
}

impl Display {
    /// Creates a new display.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            views: Vec::new(),
            shared_view_refs: Vec::new(),
            default_view: None,
            temporary: false,
        }
    }

    /// Returns the display name.
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns whether the display is temporary.
    #[inline]
    pub fn is_temporary(&self) -> bool {
        self.temporary
    }

    /// Sets whether the display is temporary.
    pub fn set_temporary(&mut self, temporary: bool) {
        self.temporary = temporary;
    }

    /// Adds a view to this display.
    pub fn add_view(&mut self, view: View) {
        if self.default_view.is_none() {
            self.default_view = Some(view.name.clone());
        }
        self.views.push(view);
    }

    /// Adds a view (builder pattern).
    pub fn with_view(mut self, view: View) -> Self {
        self.add_view(view);
        self
    }

    /// Sets the default view.
    pub fn set_default_view(&mut self, name: impl Into<String>) {
        self.default_view = Some(name.into());
    }

    /// Returns all views.
    #[inline]
    pub fn views(&self) -> &[View] {
        &self.views
    }

    /// Returns all shared view references.
    #[inline]
    pub fn shared_view_refs(&self) -> &[String] {
        &self.shared_view_refs
    }

    /// Adds a shared view reference.
    pub fn add_shared_view_ref(&mut self, name: impl Into<String>) {
        self.shared_view_refs.push(name.into());
    }

    /// Removes a shared view reference by name.
    pub fn remove_shared_view_ref(&mut self, name: &str) -> bool {
        if let Some(index) = self.shared_view_refs.iter().position(|v| v == name) {
            self.shared_view_refs.remove(index);
            return true;
        }
        false
    }

    /// Clears shared view references.
    pub fn clear_shared_view_refs(&mut self) {
        self.shared_view_refs.clear();
    }

    /// Gets a view by name.
    pub fn view(&self, name: &str) -> Option<&View> {
        self.views
            .iter()
            .find(|v| v.name.eq_ignore_ascii_case(name))
    }

    /// Checks if a view exists by name.
    pub fn has_view(&self, name: &str) -> bool {
        self.view(name).is_some()
    }

    /// Checks if a shared view reference exists.
    pub fn has_shared_view_ref(&self, name: &str) -> bool {
        self.shared_view_refs.iter().any(|v| v == name)
    }

    /// Returns the default view name.
    #[inline]
    pub fn default_view(&self) -> Option<&str> {
        self.default_view.as_deref()
    }

    /// Returns view names.
    pub fn view_names(&self) -> impl Iterator<Item = &str> {
        self.views.iter().map(|v| v.name.as_str())
    }

    /// Removes a view by name.
    pub fn remove_view(&mut self, name: &str) -> Option<View> {
        if let Some(index) = self
            .views
            .iter()
            .position(|v| v.name.eq_ignore_ascii_case(name))
        {
            let removed = self.views.remove(index);
            if let Some(default) = self.default_view.as_ref()
                && default.eq_ignore_ascii_case(name)
            {
                self.default_view = self.views.first().map(|v| v.name.clone());
            }
            return Some(removed);
        }
        None
    }

    /// Clears all views.
    pub fn clear_views(&mut self) {
        self.views.clear();
        self.default_view = None;
    }
}

/// Reference space type for view transforms (OCIO v2).
///
/// Determines which reference space a view transform operates in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReferenceSpaceType {
    /// Scene-referred (ACES AP0, etc.) - default for most view transforms.
    #[default]
    Scene,
    /// Display-referred (for display-to-display transforms).
    Display,
}

/// Direction selector for [`ViewTransform::transform`] /
/// [`ViewTransform::set_transform`]. Mirrors OCIO C++
/// `enum ViewTransformDirection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewTransformDirection {
    /// `VIEWTRANSFORM_DIR_FROM_REFERENCE` — applied after the
    /// reference colorspace.
    FromReference,
    /// `VIEWTRANSFORM_DIR_TO_REFERENCE` — inverse, applied to bring
    /// data back into the reference colorspace.
    ToReference,
}

/// View transform definition (OCIO v2).
///
/// View transforms are shared transforms that can be reused across views.
/// They operate in either scene-referred or display-referred space.
#[derive(Debug, Clone)]
pub struct ViewTransform {
    /// Name.
    name: String,
    /// Family (for categorization).
    family: String,
    /// Description.
    description: String,
    /// Reference space type (scene or display).
    reference_space_type: ReferenceSpaceType,
    /// Transform from scene reference.
    from_scene_reference: Option<Transform>,
    /// Transform to scene reference.
    to_scene_reference: Option<Transform>,
    /// Transform from display reference.
    from_display_reference: Option<Transform>,
    /// Transform to display reference.
    to_display_reference: Option<Transform>,
    /// Interchange attributes (OCIO
    /// `ViewTransform::m_interchangeAttributes`). Typically holds
    /// `amf_transform_ids` entries.
    interchange_attributes: HashMap<String, String>,
    /// Categories (OCIO `ViewTransform::getCategory`).
    categories: Vec<String>,
}

impl ViewTransform {
    /// Creates a new scene-referred view transform.
    pub fn new(name: impl Into<String>) -> Self {
        Self::with_reference_space(name, ReferenceSpaceType::Scene)
    }

    /// Creates a view transform with explicit reference space type.
    pub fn with_reference_space(
        name: impl Into<String>,
        reference_space: ReferenceSpaceType,
    ) -> Self {
        Self {
            name: name.into(),
            family: String::new(),
            description: String::new(),
            reference_space_type: reference_space,
            from_scene_reference: None,
            to_scene_reference: None,
            from_display_reference: None,
            to_display_reference: None,
            interchange_attributes: HashMap::new(),
            categories: Vec::new(),
        }
    }

    /// Returns the categories.
    #[inline]
    pub fn categories(&self) -> &[String] {
        &self.categories
    }

    /// Adds a category (ignored when already present).
    pub fn add_category(&mut self, category: impl Into<String>) {
        let category = category.into();
        if !self.categories.contains(&category) {
            self.categories.push(category);
        }
    }

    /// Returns an interchange attribute by key (OCIO
    /// `ViewTransform::getInterchangeAttribute`).
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

    /// Returns interchange attribute names.
    #[inline]
    pub fn interchange_attribute_names(&self) -> impl Iterator<Item = &str> {
        self.interchange_attributes.keys().map(|k| k.as_str())
    }

    /// Removes an interchange attribute.
    #[inline]
    pub fn remove_interchange_attribute(&mut self, name: &str) {
        self.interchange_attributes.remove(name);
    }

    /// Clears all interchange attributes.
    #[inline]
    pub fn clear_interchange_attributes(&mut self) {
        self.interchange_attributes.clear();
    }

    /// Returns the name.
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Sets the name.
    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    /// Returns the family.
    #[inline]
    pub fn family(&self) -> &str {
        &self.family
    }

    /// Sets the family.
    pub fn with_family(mut self, family: impl Into<String>) -> Self {
        self.family = family.into();
        self
    }

    /// Sets the family.
    pub fn set_family(&mut self, family: impl Into<String>) {
        self.family = family.into();
    }

    /// Returns the description.
    #[inline]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Sets the description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Sets the description.
    pub fn set_description(&mut self, desc: impl Into<String>) {
        self.description = desc.into();
    }

    /// Returns the reference space type.
    #[inline]
    pub fn reference_space_type(&self) -> ReferenceSpaceType {
        self.reference_space_type
    }

    /// Sets the reference space type.
    pub fn set_reference_space_type(&mut self, reference_space: ReferenceSpaceType) {
        self.reference_space_type = reference_space;
    }

    /// Sets transform from scene reference.
    pub fn with_from_scene_reference(mut self, t: Transform) -> Self {
        self.from_scene_reference = Some(t);
        self
    }

    /// Sets transform from scene reference.
    pub fn set_from_scene_reference(&mut self, t: Option<Transform>) {
        self.from_scene_reference = t;
    }

    /// Sets transform to scene reference.
    pub fn with_to_scene_reference(mut self, t: Transform) -> Self {
        self.to_scene_reference = Some(t);
        self
    }

    /// Sets transform to scene reference.
    pub fn set_to_scene_reference(&mut self, t: Option<Transform>) {
        self.to_scene_reference = t;
    }

    /// Sets transform from display reference.
    pub fn with_from_display_reference(mut self, t: Transform) -> Self {
        self.from_display_reference = Some(t);
        self
    }

    /// Sets transform from display reference.
    pub fn set_from_display_reference(&mut self, t: Option<Transform>) {
        self.from_display_reference = t;
    }

    /// Sets transform to display reference.
    pub fn with_to_display_reference(mut self, t: Transform) -> Self {
        self.to_display_reference = Some(t);
        self
    }

    /// Sets transform to display reference.
    pub fn set_to_display_reference(&mut self, t: Option<Transform>) {
        self.to_display_reference = t;
    }

    /// Gets transform from scene reference.
    #[inline]
    pub fn from_scene_reference(&self) -> Option<&Transform> {
        self.from_scene_reference.as_ref()
    }

    /// Gets transform to scene reference.
    #[inline]
    pub fn to_scene_reference(&self) -> Option<&Transform> {
        self.to_scene_reference.as_ref()
    }

    /// Gets transform from display reference.
    #[inline]
    pub fn from_display_reference(&self) -> Option<&Transform> {
        self.from_display_reference.as_ref()
    }

    /// Gets transform to display reference.
    #[inline]
    pub fn to_display_reference(&self) -> Option<&Transform> {
        self.to_display_reference.as_ref()
    }

    /// OCIO `ViewTransform::getTransform(ViewTransformDirection)`
    /// equivalent — picks the right one of the four stored
    /// transforms based on the active reference space and the
    /// requested direction.
    pub fn transform(&self, direction: ViewTransformDirection) -> Option<&Transform> {
        match (self.reference_space_type, direction) {
            (ReferenceSpaceType::Scene, ViewTransformDirection::FromReference) => {
                self.from_scene_reference.as_ref()
            }
            (ReferenceSpaceType::Scene, ViewTransformDirection::ToReference) => {
                self.to_scene_reference.as_ref()
            }
            (ReferenceSpaceType::Display, ViewTransformDirection::FromReference) => {
                self.from_display_reference.as_ref()
            }
            (ReferenceSpaceType::Display, ViewTransformDirection::ToReference) => {
                self.to_display_reference.as_ref()
            }
        }
    }

    /// OCIO `ViewTransform::setTransform(transform, direction)`
    /// equivalent. Stores the transform in the slot keyed by the
    /// active reference space and direction.
    pub fn set_transform(&mut self, transform: Transform, direction: ViewTransformDirection) {
        match (self.reference_space_type, direction) {
            (ReferenceSpaceType::Scene, ViewTransformDirection::FromReference) => {
                self.from_scene_reference = Some(transform);
            }
            (ReferenceSpaceType::Scene, ViewTransformDirection::ToReference) => {
                self.to_scene_reference = Some(transform);
            }
            (ReferenceSpaceType::Display, ViewTransformDirection::FromReference) => {
                self.from_display_reference = Some(transform);
            }
            (ReferenceSpaceType::Display, ViewTransformDirection::ToReference) => {
                self.to_display_reference = Some(transform);
            }
        }
    }
}

/// Collection of displays.
#[derive(Debug, Clone, Default)]
pub struct DisplayManager {
    /// All displays.
    displays: Vec<Display>,
    /// Default display name.
    default_display: Option<String>,
    /// View transforms (OCIO v2).
    view_transforms: Vec<ViewTransform>,
}

impl DisplayManager {
    /// Creates an empty display manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a display.
    pub fn add_display(&mut self, display: Display) {
        if self.default_display.is_none() {
            self.default_display = Some(display.name.clone());
        }
        self.displays.push(display);
    }

    /// Sets the default display.
    pub fn set_default_display(&mut self, name: impl Into<String>) {
        self.default_display = Some(name.into());
    }

    /// Returns all displays.
    #[inline]
    pub fn displays(&self) -> &[Display] {
        &self.displays
    }

    /// Gets a display by name.
    pub fn display(&self, name: &str) -> Option<&Display> {
        self.displays
            .iter()
            .find(|d| d.name.eq_ignore_ascii_case(name))
    }

    /// Gets mutable display by name.
    pub fn display_mut(&mut self, name: &str) -> Option<&mut Display> {
        self.displays
            .iter_mut()
            .find(|d| d.name.eq_ignore_ascii_case(name))
    }

    /// Returns the default display name.
    #[inline]
    pub fn default_display(&self) -> Option<&str> {
        self.default_display.as_deref()
    }

    /// Returns display names.
    pub fn display_names(&self) -> impl Iterator<Item = &str> {
        self.displays.iter().map(|d| d.name.as_str())
    }

    /// Adds a view transform.
    pub fn add_view_transform(&mut self, vt: ViewTransform) {
        self.view_transforms.push(vt);
    }

    /// Gets a view transform by name.
    pub fn view_transform(&self, name: &str) -> Option<&ViewTransform> {
        self.view_transforms
            .iter()
            .find(|vt| vt.name.eq_ignore_ascii_case(name))
    }

    /// Returns all view transforms.
    #[inline]
    pub fn view_transforms(&self) -> &[ViewTransform] {
        &self.view_transforms
    }

    /// Removes a display by name.
    pub fn remove_display(&mut self, name: &str) {
        self.displays.retain(|d| !d.name.eq_ignore_ascii_case(name));
    }

    /// Clears all displays.
    pub fn clear_displays(&mut self) {
        self.displays.clear();
        self.default_display = None;
    }

    /// Returns the number of displays.
    #[inline]
    pub fn len(&self) -> usize {
        self.displays.len()
    }

    /// Checks if there are no displays.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.displays.is_empty()
    }

    /// Returns the number of view transforms.
    #[inline]
    pub fn num_view_transforms(&self) -> usize {
        self.view_transforms.len()
    }

    /// Removes a view transform by name.
    pub fn remove_view_transform(&mut self, name: &str) -> Option<ViewTransform> {
        if let Some(index) = self
            .view_transforms
            .iter()
            .position(|vt| vt.name.eq_ignore_ascii_case(name))
        {
            return Some(self.view_transforms.remove(index));
        }
        None
    }

    /// Clears all view transforms.
    pub fn clear_view_transforms(&mut self) {
        self.view_transforms.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_display() {
        let mut display = Display::new("sRGB Monitor");
        display.add_view(View::new("Film", "sRGB"));
        display.add_view(View::new("Raw", "Raw"));

        assert_eq!(display.name(), "sRGB Monitor");
        assert_eq!(display.views().len(), 2);
        assert_eq!(display.default_view(), Some("Film"));
    }

    #[test]
    fn view_with_look() {
        let view = View::new("Graded", "sRGB").with_look("Show LUT");

        assert_eq!(view.looks(), Some("Show LUT"));
    }

    #[test]
    fn display_manager() {
        let mut mgr = DisplayManager::new();

        let mut srgb = Display::new("sRGB");
        srgb.add_view(View::new("Film", "sRGB"));
        mgr.add_display(srgb);

        let mut rec709 = Display::new("Rec.709");
        rec709.add_view(View::new("Video", "Rec709"));
        mgr.add_display(rec709);

        assert_eq!(mgr.displays().len(), 2);
        assert_eq!(mgr.default_display(), Some("sRGB"));
        assert!(mgr.display("rec.709").is_some());
    }
}
