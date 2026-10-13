use std::collections::BTreeMap;
use std::fmt;
use std::ops::{Index, IndexMut};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValueKind {
    Null,
    Bool,
    I64,
    F64,
    String,
    Array,
    Object,
    Vec3,
    Vec4,
    Mat3,
    Mat4,
    Color,
    /// Compact typed binary buffer (bulk numeric payload) — see [`BoxValue::Buffer`].
    Buffer,
}

/// Element type of a [`BoxValue::Buffer`]. Mirrors the `safetensors` dtype set so
/// a buffer round-trips 1:1 through a `.safetensors` checkpoint. box-rs treats the
/// bytes as opaque (it never interprets `F16`/`BF16`), so no `half` dependency is
/// needed here — a consumer that wants f16 compute converts on its own side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DType {
    Bool,
    U8,
    I8,
    U16,
    I16,
    F16,
    Bf16,
    U32,
    I32,
    F32,
    U64,
    I64,
    F64,
}

impl DType {
    /// Size of one element in bytes.
    pub fn size(self) -> usize {
        match self {
            DType::Bool | DType::U8 | DType::I8 => 1,
            DType::U16 | DType::I16 | DType::F16 | DType::Bf16 => 2,
            DType::U32 | DType::I32 | DType::F32 => 4,
            DType::U64 | DType::I64 | DType::F64 => 8,
        }
    }

    /// Lowercase tag used in the JSON encoding (`{"$buffer":{"dtype":"f32",…}}`)
    /// and as the safetensors dtype string.
    pub fn tag(self) -> &'static str {
        match self {
            DType::Bool => "bool",
            DType::U8 => "u8",
            DType::I8 => "i8",
            DType::U16 => "u16",
            DType::I16 => "i16",
            DType::F16 => "f16",
            DType::Bf16 => "bf16",
            DType::U32 => "u32",
            DType::I32 => "i32",
            DType::F32 => "f32",
            DType::U64 => "u64",
            DType::I64 => "i64",
            DType::F64 => "f64",
        }
    }

    pub fn from_tag(s: &str) -> Option<Self> {
        Some(match s {
            "bool" => DType::Bool,
            "u8" => DType::U8,
            "i8" => DType::I8,
            "u16" => DType::U16,
            "i16" => DType::I16,
            "f16" => DType::F16,
            "bf16" => DType::Bf16,
            "u32" => DType::U32,
            "i32" => DType::I32,
            "f32" => DType::F32,
            "u64" => DType::U64,
            "i64" => DType::I64,
            "f64" => DType::F64,
            _ => return None,
        })
    }
}

/// Refcounted immutable byte payload of a [`BoxValue::Buffer`]. `Arc<[u8]>` so
/// clone/freeze SHARE the bytes (cheap) — a frozen snapshot must not deep-copy MB.
/// A newtype only so its `Debug` prints `[N bytes]` instead of dumping every byte
/// (BoxValue derives `Debug`, and error paths format values with `{:?}`).
#[derive(Clone, PartialEq, Eq)]
pub struct BufData(pub Arc<[u8]>);

impl fmt::Debug for BufData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{} bytes]", self.0.len())
    }
}

impl std::ops::Deref for BufData {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.0
    }
}

/// Map interior of a [`BoxValue::Object`]. See [`BoxValue`] on why it is shared.
pub type BoxMap = BTreeMap<String, BoxValue>;
/// List interior of a [`BoxValue::Array`]. See [`BoxValue`] on why it is shared.
pub type BoxList = Vec<BoxValue>;

/// Dynamic value. Scalars/`Array`/`Object` serialize as natural JSON; typed
/// aggregates serialize **tagged** (`{"$vec3":[..]}`, `{"$color":[..]}`, …) so
/// they never collide with a plain JSON array. `Color` is RGBA and is kept
/// distinct from `Vec4` (colour-space conversions + alpha semantics).
///
/// # The containers are shared, not copied
///
/// `Array` and `Object` hold their interior behind an [`Arc`], so **cloning a
/// value of any size is a refcount bump** rather than a deep copy of the tree.
/// Mutation goes through [`Arc::make_mut`]: a value with one owner is edited in
/// place, and one with several is copied first. The observable semantics are
/// therefore unchanged — every `BoxValue` still behaves as if it owned its data
/// — only the cost moved from "every clone" to "the first write after a clone".
///
/// This is what makes a dataflow graph affordable: a payload that travels a
/// wire, lands in a bus slot and is read back by the next node used to be
/// deep-copied at each hop, which is O(tree) work per hop for data that nobody
/// modified.
///
/// The serialized form is untouched: `Serialize` goes through
/// [`BoxValue::to_json_value`], which never sees the `Arc`.
#[derive(Debug, Clone, Default)]
pub enum BoxValue {
    #[default]
    Null,
    Bool(bool),
    I64(i64),
    F64(f64),
    String(String),
    Array(Arc<BoxList>),
    Object(Arc<BoxMap>),
    Vec3([f64; 3]),
    Vec4([f64; 4]),
    Mat3([f64; 9]),
    Mat4([f64; 16]),
    Color([f64; 4]),
    /// Compact typed binary buffer for bulk numeric payloads (point clouds,
    /// keypoints, descriptors, …) that would be absurd as `Array(Vec<BoxValue>)`.
    /// The bytes are opaque + immutable (`Arc`-shared). Serializes to JSON as
    /// `{"$buffer":{"dtype":"f32","shape":[N,3],"data":[u8,…]}}` (full fidelity,
    /// verbose — for the compact binary form use the `safetensors` backend).
    Buffer {
        dtype: DType,
        shape: Vec<usize>,
        data: BufData,
    },
}

/// Equality is by CONTENT, as it was before the interiors became shared.
///
/// The pointer check is only a shortcut for the case sharing makes common (two
/// handles on the same tree); it is never the definition, so a value compares
/// equal to a deep copy of itself exactly as it always did.
impl PartialEq for BoxValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Null, Self::Null) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::I64(a), Self::I64(b)) => a == b,
            (Self::F64(a), Self::F64(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Array(a), Self::Array(b)) => Arc::ptr_eq(a, b) || a == b,
            (Self::Object(a), Self::Object(b)) => Arc::ptr_eq(a, b) || a == b,
            (Self::Vec3(a), Self::Vec3(b)) => a == b,
            (Self::Vec4(a), Self::Vec4(b)) => a == b,
            (Self::Mat3(a), Self::Mat3(b)) => a == b,
            (Self::Mat4(a), Self::Mat4(b)) => a == b,
            (Self::Color(a), Self::Color(b)) => a == b,
            (
                Self::Buffer {
                    dtype: d1,
                    shape: s1,
                    data: b1,
                },
                Self::Buffer {
                    dtype: d2,
                    shape: s2,
                    data: b2,
                },
            ) => d1 == d2 && s1 == s2 && b1 == b2,
            _ => false,
        }
    }
}

impl BoxValue {
    pub fn object() -> Self {
        Self::Object(Arc::new(BoxMap::new()))
    }

    /// An `Object` from an owned map.
    ///
    /// Prefer this over naming the variant: it is the one line that has to
    /// change if the interior representation moves again.
    pub fn obj(map: BoxMap) -> Self {
        Self::Object(Arc::new(map))
    }

    /// An `Array` from owned items.
    pub fn array(items: impl Into<BoxList>) -> Self {
        Self::Array(Arc::new(items.into()))
    }

    pub fn kind(&self) -> ValueKind {
        match self {
            Self::Null => ValueKind::Null,
            Self::Bool(_) => ValueKind::Bool,
            Self::I64(_) => ValueKind::I64,
            Self::F64(_) => ValueKind::F64,
            Self::String(_) => ValueKind::String,
            Self::Array(_) => ValueKind::Array,
            Self::Object(_) => ValueKind::Object,
            Self::Vec3(_) => ValueKind::Vec3,
            Self::Vec4(_) => ValueKind::Vec4,
            Self::Mat3(_) => ValueKind::Mat3,
            Self::Mat4(_) => ValueKind::Mat4,
            Self::Color(_) => ValueKind::Color,
            Self::Buffer { .. } => ValueKind::Buffer,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub fn is_object(&self) -> bool {
        matches!(self, Self::Object(_))
    }

    pub fn is_array(&self) -> bool {
        matches!(self, Self::Array(_))
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(v) => Some(*v),
            Self::I64(v) => Some(*v != 0),
            Self::F64(v) => Some(*v != 0.0),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::I64(v) => Some(*v),
            Self::F64(v) => Some(*v as i64),
            Self::Bool(v) => Some(i64::from(*v)),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::F64(v) => Some(*v),
            Self::I64(v) => Some(*v as f64),
            Self::Bool(v) => Some(f64::from(*v)),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&BoxList> {
        match self {
            Self::Array(v) => Some(v),
            _ => None,
        }
    }

    /// Mutable access, copying the interior first if it is shared
    /// ([`Arc::make_mut`]). Same signature and same semantics as before the
    /// interiors were shared — the copy is what preserves them.
    pub fn as_array_mut(&mut self) -> Option<&mut BoxList> {
        match self {
            Self::Array(v) => Some(Arc::make_mut(v)),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&BoxMap> {
        match self {
            Self::Object(v) => Some(v),
            _ => None,
        }
    }

    /// Mutable access, copying the interior first if it is shared. See
    /// [`BoxValue::as_array_mut`].
    pub fn as_object_mut(&mut self) -> Option<&mut BoxMap> {
        match self {
            Self::Object(v) => Some(Arc::make_mut(v)),
            _ => None,
        }
    }

    /// `Vec3` directly, or a length-3 numeric `Array` coerced.
    pub fn as_vec3(&self) -> Option<[f64; 3]> {
        match self {
            Self::Vec3(v) => Some(*v),
            Self::Array(a) => arr_to_fixed(a),
            _ => None,
        }
    }

    /// `Vec4`/`Color` directly, or a length-4 numeric `Array` coerced.
    pub fn as_vec4(&self) -> Option<[f64; 4]> {
        match self {
            Self::Vec4(v) | Self::Color(v) => Some(*v),
            Self::Array(a) => arr_to_fixed(a),
            _ => None,
        }
    }

    pub fn as_mat3(&self) -> Option<[f64; 9]> {
        match self {
            Self::Mat3(v) => Some(*v),
            Self::Array(a) => arr_to_fixed(a),
            _ => None,
        }
    }

    pub fn as_mat4(&self) -> Option<[f64; 16]> {
        match self {
            Self::Mat4(v) => Some(*v),
            Self::Array(a) => arr_to_fixed(a),
            _ => None,
        }
    }

    /// `Color`/`Vec4` directly, or a length-4 numeric `Array` coerced.
    pub fn as_color(&self) -> Option<[f64; 4]> {
        match self {
            Self::Color(v) | Self::Vec4(v) => Some(*v),
            Self::Array(a) => arr_to_fixed(a),
            _ => None,
        }
    }

    pub fn to_json_value(&self) -> serde_json::Value {
        match self {
            Self::Null => serde_json::Value::Null,
            Self::Bool(v) => serde_json::Value::Bool(*v),
            Self::I64(v) => serde_json::json!(*v),
            Self::F64(v) => serde_json::json!(*v),
            Self::String(v) => serde_json::Value::String(v.clone()),
            Self::Array(items) => {
                serde_json::Value::Array(items.iter().map(|i| i.to_json_value()).collect())
            }
            Self::Object(map) => {
                let mut obj = serde_json::Map::new();
                for (k, v) in map.iter() {
                    obj.insert(k.clone(), v.to_json_value());
                }
                serde_json::Value::Object(obj)
            }
            Self::Vec3(v) => serde_json::json!({ "$vec3": v.to_vec() }),
            Self::Vec4(v) => serde_json::json!({ "$vec4": v.to_vec() }),
            Self::Mat3(v) => serde_json::json!({ "$mat3": v.to_vec() }),
            Self::Mat4(v) => serde_json::json!({ "$mat4": v.to_vec() }),
            Self::Color(v) => serde_json::json!({ "$color": v.to_vec() }),
            // Bytes as a native JSON u8 array (NOT base64 — keeps box-rs dep-light;
            // this pure-JSON form is verbose and meant for small buffers / interop,
            // the `safetensors` backend is the compact binary transport).
            Self::Buffer { dtype, shape, data } => serde_json::json!({
                "$buffer": {
                    "dtype": dtype.tag(),
                    "shape": shape,
                    "data": data.0.as_ref(),
                }
            }),
        }
    }

    pub fn from_json_value(v: serde_json::Value) -> Self {
        match v {
            serde_json::Value::Null => Self::Null,
            serde_json::Value::Bool(b) => Self::Bool(b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Self::I64(i)
                } else if let Some(f) = n.as_f64() {
                    Self::F64(f)
                } else {
                    Self::Null
                }
            }
            serde_json::Value::String(s) => Self::String(s),
            serde_json::Value::Array(a) => Self::array(
                a.into_iter()
                    .map(Self::from_json_value)
                    .collect::<BoxList>(),
            ),
            serde_json::Value::Object(o) => {
                if let Some(typed) = tagged_from_obj(&o) {
                    typed
                } else {
                    Self::obj(
                        o.into_iter()
                            .map(|(k, v)| (k, Self::from_json_value(v)))
                            .collect(),
                    )
                }
            }
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self.kind() {
            ValueKind::Null => "null",
            ValueKind::Bool => "bool",
            ValueKind::I64 => "i64",
            ValueKind::F64 => "f64",
            ValueKind::String => "string",
            ValueKind::Array => "array",
            ValueKind::Object => "object",
            ValueKind::Vec3 => "vec3",
            ValueKind::Vec4 => "vec4",
            ValueKind::Mat3 => "mat3",
            ValueKind::Mat4 => "mat4",
            ValueKind::Color => "color",
            ValueKind::Buffer => "buffer",
        }
    }

    // -----------------------------------------------------------------------
    // Buffer accessors + constructor
    // -----------------------------------------------------------------------

    /// Build a typed binary buffer. `data` length should be `shape.product() *
    /// dtype.size()`, but that is the caller's contract — not enforced here.
    pub fn buffer(dtype: DType, shape: impl Into<Vec<usize>>, data: impl Into<Arc<[u8]>>) -> Self {
        Self::Buffer {
            dtype,
            shape: shape.into(),
            data: BufData(data.into()),
        }
    }

    /// Raw bytes of a `Buffer` (any dtype), else `None`.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Buffer { data, .. } => Some(&data.0),
            _ => None,
        }
    }

    /// `(dtype, shape, bytes)` of a `Buffer`, else `None`. Consumers cast the raw
    /// bytes to a typed slice on their side (e.g. `bytemuck::cast_slice`) — box-rs
    /// stays `unsafe`-free and dtype-agnostic.
    pub fn as_buffer(&self) -> Option<(DType, &[usize], &[u8])> {
        match self {
            Self::Buffer { dtype, shape, data } => Some((*dtype, shape, &data.0)),
            _ => None,
        }
    }

    /// Arc-shared handle to the buffer bytes (cheap clone), else `None`.
    pub fn buffer_arc(&self) -> Option<Arc<[u8]>> {
        match self {
            Self::Buffer { data, .. } => Some(Arc::clone(&data.0)),
            _ => None,
        }
    }
}

/// Coerce a numeric `BoxValue` slice into a fixed array, or `None` on length /
/// type mismatch. Used by the typed accessors (`as_vec3`, `as_color`, …).
fn arr_to_fixed<const N: usize>(a: &[BoxValue]) -> Option<[f64; N]> {
    if a.len() != N {
        return None;
    }
    let mut out = [0.0_f64; N];
    for (o, v) in out.iter_mut().zip(a) {
        *o = v.as_f64()?;
    }
    Some(out)
}

/// Recognise a single-key tagged aggregate object (`{"$vec3":[..]}`, …).
fn tagged_from_obj(o: &serde_json::Map<String, serde_json::Value>) -> Option<BoxValue> {
    if o.len() != 1 {
        return None;
    }
    let (k, val) = o.iter().next().unwrap();
    // `$buffer` carries an OBJECT payload (dtype/shape/data), not an array — decode
    // it BEFORE the `as_array()?` below (which would otherwise bail and silently
    // demote a Buffer back to a plain Object, losing the type). See the round-trip
    // invariant test `buffer_json_roundtrip`.
    if k == "$buffer" {
        return buffer_from_json(val.as_object()?);
    }
    let arr = val.as_array()?;
    let nums: Vec<f64> = arr.iter().map(|n| n.as_f64()).collect::<Option<_>>()?;
    match (k.as_str(), nums.len()) {
        ("$vec3", 3) => Some(BoxValue::Vec3(nums.try_into().ok()?)),
        ("$vec4", 4) => Some(BoxValue::Vec4(nums.try_into().ok()?)),
        ("$mat3", 9) => Some(BoxValue::Mat3(nums.try_into().ok()?)),
        ("$mat4", 16) => Some(BoxValue::Mat4(nums.try_into().ok()?)),
        ("$color", 4) => Some(BoxValue::Color(nums.try_into().ok()?)),
        _ => None,
    }
}

/// Decode a `$buffer` payload object `{"dtype":"f32","shape":[..],"data":[u8,…]}`
/// back into a `BoxValue::Buffer`. Returns `None` on a malformed payload (unknown
/// dtype, non-u8 data element) so `from_json_value` falls back to a plain object.
fn buffer_from_json(payload: &serde_json::Map<String, serde_json::Value>) -> Option<BoxValue> {
    let dtype = DType::from_tag(payload.get("dtype")?.as_str()?)?;
    let shape: Vec<usize> = payload
        .get("shape")?
        .as_array()?
        .iter()
        .map(|n| n.as_u64().map(|u| u as usize))
        .collect::<Option<_>>()?;
    let data: Vec<u8> = payload
        .get("data")?
        .as_array()?
        .iter()
        .map(|n| n.as_u64().and_then(|u| u8::try_from(u).ok()))
        .collect::<Option<_>>()?;
    Some(BoxValue::Buffer {
        dtype,
        shape,
        data: BufData(Arc::from(data)),
    })
}

impl Serialize for BoxValue {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.to_json_value().serialize(s)
    }
}

impl<'de> Deserialize<'de> for BoxValue {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let j = serde_json::Value::deserialize(d)?;
        Ok(BoxValue::from_json_value(j))
    }
}

impl Index<usize> for BoxValue {
    type Output = BoxValue;

    fn index(&self, i: usize) -> &BoxValue {
        self.as_array()
            .and_then(|a| a.get(i))
            .expect("box-rs: index out of range / not an array")
    }
}

impl IndexMut<usize> for BoxValue {
    fn index_mut(&mut self, i: usize) -> &mut BoxValue {
        self.as_array_mut()
            .and_then(|a| a.get_mut(i))
            .expect("box-rs: index out of range / not an array")
    }
}

impl From<bool> for BoxValue {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}

impl From<i64> for BoxValue {
    fn from(v: i64) -> Self {
        Self::I64(v)
    }
}

impl From<i32> for BoxValue {
    fn from(v: i32) -> Self {
        Self::I64(v as i64)
    }
}

impl From<f64> for BoxValue {
    fn from(v: f64) -> Self {
        Self::F64(v)
    }
}

impl From<&str> for BoxValue {
    fn from(v: &str) -> Self {
        Self::String(v.to_string())
    }
}

impl From<String> for BoxValue {
    fn from(v: String) -> Self {
        Self::String(v)
    }
}

impl From<[f64; 3]> for BoxValue {
    fn from(v: [f64; 3]) -> Self {
        Self::Vec3(v)
    }
}

impl From<[f64; 4]> for BoxValue {
    fn from(v: [f64; 4]) -> Self {
        Self::Vec4(v)
    }
}

impl From<[f64; 9]> for BoxValue {
    fn from(v: [f64; 9]) -> Self {
        Self::Mat3(v)
    }
}

impl From<[f64; 16]> for BoxValue {
    fn from(v: [f64; 16]) -> Self {
        Self::Mat4(v)
    }
}

impl From<BoxList> for BoxValue {
    fn from(v: BoxList) -> Self {
        Self::array(v)
    }
}

impl From<BoxMap> for BoxValue {
    fn from(v: BoxMap) -> Self {
        Self::obj(v)
    }
}

impl FromIterator<BoxValue> for BoxValue {
    fn from_iter<I: IntoIterator<Item = BoxValue>>(iter: I) -> Self {
        Self::array(iter.into_iter().collect::<BoxList>())
    }
}

impl FromIterator<(String, BoxValue)> for BoxValue {
    fn from_iter<I: IntoIterator<Item = (String, BoxValue)>>(iter: I) -> Self {
        Self::obj(iter.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_variants_kind() {
        assert_eq!(BoxValue::Vec3([1.0, 2.0, 3.0]).kind(), ValueKind::Vec3);
        assert_eq!(BoxValue::Vec4([1.0; 4]).kind(), ValueKind::Vec4);
        assert_eq!(BoxValue::Mat3([0.0; 9]).kind(), ValueKind::Mat3);
        assert_eq!(BoxValue::Mat4([0.0; 16]).kind(), ValueKind::Mat4);
        assert_eq!(
            BoxValue::Color([0.1, 0.2, 0.3, 1.0]).kind(),
            ValueKind::Color
        );
    }

    #[test]
    fn typed_accessors_and_coercions() {
        assert_eq!(
            BoxValue::Vec3([1.0, 2.0, 3.0]).as_vec3(),
            Some([1.0, 2.0, 3.0])
        );
        assert_eq!(
            BoxValue::Color([0.1, 0.2, 0.3, 1.0]).as_color(),
            Some([0.1, 0.2, 0.3, 1.0])
        );
        // length-checked coercion from a plain array:
        let arr3 = BoxValue::array(vec![1.0.into(), 2.0.into(), 3.0.into()]);
        assert_eq!(arr3.as_vec3(), Some([1.0, 2.0, 3.0]));
        let arr2 = BoxValue::array(vec![1.0.into(), 2.0.into()]);
        assert_eq!(arr2.as_vec3(), None);
        // From impls:
        assert_eq!(
            BoxValue::from([1.0_f64, 2.0, 3.0]),
            BoxValue::Vec3([1.0, 2.0, 3.0])
        );
    }

    #[test]
    fn typed_json_roundtrip() {
        for v in [
            BoxValue::Vec3([1.0, 2.0, 3.0]),
            BoxValue::Vec4([1.0, 2.0, 3.0, 4.0]),
            BoxValue::Mat3([0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]),
            BoxValue::Mat4([0.0; 16]),
            BoxValue::Color([0.1, 0.2, 0.3, 1.0]),
        ] {
            let j = v.to_json_value();
            assert_eq!(BoxValue::from_json_value(j), v);
        }
        // a plain 3-array stays an Array (NOT a Vec3):
        let arr = serde_json::json!([1.0, 2.0, 3.0]);
        assert!(matches!(BoxValue::from_json_value(arr), BoxValue::Array(_)));
    }

    #[test]
    fn serde_via_serde_json_matches_tagged() {
        let v = BoxValue::Vec3([1.0, 2.0, 3.0]);
        let s = serde_json::to_string(&v).unwrap();
        assert_eq!(s, r#"{"$vec3":[1.0,2.0,3.0]}"#);
        let back: BoxValue = serde_json::from_str(&s).unwrap();
        assert_eq!(back, v);

        let obj = BoxValue::Object(Default::default());
        assert_eq!(serde_json::to_string(&obj).unwrap(), "{}");
    }

    #[test]
    fn buffer_json_roundtrip() {
        // Non-empty typed buffer round-trips through the pure-JSON backend.
        let v = BoxValue::buffer(DType::F32, vec![2usize], vec![1u8, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(v.kind(), ValueKind::Buffer);
        assert_eq!(BoxValue::from_json_value(v.to_json_value()), v);

        // Via the serde path (Serialize/Deserialize) too.
        let s = serde_json::to_string(&v).unwrap();
        let back: BoxValue = serde_json::from_str(&s).unwrap();
        assert_eq!(back, v);

        // Empty buffer is valid and round-trips.
        let e = BoxValue::buffer(DType::U8, vec![0usize], Vec::<u8>::new());
        assert_eq!(BoxValue::from_json_value(e.to_json_value()), e);

        // `$buffer` (object payload) is NOT confused with `$vec4` (array payload).
        let vec4 = BoxValue::Vec4([1.0, 2.0, 3.0, 4.0]);
        assert!(matches!(
            BoxValue::from_json_value(vec4.to_json_value()),
            BoxValue::Vec4(_)
        ));

        // Debug prints byte COUNT, not the bytes.
        assert!(format!("{v:?}").contains("8 bytes"));
    }

    /// The whole point of the shared interiors: a clone must stay a value, not
    /// become a second handle on the same data. If this ever fails, every
    /// consumer that clones a payload and edits it is silently corrupting the
    /// original.
    #[test]
    fn a_clone_is_independent_however_deep_the_edit() {
        let mut original = BoxValue::object();
        original
            .as_object_mut()
            .expect("object")
            .insert("list".into(), BoxValue::array(vec![BoxValue::from(1)]));

        let mut copy = original.clone();
        // Reach through two levels of shared interior and write.
        copy.as_object_mut()
            .expect("object")
            .get_mut("list")
            .expect("list")
            .as_array_mut()
            .expect("array")
            .push(BoxValue::from(2));

        assert_eq!(
            original["list"].as_array().map(Vec::len),
            Some(1),
            "writing through a clone reached back into the original"
        );
        assert_eq!(copy["list"].as_array().map(Vec::len), Some(2));

        // ...and the other direction.
        original
            .as_object_mut()
            .expect("object")
            .insert("extra".into(), BoxValue::from(9));
        assert!(copy.as_object().expect("object").get("extra").is_none());
    }

    /// Sharing must not turn into identity comparison: two trees built
    /// separately are still equal, and a clone is still equal to its source.
    #[test]
    fn equality_is_by_content_not_by_pointer() {
        let build = || {
            let mut v = BoxValue::object();
            v.as_object_mut()
                .expect("object")
                .insert("a".into(), BoxValue::array(vec![BoxValue::from(1)]));
            v
        };
        assert_eq!(build(), build(), "separately built equal trees differ");
        let one = build();
        assert_eq!(one, one.clone());

        let mut other = build();
        other
            .as_object_mut()
            .expect("object")
            .insert("a".into(), BoxValue::array(vec![BoxValue::from(2)]));
        assert_ne!(one, other);
    }

    /// `deep_freeze_clone` no longer walks the tree; it must still hand back a
    /// value nothing can mutate behind the caller's back.
    #[test]
    fn a_frozen_clone_is_independent() {
        let mut original = BoxValue::array(vec![BoxValue::from(1)]);
        let frozen = crate::convert::deep_freeze_clone(&original);
        original
            .as_array_mut()
            .expect("array")
            .push(BoxValue::from(2));
        assert_eq!(frozen.as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn array_index() {
        let v = BoxValue::array(vec![BoxValue::from(10), BoxValue::from(20)]);
        assert_eq!(v[1].as_i64(), Some(20));
        let mut m = BoxValue::array(vec![BoxValue::from(1)]);
        m[0] = BoxValue::from(99);
        assert_eq!(m[0].as_i64(), Some(99));
    }
}
