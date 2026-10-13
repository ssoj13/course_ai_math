//! Port definitions ([`PortDef`]) and renderer-agnostic pin visuals ([`PinVisual`]).
//!
//! A port is a name + a value-type name. A BUILTIN port's default comes from the builtin seed
//! table ([`PortDef::typed`]); a port of a HOST-registered type must be built through
//! [`crate::ValueTypes::port_def`], which reads the type's registered default.
//! [`pin_visual_for_type`] maps value type names to colour/shape for `nodes-view`
//! SDF pins; [`wire_color_for_types`] picks wire tint from connected port types.

use crate::box_model::BoxValue;
use crate::value_types::ValueTypes;

/// Visual pin shape (no egui dependency).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PinShape {
    #[default]
    Circle,
    Square,
    Triangle,
    /// Square rotated 45° (diamond).
    Diamond,
}

/// RGBA pin colour for renderers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PinVisual {
    pub color: [f32; 4],
    pub shape: PinShape,
    /// Optional outline; when `None`, renderer uses a default dark stroke.
    pub stroke: Option<([f32; 4], f32)>,
}

impl PinVisual {
    pub const FLOAT: Self = Self {
        color: [0.35, 0.75, 1.0, 1.0],
        shape: PinShape::Circle,
        stroke: None,
    };
    pub const INT: Self = Self {
        color: [0.5, 0.85, 0.45, 1.0],
        shape: PinShape::Square,
        stroke: None,
    };
    pub const BOOL: Self = Self {
        color: [1.0, 0.55, 0.2, 1.0],
        shape: PinShape::Triangle,
        stroke: None,
    };
    pub const STRING: Self = Self {
        color: [0.95, 0.85, 0.35, 1.0],
        shape: PinShape::Circle,
        stroke: None,
    };
    pub const VEC3: Self = Self {
        color: [0.75, 0.55, 1.0, 1.0],
        shape: PinShape::Diamond,
        stroke: None,
    };
    pub const COLOR: Self = Self {
        color: [1.0, 0.4, 0.7, 1.0],
        shape: PinShape::Diamond,
        stroke: None,
    };
    pub const ANY: Self = Self {
        color: [0.55, 0.55, 0.55, 1.0],
        shape: PinShape::Circle,
        stroke: None,
    };
    /// Map (bag): teal diamond, distinct from all scalar/vector types.
    pub const MAP: Self = Self {
        color: [0.2, 0.85, 0.75, 1.0],
        shape: PinShape::Diamond,
        stroke: None,
    };
    /// Image (RGBA8 buffer): warm amber square, distinct from the scalar/vector/map pins.
    pub const IMAGE: Self = Self {
        color: [0.95, 0.60, 0.30, 1.0],
        shape: PinShape::Square,
        stroke: None,
    };
}

/// Resolve pin visual for a port type name.
pub fn pin_visual_for_type(type_name: &str, value_types: &ValueTypes) -> PinVisual {
    value_types
        .get(type_name)
        .map(|i| i.visual)
        .unwrap_or(PinVisual::FLOAT)
}

/// Wire colour: mostly output type, lightly mixed with input type.
pub fn wire_color_for_types(from_type: &str, to_type: &str, value_types: &ValueTypes) -> [f32; 4] {
    const FROM_W: f32 = 0.75;
    const TO_W: f32 = 0.25;
    let a = pin_visual_for_type(from_type, value_types).color;
    let b = pin_visual_for_type(to_type, value_types).color;
    [
        a[0] * FROM_W + b[0] * TO_W,
        a[1] * FROM_W + b[1] * TO_W,
        a[2] * FROM_W + b[2] * TO_W,
        a[3] * FROM_W + b[3] * TO_W,
    ]
}

pub type PortTypeName = &'static str;

/// A named port with a value-type name and the initial value written into the attr on
/// instantiation.
#[derive(Clone)]
pub struct PortDef {
    pub name: String,
    pub port_type: String,
    /// Initial value written into the attr on instantiation.
    pub default: BoxValue,
}

impl PortDef {
    /// A port with an explicit default. `port_type` is a plain `&str` (not `&'static str`) so a
    /// host type whose name is only known at runtime can declare ports.
    ///
    /// Whether an OUTPUT's value is saved is NOT a property of the port — it follows from the node
    /// type's [`crate::NodeKind`] (a Job's outputs record an effect that already happened, so they are
    /// persisted; a Dataflow node's are cheaper to recompute than to store, so they are blanked).
    pub fn new(name: &str, port_type: &str, default: BoxValue) -> Self {
        Self {
            name: name.into(),
            port_type: port_type.into(),
            default,
        }
    }

    /// Port of a **BUILTIN** type, defaulted from the builtin seed table.
    ///
    /// For a HOST-registered type use [`crate::ValueTypes::port_def`] instead — this function has
    /// no registry to consult and would silently hand a custom type a `Null` default.
    pub fn typed(name: &str, port_type: PortTypeName) -> Self {
        Self::new(name, port_type, crate::values::builtin_default(port_type))
    }

    pub fn float(name: &str, default: f64) -> Self {
        Self::new(name, "Float", BoxValue::F64(default))
    }

    pub fn int(name: &str, default: i64) -> Self {
        Self::new(name, "Int", BoxValue::I64(default))
    }

    pub fn bool(name: &str, default: bool) -> Self {
        Self::new(name, "Bool", BoxValue::Bool(default))
    }

    pub fn string(name: &str) -> Self {
        Self::new(name, "String", BoxValue::String(String::new()))
    }

    pub fn vec3(name: &str) -> Self {
        Self::typed(name, "Vec3")
    }

    pub fn color(name: &str) -> Self {
        Self::typed(name, "Color")
    }

    pub fn any(name: &str) -> Self {
        Self::new(name, "Any", BoxValue::Null)
    }
}
