//! Data model for the keyframe panel.
//!
//! The model is a flat, app-agnostic snapshot the caller rebuilds (or borrows)
//! each frame. It deliberately uses plain scalars (`f32`) and a `u64` id rather
//! than any engine value type or `Uuid`, so the widget never needs to know about
//! the host application. All edits flow back out as [`KeyAction`] intents; the
//! widget never mutates the model in place.

/// What kind of value a [`Property`] holds, used only to pick the right inline
/// value editor and to clamp/scale drag interactions. Keyframe *values* on the
/// track are always stored as plain `f32` (see [`Key`]); for multi-component
/// properties (`Vec2`/`Vec3`/`Vec4`/`Color`) the keyframe value is treated as a
/// single animated magnitude and only the scalar `Property::value` components are
/// edited inline. This keeps the generic track model simple while still covering
/// the common "scalar over time" case faithfully.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PropKind {
    /// A boolean toggle (checkbox); keyframe value 0.0 = false, otherwise true.
    Bool,
    /// A single `f32` scalar (drag value), optionally bounded — see [`Property::range`].
    Float,
    /// Two `f32` components (e.g. position/scale X,Y).
    Vec2,
    /// Three `f32` components (e.g. RGB / XYZ).
    Vec3,
    /// Four `f32` components (e.g. RGBA / XYZW).
    Vec4,
}

impl PropKind {
    /// Number of editable scalar components for this kind.
    pub fn arity(self) -> usize {
        match self {
            PropKind::Bool | PropKind::Float => 1,
            PropKind::Vec2 => 2,
            PropKind::Vec3 => 3,
            PropKind::Vec4 => 4,
        }
    }

    /// Short per-axis labels used by the multi-component editor.
    pub fn axis_labels(self) -> &'static [&'static str] {
        match self {
            PropKind::Vec2 => &["X", "Y"],
            PropKind::Vec3 => &["X", "Y", "Z"],
            PropKind::Vec4 => &["X", "Y", "Z", "W"],
            _ => &[],
        }
    }
}

/// A single keyframe: a value sampled at a point in time.
///
/// `time` is in the same unit as [`KeyframePanel::time`] (seconds or frames — the
/// caller decides; the widget treats it as an opaque `f64` mapped onto the ruler).
/// `value` is a plain `f32` so the model stays free of app value types.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Key {
    /// Time of this keyframe, in panel time units.
    pub time: f64,
    /// Sampled value at `time`.
    pub value: f32,
}

impl Key {
    /// Convenience constructor.
    pub fn new(time: f64, value: f32) -> Self {
        Self { time, value }
    }
}

/// One animatable property row: a name, a current/static value, an optional
/// inclusive range for the inline editor, and its keyframe track.
///
/// `value` holds the up-to-four scalar components used by the inline editor (only
/// `kind.arity()` of them are meaningful). For animated properties the caller
/// typically fills `value[0]` with the current sampled value at the playhead so
/// the inline editor and the track agree.
#[derive(Debug, Clone)]
pub struct Property {
    /// Stable caller-side identifier (e.g. a hashed path). Used to address edits.
    pub id: u64,
    /// Display name shown in the left column.
    pub name: String,
    /// Value kind — selects the inline editor and component count.
    pub kind: PropKind,
    /// Current scalar components (`kind.arity()` meaningful). `[0]` is the value
    /// echoed onto the keyframe track for single-scalar kinds.
    pub value: [f32; 4],
    /// Optional inclusive `(min, max)` bound for the inline drag/slider editor.
    /// `None` => unbounded drag value.
    pub range: Option<(f32, f32)>,
    /// Drag speed / slider step for the inline editor (defaults applied if 0.0).
    pub speed: f32,
    /// Keyframes for this property, expected sorted ascending by `time`. The
    /// widget tolerates unsorted input but renders/reports indices as given.
    pub keys: Vec<Key>,
}

impl Property {
    /// Minimal constructor for a scalar float property with no keyframes.
    pub fn float(id: u64, name: impl Into<String>, value: f32) -> Self {
        Self {
            id,
            name: name.into(),
            kind: PropKind::Float,
            value: [value, 0.0, 0.0, 0.0],
            range: None,
            speed: 0.1,
            keys: Vec::new(),
        }
    }

    /// Effective drag speed, falling back to a sensible default when unset.
    pub fn eff_speed(&self) -> f32 {
        if self.speed > 0.0 { self.speed } else { 0.1 }
    }
}

/// The full panel snapshot passed to [`crate::show`] each frame.
///
/// The caller owns this; the widget reads it and returns [`KeyAction`]s. `time`
/// is the playhead position; `start`/`end` define the visible time window mapped
/// onto the ruler.
#[derive(Debug, Clone)]
pub struct KeyframePanel {
    /// Properties to display, top to bottom.
    pub properties: Vec<Property>,
    /// Current playhead time (panel units).
    pub time: f64,
    /// Start of the visible time window (left edge of the ruler).
    pub start: f64,
    /// End of the visible time window (right edge of the ruler).
    pub end: f64,
}

impl Default for KeyframePanel {
    fn default() -> Self {
        Self {
            properties: Vec::new(),
            time: 0.0,
            start: 0.0,
            end: 1.0,
        }
    }
}

/// Edit intents returned by [`crate::show`]. The caller applies these to its own
/// data; the widget is purely a view + interaction reporter.
#[derive(Debug, Clone, PartialEq)]
pub enum KeyAction {
    /// Add a keyframe to property `prop` at `time` with `value`.
    AddKey { prop: u64, time: f64, value: f32 },
    /// Move keyframe `index` of property `prop` to a new `time`/`value`.
    MoveKey {
        prop: u64,
        index: usize,
        time: f64,
        value: f32,
    },
    /// Delete keyframe `index` of property `prop`.
    DeleteKey { prop: u64, index: usize },
    /// Set the static/current value of property `prop`. `component` selects which
    /// scalar of a multi-component value changed (0 for scalar kinds).
    SetValue {
        prop: u64,
        component: usize,
        value: f32,
    },
    /// Move the playhead to `time` (ruler click/drag).
    Seek { time: f64 },
}
