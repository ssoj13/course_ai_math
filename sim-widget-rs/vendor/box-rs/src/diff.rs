use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::RustBox;
use crate::access::get_at_ref;
use crate::config::BoxConfig;
use crate::error::BoxError;
use crate::path::{PathKey, Segment, parse_path};
use crate::value::BoxValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeKind {
    Added,
    Removed,
    Changed,
    TypeChanged,
    Moved,
    Resized,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ScalarComparePolicy {
    Strict,
    JsonCanonical,
    Numeric {
        epsilon: f64,
    },
    StringTrimmed,
    /// Use [`DiffOptions::scalar_custom`].
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArrayDiffPolicy {
    ByIndex,
    LongestCommonSubsequence,
    HashBy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectDiffPolicy {
    Deep,
    KeysOnly,
    WithValues,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PathFormat {
    Dot,
    JsonPointer,
    Segments,
}

pub type ScalarCompareResolver = Arc<dyn Fn(&BoxValue, &BoxValue) -> bool + Send + Sync>;

#[derive(Clone)]
pub struct DiffOptions {
    pub scalar: ScalarComparePolicy,
    pub array: ArrayDiffPolicy,
    pub object: ObjectDiffPolicy,
    pub path_format: PathFormat,
    pub include_patch: bool,
    pub hash_by_path: Option<String>,
    pub scalar_custom: Option<ScalarCompareResolver>,
}

impl std::fmt::Debug for DiffOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiffOptions")
            .field("scalar", &self.scalar)
            .field("array", &self.array)
            .field("object", &self.object)
            .field("has_scalar_custom", &self.scalar_custom.is_some())
            .finish()
    }
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self {
            scalar: ScalarComparePolicy::Strict,
            array: ArrayDiffPolicy::ByIndex,
            object: ObjectDiffPolicy::Deep,
            path_format: PathFormat::Dot,
            include_patch: true,
            hash_by_path: None,
            scalar_custom: None,
        }
    }
}

impl Serialize for DiffOptions {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DiffOptionsSer {
            scalar: self.scalar,
            array: self.array,
            object: self.object,
            path_format: self.path_format,
            include_patch: self.include_patch,
            hash_by_path: self.hash_by_path.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for DiffOptions {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = DiffOptionsSer::deserialize(deserializer)?;
        Ok(Self {
            scalar: s.scalar,
            array: s.array,
            object: s.object,
            path_format: s.path_format,
            include_patch: s.include_patch,
            hash_by_path: s.hash_by_path,
            scalar_custom: None,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct DiffOptionsSer {
    scalar: ScalarComparePolicy,
    array: ArrayDiffPolicy,
    object: ObjectDiffPolicy,
    path_format: PathFormat,
    include_patch: bool,
    hash_by_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    pub path: String,
    pub kind: ChangeKind,
    pub old: Option<BoxValue>,
    pub new: Option<BoxValue>,
}

#[derive(Debug, Clone)]
pub struct DiffReport {
    pub changes: Vec<Change>,
    pub patch: Option<RustBox>,
}

pub fn diff_values(left: &BoxValue, right: &BoxValue, options: &DiffOptions) -> DiffReport {
    let mut changes = Vec::new();
    diff_pair(left, right, &PathKey::empty(), options, &mut changes);
    let patch = if options.include_patch {
        Some(build_patch(&changes))
    } else {
        None
    };
    DiffReport { changes, patch }
}

impl DiffReport {
    /// Apply this diff to `target` (uses `changes`; `patch` is a convenience snapshot).
    pub fn apply_to(&self, target: &mut RustBox) -> Result<(), BoxError> {
        for ch in &self.changes {
            match ch.kind {
                ChangeKind::Added
                | ChangeKind::Changed
                | ChangeKind::TypeChanged
                | ChangeKind::Moved => {
                    if let Some(ref v) = ch.new {
                        target.set_path(&ch.path, v.clone())?;
                    }
                }
                ChangeKind::Removed => {
                    let _ = target.pop(&ch.path);
                }
                ChangeKind::Resized => {}
            }
        }
        Ok(())
    }
}

fn diff_pair(
    left: &BoxValue,
    right: &BoxValue,
    path: &PathKey,
    options: &DiffOptions,
    out: &mut Vec<Change>,
) {
    if left.kind() != right.kind() {
        out.push(Change {
            path: format_path(path, options.path_format),
            kind: ChangeKind::TypeChanged,
            old: Some(left.clone()),
            new: Some(right.clone()),
        });
        return;
    }
    match (left, right) {
        (BoxValue::Object(l), BoxValue::Object(r)) => diff_objects(l, r, path, options, out),
        (BoxValue::Array(l), BoxValue::Array(r)) => diff_arrays(l, r, path, options, out),
        _ => {
            if !values_equal(left, right, options) {
                out.push(Change {
                    path: format_path(path, options.path_format),
                    kind: ChangeKind::Changed,
                    old: Some(left.clone()),
                    new: Some(right.clone()),
                });
            }
        }
    }
}

fn diff_objects(
    left: &BTreeMap<String, BoxValue>,
    right: &BTreeMap<String, BoxValue>,
    path: &PathKey,
    options: &DiffOptions,
    out: &mut Vec<Change>,
) {
    for (k, lv) in left {
        let child = path.push(Segment::Key(k.clone()));
        match right.get(k) {
            Some(rv) => match options.object {
                ObjectDiffPolicy::KeysOnly => {
                    if !values_equal(lv, rv, options) {
                        out.push(Change {
                            path: format_path(&child, options.path_format),
                            kind: ChangeKind::Changed,
                            old: Some(lv.clone()),
                            new: Some(rv.clone()),
                        });
                    }
                }
                ObjectDiffPolicy::Deep | ObjectDiffPolicy::WithValues => {
                    diff_pair(lv, rv, &child, options, out);
                }
            },
            None => out.push(Change {
                path: format_path(&child, options.path_format),
                kind: ChangeKind::Removed,
                old: Some(lv.clone()),
                new: None,
            }),
        }
    }
    for (k, rv) in right {
        if !left.contains_key(k) {
            let child = path.push(Segment::Key(k.clone()));
            out.push(Change {
                path: format_path(&child, options.path_format),
                kind: ChangeKind::Added,
                old: None,
                new: Some(rv.clone()),
            });
        }
    }
}

fn diff_arrays(
    left: &[BoxValue],
    right: &[BoxValue],
    path: &PathKey,
    options: &DiffOptions,
    out: &mut Vec<Change>,
) {
    if left.len() != right.len() {
        out.push(Change {
            path: format_path(path, options.path_format),
            kind: ChangeKind::Resized,
            old: Some(BoxValue::I64(left.len() as i64)),
            new: Some(BoxValue::I64(right.len() as i64)),
        });
    }
    match options.array {
        ArrayDiffPolicy::ByIndex => diff_arrays_by_index(left, right, path, options, out),
        ArrayDiffPolicy::LongestCommonSubsequence => {
            let aligned = super::merge::lcs_align(left, right);
            for (idx, (l, r)) in aligned.into_iter().enumerate() {
                let child = path.push(Segment::Index(idx));
                match (l, r) {
                    (Some(l), Some(r)) => diff_pair(l, r, &child, options, out),
                    (Some(l), None) => out.push(Change {
                        path: format_path(&child, options.path_format),
                        kind: ChangeKind::Removed,
                        old: Some(l.clone()),
                        new: None,
                    }),
                    (None, Some(r)) => out.push(Change {
                        path: format_path(&child, options.path_format),
                        kind: ChangeKind::Added,
                        old: None,
                        new: Some(r.clone()),
                    }),
                    (None, None) => {}
                }
            }
        }
        ArrayDiffPolicy::HashBy => {
            if options.hash_by_path.is_none() {
                diff_arrays_by_index(left, right, path, options, out);
            } else {
                diff_arrays_hash_by(left, right, path, options, out);
            }
        }
    }
}

fn diff_arrays_by_index(
    left: &[BoxValue],
    right: &[BoxValue],
    path: &PathKey,
    options: &DiffOptions,
    out: &mut Vec<Change>,
) {
    let max = left.len().max(right.len());
    for i in 0..max {
        let child = path.push(Segment::Index(i));
        match (left.get(i), right.get(i)) {
            (Some(l), Some(r)) => diff_pair(l, r, &child, options, out),
            (Some(l), None) => out.push(Change {
                path: format_path(&child, options.path_format),
                kind: ChangeKind::Removed,
                old: Some(l.clone()),
                new: None,
            }),
            (None, Some(r)) => out.push(Change {
                path: format_path(&child, options.path_format),
                kind: ChangeKind::Added,
                old: None,
                new: Some(r.clone()),
            }),
            (None, None) => {}
        }
    }
}

fn diff_arrays_hash_by(
    left: &[BoxValue],
    right: &[BoxValue],
    path: &PathKey,
    options: &DiffOptions,
    out: &mut Vec<Change>,
) {
    let key_path_str = match options.hash_by_path.as_deref() {
        Some(p) => p,
        None => {
            diff_arrays_by_index(left, right, path, options, out);
            return;
        }
    };
    let key_path = match parse_path(key_path_str) {
        Ok(p) => p,
        Err(_) => {
            diff_arrays_by_index(left, right, path, options, out);
            return;
        }
    };
    let config = BoxConfig::default();

    let left_map = index_array_by_key(left, &key_path, &config);
    let right_map = index_array_by_key(right, &key_path, &config);

    for (id, (li, lv)) in &left_map {
        if let Some((ri, rv)) = right_map.get(id) {
            if li != ri {
                out.push(Change {
                    path: format_path(path, options.path_format),
                    kind: ChangeKind::Moved,
                    old: Some(BoxValue::I64(*li as i64)),
                    new: Some(BoxValue::I64(*ri as i64)),
                });
            }
            let child = path.push(Segment::Index(*ri));
            diff_pair(lv, rv, &child, options, out);
        } else {
            let child = path.push(Segment::Index(*li));
            out.push(Change {
                path: format_path(&child, options.path_format),
                kind: ChangeKind::Removed,
                old: Some(lv.clone()),
                new: None,
            });
        }
    }
    for (id, (ri, rv)) in &right_map {
        if !left_map.contains_key(id) {
            let child = path.push(Segment::Index(*ri));
            out.push(Change {
                path: format_path(&child, options.path_format),
                kind: ChangeKind::Added,
                old: None,
                new: Some(rv.clone()),
            });
        }
    }
}

fn index_array_by_key(
    items: &[BoxValue],
    key_path: &PathKey,
    config: &BoxConfig,
) -> BTreeMap<String, (usize, BoxValue)> {
    let mut map = BTreeMap::new();
    for (i, item) in items.iter().enumerate() {
        if let Some(id_val) = get_at_ref(item, key_path, config) {
            let id = id_val
                .as_str()
                .map(|s| s.to_string())
                .or_else(|| id_val.as_i64().map(|n| n.to_string()))
                .unwrap_or_else(|| id_val.to_json_value().to_string());
            map.insert(id, (i, item.clone()));
        }
    }
    map
}

pub fn values_equal(a: &BoxValue, b: &BoxValue, options: &DiffOptions) -> bool {
    scalars_equal(a, b, options.scalar, options.scalar_custom.as_ref())
}

pub fn scalars_equal(
    a: &BoxValue,
    b: &BoxValue,
    policy: ScalarComparePolicy,
    custom: Option<&ScalarCompareResolver>,
) -> bool {
    if policy == ScalarComparePolicy::Custom {
        if let Some(f) = custom {
            return f(a, b);
        }
        return a == b;
    }
    match policy {
        ScalarComparePolicy::Strict => a == b,
        ScalarComparePolicy::JsonCanonical => a.to_json_value() == b.to_json_value(),
        ScalarComparePolicy::Numeric { epsilon } => match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => (x - y).abs() <= epsilon,
            _ => a == b,
        },
        ScalarComparePolicy::StringTrimmed => match (a.as_str(), b.as_str()) {
            (Some(x), Some(y)) => x.trim() == y.trim(),
            _ => a == b,
        },
        ScalarComparePolicy::Custom => unreachable!(),
    }
}

fn format_path(path: &PathKey, fmt: PathFormat) -> String {
    match fmt {
        PathFormat::Dot => path.format_dot(),
        PathFormat::Segments => path
            .segments()
            .iter()
            .map(|s| match s {
                Segment::Key(k) => k.clone(),
                Segment::Index(i) => format!("[{i}]"),
            })
            .collect::<Vec<_>>()
            .join("/"),
        PathFormat::JsonPointer => {
            if path.is_empty() {
                return "/".into();
            }
            let mut out = String::new();
            for seg in path.segments() {
                match seg {
                    Segment::Key(k) => {
                        out.push('/');
                        out.push_str(&k.replace('~', "~0").replace('/', "~1"));
                    }
                    Segment::Index(i) => out.push_str(&format!("/{i}")),
                }
            }
            out
        }
    }
}

fn build_patch(changes: &[Change]) -> RustBox {
    let mut patch = RustBox::new();
    for ch in changes {
        if matches!(
            ch.kind,
            ChangeKind::Added | ChangeKind::Changed | ChangeKind::TypeChanged | ChangeKind::Moved
        ) && let Some(ref v) = ch.new
        {
            let _ = patch.set_path(&ch.path, v.clone());
        }
    }
    patch
}
