//! `.safetensors` backend for a [`RustBox`] scene (feature `safetensors`).
//!
//! A second serialization backend alongside the built-in JSON one. Bulk
//! [`BoxValue::Buffer`] payloads are written as real safetensors tensors
//! (typed, aligned, mmap-friendly), and the rest of the box TREE — with each
//! buffer replaced by a `{"$bufref":"<name>"}` marker — is stored as a JSON
//! string in the safetensors `__metadata__` under the key `"box"`. Loading
//! reverses it: parse the tree JSON, resolve each `$bufref` to its tensor's
//! bytes (copied into an `Arc<[u8]>`).
//!
//! This makes a box a single, tool-inspectable checkpoint file: `numpy`/HF can
//! open the buffers, and the scene structure rides in the header metadata.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use safetensors::tensor::{Dtype, SafeTensors, View, serialize_to_file};

use crate::RustBox;
use crate::config::BoxConfig;
use crate::value::{BoxValue, BufData, DType};

/// Metadata key under which the box tree JSON (buffers → `$bufref`) is stored.
const BOX_META_KEY: &str = "box";

#[derive(Debug, thiserror::Error)]
pub enum ContainerError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("safetensors: {0}")]
    SafeTensor(#[from] safetensors::SafeTensorError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("container format: {0}")]
    Format(String),
}

type Result<T> = std::result::Result<T, ContainerError>;

// ---------------------------------------------------------------------------
// dtype bridge
// ---------------------------------------------------------------------------

fn to_st(d: DType) -> Dtype {
    match d {
        DType::Bool => Dtype::BOOL,
        DType::U8 => Dtype::U8,
        DType::I8 => Dtype::I8,
        DType::U16 => Dtype::U16,
        DType::I16 => Dtype::I16,
        DType::F16 => Dtype::F16,
        DType::Bf16 => Dtype::BF16,
        DType::U32 => Dtype::U32,
        DType::I32 => Dtype::I32,
        DType::F32 => Dtype::F32,
        DType::U64 => Dtype::U64,
        DType::I64 => Dtype::I64,
        DType::F64 => Dtype::F64,
    }
}

fn from_st(d: Dtype) -> Option<DType> {
    Some(match d {
        Dtype::BOOL => DType::Bool,
        Dtype::U8 => DType::U8,
        Dtype::I8 => DType::I8,
        Dtype::U16 => DType::U16,
        Dtype::I16 => DType::I16,
        Dtype::F16 => DType::F16,
        Dtype::BF16 => DType::Bf16,
        Dtype::U32 => DType::U32,
        Dtype::I32 => DType::I32,
        Dtype::F32 => DType::F32,
        Dtype::U64 => DType::U64,
        Dtype::I64 => DType::I64,
        Dtype::F64 => DType::F64,
        _ => return None,
    })
}

/// A safetensors `View` over a box buffer's bytes (zero-copy write via `Cow::Borrowed`).
struct BufView {
    dtype: Dtype,
    shape: Vec<usize>,
    data: Arc<[u8]>,
}

impl View for BufView {
    fn dtype(&self) -> Dtype {
        self.dtype
    }
    fn shape(&self) -> &[usize] {
        &self.shape
    }
    fn data(&self) -> Cow<'_, [u8]> {
        Cow::Borrowed(&self.data)
    }
    fn data_len(&self) -> usize {
        self.data.len()
    }
}

// ---------------------------------------------------------------------------
// save
// ---------------------------------------------------------------------------

/// Serialize `b` to a `.safetensors` file: buffers as tensors, the rest of the
/// tree as JSON in `__metadata__["box"]`.
pub fn save_safetensors(b: &RustBox, path: impl AsRef<Path>) -> Result<()> {
    let mut tensors: Vec<(String, BufView)> = Vec::new();
    let tree = encode(b.value(), "$".to_string(), &mut tensors);
    let tree_json = serde_json::to_string(&tree)?;
    let mut meta = HashMap::new();
    meta.insert(BOX_META_KEY.to_string(), tree_json);
    serialize_to_file(tensors, Some(meta), path.as_ref())?;
    Ok(())
}

/// Walk the tree: each `Buffer` becomes a tensor named by its unique tree PATH and
/// is replaced by a `{"$bufref": <path>}` marker; everything else recurses (leaves
/// go through the normal JSON encoding, which never contains a nested buffer).
fn encode(v: &BoxValue, name: String, tensors: &mut Vec<(String, BufView)>) -> serde_json::Value {
    match v {
        BoxValue::Buffer { dtype, shape, data } => {
            tensors.push((
                name.clone(),
                BufView {
                    dtype: to_st(*dtype),
                    shape: shape.clone(),
                    data: Arc::clone(&data.0),
                },
            ));
            serde_json::json!({ "$bufref": name })
        }
        BoxValue::Array(items) => serde_json::Value::Array(
            items
                .iter()
                .enumerate()
                .map(|(i, x)| encode(x, format!("{name}/{i}"), tensors))
                .collect(),
        ),
        BoxValue::Object(map) => serde_json::Value::Object(
            map.iter()
                .map(|(k, x)| (k.clone(), encode(x, format!("{name}/{k}"), tensors)))
                .collect(),
        ),
        other => other.to_json_value(),
    }
}

// ---------------------------------------------------------------------------
// load
// ---------------------------------------------------------------------------

/// Load a `.safetensors` scene written by [`save_safetensors`].
pub fn load_safetensors(path: impl AsRef<Path>) -> Result<RustBox> {
    let bytes = std::fs::read(path.as_ref())?;
    let (_n, metadata) = SafeTensors::read_metadata(&bytes)?;
    let tree_json = metadata
        .metadata()
        .as_ref()
        .and_then(|m| m.get(BOX_META_KEY))
        .ok_or_else(|| ContainerError::Format("missing __metadata__[\"box\"]".into()))?;
    let tree: serde_json::Value = serde_json::from_str(tree_json)?;
    let st = SafeTensors::deserialize(&bytes)?;
    let value = decode(&tree, &st)?;
    Ok(RustBox::from_value(value, BoxConfig::default()))
}

/// Rebuild the box tree, resolving each `{"$bufref": name}` to its tensor bytes.
fn decode(v: &serde_json::Value, st: &SafeTensors) -> Result<BoxValue> {
    match v {
        serde_json::Value::Object(o) => {
            // A single "$"-prefixed key is a tagged LEAF, not a nested map:
            //  - `$bufref` → resolve the tensor into a Buffer;
            //  - `$vec3`/`$mat4`/… → let `from_json_value` decode the aggregate
            //    (recursing here would wrongly flatten it into a plain Object).
            if o.len() == 1 {
                let (k, _) = o.iter().next().unwrap();
                if k == "$bufref" {
                    let name = o[k].as_str().ok_or_else(|| {
                        ContainerError::Format("$bufref value is not a string".into())
                    })?;
                    let t = st.tensor(name)?;
                    let dtype = from_st(t.dtype()).ok_or_else(|| {
                        ContainerError::Format(format!("unsupported dtype in tensor {name}"))
                    })?;
                    return Ok(BoxValue::Buffer {
                        dtype,
                        shape: t.shape().to_vec(),
                        data: BufData(Arc::from(t.data())),
                    });
                }
                if k.starts_with('$') {
                    return Ok(BoxValue::from_json_value(v.clone()));
                }
            }
            let mut map = std::collections::BTreeMap::new();
            for (k, x) in o {
                map.insert(k.clone(), decode(x, st)?);
            }
            Ok(BoxValue::obj(map))
        }
        serde_json::Value::Array(a) => {
            let mut out = Vec::with_capacity(a.len());
            for x in a {
                out.push(decode(x, st)?);
            }
            Ok(BoxValue::array(out))
        }
        other => Ok(BoxValue::from_json_value(other.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safetensors_scene_roundtrip() {
        let mut b = RustBox::new();
        b.set_path("meta.name", "robat".into()).unwrap();
        b.set_path("meta.stage", "mapper".into()).unwrap();
        b.set_path(
            "cameras[0].pose",
            BoxValue::Mat4([
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 5.0, 6.0, 7.0, 1.0,
            ]),
        )
        .unwrap();
        // a point-cloud xyz buffer (2 points, f32)
        let xyz: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let orig_bytes: Vec<u8> = xyz.iter().flat_map(|f| f.to_le_bytes()).collect();
        b.set_path(
            "points.xyz",
            BoxValue::buffer(DType::F32, vec![2usize, 3], orig_bytes.clone()),
        )
        .unwrap();

        let dir = std::env::temp_dir();
        let path = dir.join("box_rs_scene_test.safetensors");
        save_safetensors(&b, &path).unwrap();
        let loaded = load_safetensors(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        // Metadata strings survive the __metadata__["box"] round-trip.
        assert_eq!(
            loaded.get_path("meta.name").and_then(|v| v.as_str()),
            Some("robat")
        );
        assert_eq!(
            loaded.get_path("meta.stage").and_then(|v| v.as_str()),
            Some("mapper")
        );
        // A nested tagged aggregate (camera pose) survives inside an array.
        assert_eq!(
            loaded.get_path("cameras[0].pose").and_then(|v| v.as_mat4()),
            Some([
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 5.0, 6.0, 7.0, 1.0
            ])
        );
        // The buffer round-trips as a real tensor: dtype, shape AND the exact bytes.
        let (dt, shape, data) = loaded.get_path("points.xyz").unwrap().as_buffer().unwrap();
        assert_eq!(dt, DType::F32);
        assert_eq!(shape, &[2usize, 3]);
        assert_eq!(
            data,
            orig_bytes.as_slice(),
            "buffer bytes must round-trip exactly"
        );
        // ...and decode back to the original f32 values (proves dtype+layout).
        let back: Vec<f32> = data
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        assert_eq!(back, xyz);
    }
}
