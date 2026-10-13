use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::config::BoxConfig;
use crate::error::MergeError;
use crate::path::{PathKey, Segment};
use crate::value::BoxValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectMergePolicy {
    Deep,
    ShallowReplace,
    ReplaceSubtree,
    MergeKeysPresentInLeft,
    MergeKeysPresentInRight,
    Union,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArrayMergePolicy {
    Replace,
    Concat,
    Prepend,
    MergeByIndex,
    ZipStrict,
    ZipLongest,
    LongestCommonSubsequence,
    UnionByKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZipTailPolicy {
    KeepLeft,
    KeepRight,
    Truncate,
    FillNull,
    FillDefaultBox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZipPadPolicy {
    FillNull,
    FillDefaultBox,
    Truncate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScalarConflictPolicy {
    PreferLeft,
    PreferRight,
    Error,
    PreferTypeOfRight,
    PreferTypeOfLeft,
    CoerceToRight,
    CoerceToLeft,
    /// Use [`MergeOptions::scalar_custom`] for the resolver.
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NullMergePolicy {
    Ignore,
    SetNull,
    DeleteKey,
    TreatAsMissing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeConflictPolicy {
    Error,
    PreferLeft,
    PreferRight,
    ReplaceWithCoercion,
    WrapAsValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MissingKeyPolicy {
    Keep,
    Remove,
    SetNull,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MergeMode {
    Patch,
    Overlay,
    Symmetric,
}

pub type ScalarMergeResolver =
    Arc<dyn Fn(&BoxValue, &BoxValue, &str) -> Result<BoxValue, MergeError> + Send + Sync>;

#[derive(Clone)]
pub struct MergeOptions {
    pub mode: MergeMode,
    pub object: ObjectMergePolicy,
    pub array: ArrayMergePolicy,
    pub scalar: ScalarConflictPolicy,
    pub null_policy: NullMergePolicy,
    pub type_conflict: TypeConflictPolicy,
    pub missing_key: MissingKeyPolicy,
    pub zip_tail: ZipTailPolicy,
    pub zip_pad: ZipPadPolicy,
    pub union_by_key_path: Option<String>,
    /// Required for [`MergeMode::Symmetric`] (3-way merge).
    pub base: Option<BoxValue>,
    /// Required when `scalar == ScalarConflictPolicy::Custom`.
    pub scalar_custom: Option<ScalarMergeResolver>,
}

impl std::fmt::Debug for MergeOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MergeOptions")
            .field("mode", &self.mode)
            .field("object", &self.object)
            .field("array", &self.array)
            .field("scalar", &self.scalar)
            .field("has_base", &self.base.is_some())
            .field("has_scalar_custom", &self.scalar_custom.is_some())
            .finish()
    }
}

impl Default for MergeOptions {
    fn default() -> Self {
        Self {
            mode: MergeMode::Patch,
            object: ObjectMergePolicy::Deep,
            array: ArrayMergePolicy::Replace,
            scalar: ScalarConflictPolicy::PreferRight,
            null_policy: NullMergePolicy::Ignore,
            type_conflict: TypeConflictPolicy::PreferRight,
            missing_key: MissingKeyPolicy::Keep,
            zip_tail: ZipTailPolicy::KeepRight,
            zip_pad: ZipPadPolicy::FillNull,
            union_by_key_path: None,
            base: None,
            scalar_custom: None,
        }
    }
}

impl Serialize for MergeOptions {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        MergeOptionsSer::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for MergeOptions {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = MergeOptionsSer::deserialize(deserializer)?;
        Ok(s.into())
    }
}

#[derive(Serialize, Deserialize)]
struct MergeOptionsSer {
    mode: MergeMode,
    object: ObjectMergePolicy,
    array: ArrayMergePolicy,
    scalar: ScalarConflictPolicy,
    null_policy: NullMergePolicy,
    type_conflict: TypeConflictPolicy,
    missing_key: MissingKeyPolicy,
    zip_tail: ZipTailPolicy,
    zip_pad: ZipPadPolicy,
    union_by_key_path: Option<String>,
}

impl From<&MergeOptions> for MergeOptionsSer {
    fn from(o: &MergeOptions) -> Self {
        Self {
            mode: o.mode,
            object: o.object,
            array: o.array,
            scalar: o.scalar,
            null_policy: o.null_policy,
            type_conflict: o.type_conflict,
            missing_key: o.missing_key,
            zip_tail: o.zip_tail,
            zip_pad: o.zip_pad,
            union_by_key_path: o.union_by_key_path.clone(),
        }
    }
}

impl From<MergeOptionsSer> for MergeOptions {
    fn from(s: MergeOptionsSer) -> Self {
        Self {
            mode: s.mode,
            object: s.object,
            array: s.array,
            scalar: s.scalar,
            null_policy: s.null_policy,
            type_conflict: s.type_conflict,
            missing_key: s.missing_key,
            zip_tail: s.zip_tail,
            zip_pad: s.zip_pad,
            union_by_key_path: s.union_by_key_path,
            base: None,
            scalar_custom: None,
        }
    }
}

pub fn merge_values(
    left: &BoxValue,
    right: &BoxValue,
    config: &BoxConfig,
    options: &MergeOptions,
) -> Result<BoxValue, MergeError> {
    match options.mode {
        MergeMode::Patch => merge_pair(left, right, config, options, &PathKey::empty()),
        MergeMode::Overlay => merge_overlay(left, right, config, options),
        MergeMode::Symmetric => {
            let base = options
                .base
                .as_ref()
                .ok_or_else(|| MergeError::MissingBase("Symmetric".into()))?;
            merge_three_way(base, left, right, config, options, &PathKey::empty())
        }
    }
}

fn merge_overlay(
    left: &BoxValue,
    right: &BoxValue,
    config: &BoxConfig,
    options: &MergeOptions,
) -> Result<BoxValue, MergeError> {
    match (left, right) {
        (BoxValue::Object(l), BoxValue::Object(r)) => {
            let mut out = (**l).clone();
            for (k, rv) in r.iter() {
                let child = PathKey::single_key(k);
                if let Some(lv) = l.get(k) {
                    let merged = merge_pair(lv, rv, config, options, &child)?;
                    out.insert(k.clone(), merged);
                } else {
                    out.insert(k.clone(), rv.clone());
                }
            }
            Ok(BoxValue::obj(out))
        }
        _ => merge_pair(left, right, config, options, &PathKey::empty()),
    }
}

fn merge_three_way(
    base: &BoxValue,
    left: &BoxValue,
    right: &BoxValue,
    config: &BoxConfig,
    options: &MergeOptions,
    path: &PathKey,
) -> Result<BoxValue, MergeError> {
    if left == right {
        return Ok(left.clone());
    }
    if base == left {
        return merge_pair(base, right, config, options, path);
    }
    if base == right {
        return Ok(left.clone());
    }
    if left.is_null() && !right.is_null() {
        return Ok(right.clone());
    }
    if right.is_null() {
        return apply_null_three_way(base, left, options, path);
    }
    if base.kind() == left.kind() && base.kind() == right.kind() {
        match (base, left, right) {
            (BoxValue::Object(b), BoxValue::Object(l), BoxValue::Object(r)) => {
                merge_objects_three(b, l, r, config, options, path)
            }
            (BoxValue::Array(b), BoxValue::Array(l), BoxValue::Array(r)) => {
                merge_arrays_three(b, l, r, config, options, path)
            }
            _ => resolve_scalar_three(base, left, right, options, path),
        }
    } else {
        resolve_type_conflict(left, right, config, options, path)
    }
}

fn apply_null_three_way(
    base: &BoxValue,
    left: &BoxValue,
    options: &MergeOptions,
    path: &PathKey,
) -> Result<BoxValue, MergeError> {
    let _ = base;
    match options.null_policy {
        NullMergePolicy::Ignore | NullMergePolicy::TreatAsMissing => Ok(left.clone()),
        NullMergePolicy::SetNull => Ok(BoxValue::Null),
        NullMergePolicy::DeleteKey => Err(MergeError::Conflict {
            path: path.format_dot(),
            reason: "DeleteKey at value level in 3-way merge".into(),
        }),
    }
}

fn merge_objects_three(
    base: &BTreeMap<String, BoxValue>,
    left: &BTreeMap<String, BoxValue>,
    right: &BTreeMap<String, BoxValue>,
    config: &BoxConfig,
    options: &MergeOptions,
    path: &PathKey,
) -> Result<BoxValue, MergeError> {
    let mut keys = BTreeSet::new();
    keys.extend(base.keys());
    keys.extend(left.keys());
    keys.extend(right.keys());

    let mut out = BTreeMap::new();
    for k in keys {
        let b = base.get(k);
        let l = left.get(k);
        let r = right.get(k);
        let child = path.push(Segment::Key(k.clone()));

        match (l, r) {
            (None, None) => {}
            (Some(lv), None) => {
                if b == Some(lv) {
                    // deleted on right
                } else {
                    out.insert(k.clone(), lv.clone());
                }
            }
            (None, Some(rv)) => {
                if b == Some(rv) {
                    // deleted on left
                } else {
                    out.insert(k.clone(), rv.clone());
                }
            }
            (Some(lv), Some(rv)) => {
                if rv.is_null() {
                    match options.null_policy {
                        NullMergePolicy::DeleteKey => continue,
                        NullMergePolicy::SetNull => {
                            out.insert(k.clone(), BoxValue::Null);
                        }
                        NullMergePolicy::Ignore => {
                            out.insert(k.clone(), lv.clone());
                        }
                        NullMergePolicy::TreatAsMissing => {
                            let bv = b.cloned().unwrap_or(BoxValue::Null);
                            let m = merge_three_way(&bv, lv, rv, config, options, &child)?;
                            out.insert(k.clone(), m);
                        }
                    }
                    continue;
                }
                let bv = b.cloned().unwrap_or(BoxValue::Null);
                let merged = merge_three_way(&bv, lv, rv, config, options, &child)?;
                out.insert(k.clone(), merged);
            }
        }
    }
    Ok(BoxValue::obj(out))
}

fn merge_arrays_three(
    base: &[BoxValue],
    left: &[BoxValue],
    right: &[BoxValue],
    config: &BoxConfig,
    options: &MergeOptions,
    path: &PathKey,
) -> Result<BoxValue, MergeError> {
    let len = base.len().max(left.len()).max(right.len());
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        let child = path.push(Segment::Index(i));
        let b = base.get(i).cloned().unwrap_or(BoxValue::Null);
        let l = left.get(i).cloned().unwrap_or(BoxValue::Null);
        let r = right.get(i).cloned().unwrap_or(BoxValue::Null);
        out.push(merge_three_way(&b, &l, &r, config, options, &child)?);
    }
    Ok(BoxValue::array(out))
}

fn resolve_scalar_three(
    base: &BoxValue,
    left: &BoxValue,
    right: &BoxValue,
    options: &MergeOptions,
    path: &PathKey,
) -> Result<BoxValue, MergeError> {
    if left == right {
        return Ok(left.clone());
    }
    if base == left {
        return Ok(right.clone());
    }
    if base == right {
        return Ok(left.clone());
    }
    resolve_scalar(left, right, options, path)
}

fn merge_pair(
    left: &BoxValue,
    right: &BoxValue,
    config: &BoxConfig,
    options: &MergeOptions,
    path: &PathKey,
) -> Result<BoxValue, MergeError> {
    if right.is_null() {
        return handle_null(left, right, options, path);
    }
    if left.is_null() {
        return Ok(right.clone());
    }
    if left.kind() != right.kind() {
        return resolve_type_conflict(left, right, config, options, path);
    }
    match (left, right) {
        (BoxValue::Object(l), BoxValue::Object(r)) => merge_objects(l, r, config, options, path),
        (BoxValue::Array(l), BoxValue::Array(r)) => merge_arrays(l, r, config, options, path),
        _ => resolve_scalar(left, right, options, path),
    }
}

fn handle_null(
    left: &BoxValue,
    right: &BoxValue,
    options: &MergeOptions,
    path: &PathKey,
) -> Result<BoxValue, MergeError> {
    let _ = right;
    match options.null_policy {
        NullMergePolicy::Ignore => Ok(left.clone()),
        NullMergePolicy::SetNull => Ok(BoxValue::Null),
        NullMergePolicy::DeleteKey => Err(MergeError::Conflict {
            path: path.format_dot(),
            reason: "DeleteKey requires object parent (use merge at object level)".into(),
        }),
        NullMergePolicy::TreatAsMissing => Ok(left.clone()),
    }
}

fn resolve_type_conflict(
    left: &BoxValue,
    right: &BoxValue,
    config: &BoxConfig,
    options: &MergeOptions,
    path: &PathKey,
) -> Result<BoxValue, MergeError> {
    match options.type_conflict {
        TypeConflictPolicy::Error => Err(MergeError::TypeConflict {
            path: path.format_dot(),
            left: left.type_name().into(),
            right: right.type_name().into(),
        }),
        TypeConflictPolicy::PreferLeft => Ok(left.clone()),
        TypeConflictPolicy::PreferRight => Ok(right.clone()),
        TypeConflictPolicy::ReplaceWithCoercion => coerce_merge(left, right, config, options, path),
        TypeConflictPolicy::WrapAsValue => Ok(BoxValue::obj(
            [
                ("left".into(), left.clone()),
                ("right".into(), right.clone()),
            ]
            .into(),
        )),
    }
}

fn coerce_merge(
    left: &BoxValue,
    right: &BoxValue,
    config: &BoxConfig,
    options: &MergeOptions,
    path: &PathKey,
) -> Result<BoxValue, MergeError> {
    if right.is_object() && !left.is_object() {
        return Ok(right.clone());
    }
    if right.is_array() && !left.is_array() {
        return Ok(right.clone());
    }
    merge_pair(left, right, config, options, path)
}

fn resolve_scalar(
    left: &BoxValue,
    right: &BoxValue,
    options: &MergeOptions,
    path: &PathKey,
) -> Result<BoxValue, MergeError> {
    if left == right {
        return Ok(left.clone());
    }
    if options.scalar == ScalarConflictPolicy::Custom {
        let resolver = options
            .scalar_custom
            .as_ref()
            .ok_or_else(|| MergeError::Conflict {
                path: path.format_dot(),
                reason: "ScalarConflictPolicy::Custom requires MergeOptions.scalar_custom".into(),
            })?;
        return resolver(left, right, &path.format_dot());
    }
    match options.scalar {
        ScalarConflictPolicy::PreferLeft => Ok(left.clone()),
        ScalarConflictPolicy::PreferRight => Ok(right.clone()),
        ScalarConflictPolicy::Error => Err(MergeError::Conflict {
            path: path.format_dot(),
            reason: format!("scalar conflict: {:?} vs {:?}", left, right),
        }),
        ScalarConflictPolicy::PreferTypeOfRight => Ok(right.clone()),
        ScalarConflictPolicy::PreferTypeOfLeft => Ok(left.clone()),
        ScalarConflictPolicy::CoerceToRight => Ok(coerce_to(right, left)),
        ScalarConflictPolicy::CoerceToLeft => Ok(coerce_to(left, right)),
        ScalarConflictPolicy::Custom => unreachable!(),
    }
}

fn coerce_to(target: &BoxValue, source: &BoxValue) -> BoxValue {
    match target.kind() {
        crate::value::ValueKind::Bool => BoxValue::Bool(source.as_bool().unwrap_or(false)),
        crate::value::ValueKind::I64 => BoxValue::I64(source.as_i64().unwrap_or(0)),
        crate::value::ValueKind::F64 => BoxValue::F64(source.as_f64().unwrap_or(0.0)),
        crate::value::ValueKind::String => {
            BoxValue::String(source.as_str().unwrap_or("").to_string())
        }
        _ => target.clone(),
    }
}

fn merge_objects(
    left: &BTreeMap<String, BoxValue>,
    right: &BTreeMap<String, BoxValue>,
    config: &BoxConfig,
    options: &MergeOptions,
    path: &PathKey,
) -> Result<BoxValue, MergeError> {
    let mut out = BTreeMap::new();

    match options.object {
        ObjectMergePolicy::ShallowReplace => {
            out.extend(left.clone());
            for (k, v) in right {
                if v.is_null() {
                    match options.null_policy {
                        NullMergePolicy::DeleteKey => {
                            out.remove(k);
                        }
                        NullMergePolicy::SetNull => {
                            out.insert(k.clone(), BoxValue::Null);
                        }
                        _ => {
                            out.insert(k.clone(), v.clone());
                        }
                    }
                } else {
                    out.insert(k.clone(), v.clone());
                }
            }
            return Ok(BoxValue::obj(out));
        }
        ObjectMergePolicy::MergeKeysPresentInRight => {
            out.extend(left.clone());
            for (k, v) in right {
                if left.contains_key(k) {
                    let child_path = path.push(Segment::Key(k.clone()));
                    let merged = merge_pair(left.get(k).unwrap(), v, config, options, &child_path)?;
                    out.insert(k.clone(), merged);
                } else {
                    out.insert(k.clone(), v.clone());
                }
            }
            return Ok(BoxValue::obj(out));
        }
        _ => {}
    }

    for (k, lv) in left {
        let child_path = path.push(Segment::Key(k.clone()));
        if let Some(rv) = right.get(k) {
            if rv.is_null() {
                match options.null_policy {
                    NullMergePolicy::DeleteKey => continue,
                    NullMergePolicy::SetNull => {
                        out.insert(k.clone(), BoxValue::Null);
                    }
                    NullMergePolicy::Ignore => {
                        out.insert(k.clone(), lv.clone());
                    }
                    NullMergePolicy::TreatAsMissing => {
                        let merged = merge_pair(lv, rv, config, options, &child_path)?;
                        out.insert(k.clone(), merged);
                    }
                }
                continue;
            }
            let merged = match options.object {
                ObjectMergePolicy::ReplaceSubtree => rv.clone(),
                ObjectMergePolicy::Deep
                | ObjectMergePolicy::Union
                | ObjectMergePolicy::MergeKeysPresentInLeft => {
                    merge_pair(lv, rv, config, options, &child_path)?
                }
                ObjectMergePolicy::ShallowReplace | ObjectMergePolicy::MergeKeysPresentInRight => {
                    unreachable!()
                }
            };
            out.insert(k.clone(), merged);
        } else {
            match options.missing_key {
                MissingKeyPolicy::Keep => {
                    out.insert(k.clone(), lv.clone());
                }
                MissingKeyPolicy::Remove => {}
                MissingKeyPolicy::SetNull => {
                    out.insert(k.clone(), BoxValue::Null);
                }
            }
        }
    }

    if matches!(
        options.object,
        ObjectMergePolicy::Deep
            | ObjectMergePolicy::Union
            | ObjectMergePolicy::MergeKeysPresentInRight
    ) {
        for (k, rv) in right {
            if !out.contains_key(k) && !rv.is_null() {
                out.insert(k.clone(), rv.clone());
            }
        }
    }

    Ok(BoxValue::obj(out))
}

fn merge_arrays(
    left: &[BoxValue],
    right: &[BoxValue],
    config: &BoxConfig,
    options: &MergeOptions,
    path: &PathKey,
) -> Result<BoxValue, MergeError> {
    match options.array {
        ArrayMergePolicy::Replace => Ok(BoxValue::array(right.to_vec())),
        ArrayMergePolicy::Concat => {
            let mut v = left.to_vec();
            v.extend_from_slice(right);
            Ok(BoxValue::array(v))
        }
        ArrayMergePolicy::Prepend => {
            let mut v = right.to_vec();
            v.extend_from_slice(left);
            Ok(BoxValue::array(v))
        }
        ArrayMergePolicy::ZipStrict => {
            if left.len() != right.len() {
                return Err(MergeError::ZipStrictLength {
                    path: path.format_dot(),
                });
            }
            zip_merge(left, right, config, options, path, left.len())
        }
        ArrayMergePolicy::MergeByIndex | ArrayMergePolicy::ZipLongest => {
            let len = left.len().max(right.len());
            zip_merge(left, right, config, options, path, len)
        }
        ArrayMergePolicy::LongestCommonSubsequence => {
            let aligned = lcs_align(left, right);
            let mut out = Vec::with_capacity(aligned.len());
            for (li, ri) in aligned {
                let child_path = path.push(Segment::Index(out.len()));
                let merged = match (li, ri) {
                    (Some(l), Some(r)) => merge_pair(l, r, config, options, &child_path)?,
                    (Some(l), None) => pad_missing(l, options, config),
                    (None, Some(r)) => pad_missing(r, options, config),
                    (None, None) => BoxValue::Null,
                };
                out.push(merged);
            }
            Ok(BoxValue::array(out))
        }
        ArrayMergePolicy::UnionByKey => {
            let key_path =
                options
                    .union_by_key_path
                    .as_deref()
                    .ok_or_else(|| MergeError::Conflict {
                        path: path.format_dot(),
                        reason: "UnionByKey requires union_by_key_path".into(),
                    })?;
            union_arrays_by_key(left, right, key_path, config, options, path)
        }
    }
}

fn pad_missing(sample: &BoxValue, options: &MergeOptions, config: &BoxConfig) -> BoxValue {
    match options.zip_pad {
        ZipPadPolicy::FillNull => BoxValue::Null,
        ZipPadPolicy::FillDefaultBox => {
            if config.default_box {
                BoxValue::object()
            } else {
                sample.clone()
            }
        }
        ZipPadPolicy::Truncate => BoxValue::Null,
    }
}

fn zip_merge(
    left: &[BoxValue],
    right: &[BoxValue],
    config: &BoxConfig,
    options: &MergeOptions,
    path: &PathKey,
    len: usize,
) -> Result<BoxValue, MergeError> {
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        let child_path = path.push(Segment::Index(i));
        let l = left.get(i);
        let r = right.get(i);
        let merged = match (l, r) {
            (Some(l), Some(r)) => merge_pair(l, r, config, options, &child_path)?,
            (Some(l), None) => match options.zip_tail {
                ZipTailPolicy::KeepLeft => l.clone(),
                ZipTailPolicy::KeepRight | ZipTailPolicy::Truncate => continue,
                ZipTailPolicy::FillNull => BoxValue::Null,
                ZipTailPolicy::FillDefaultBox => {
                    if config.default_box {
                        BoxValue::object()
                    } else {
                        l.clone()
                    }
                }
            },
            (None, Some(r)) => match options.zip_tail {
                ZipTailPolicy::KeepRight => r.clone(),
                ZipTailPolicy::KeepLeft | ZipTailPolicy::Truncate => continue,
                ZipTailPolicy::FillNull => BoxValue::Null,
                ZipTailPolicy::FillDefaultBox => {
                    if config.default_box {
                        BoxValue::object()
                    } else {
                        r.clone()
                    }
                }
            },
            (None, None) => BoxValue::Null,
        };
        out.push(merged);
    }
    Ok(BoxValue::array(out))
}

fn union_arrays_by_key(
    left: &[BoxValue],
    right: &[BoxValue],
    key_path: &str,
    config: &BoxConfig,
    options: &MergeOptions,
    path: &PathKey,
) -> Result<BoxValue, MergeError> {
    let key = crate::path::parse_path(key_path).map_err(MergeError::Box)?;
    let mut map: BTreeMap<String, BoxValue> = BTreeMap::new();

    for item in left.iter().chain(right.iter()) {
        let id = crate::access::get_at_ref(item, &key, config)
            .and_then(|v| {
                v.as_str()
                    .map(|s| s.to_string())
                    .or_else(|| v.as_i64().map(|n| n.to_string()))
            })
            .ok_or_else(|| MergeError::Conflict {
                path: path.format_dot(),
                reason: format!("UnionByKey missing key at `{key_path}`"),
            })?;
        if let Some(existing) = map.remove(&id) {
            let merged = merge_pair(&existing, item, config, options, path)?;
            map.insert(id, merged);
        } else {
            map.insert(id, item.clone());
        }
    }
    Ok(BoxValue::array(map.into_values().collect::<Vec<_>>()))
}

pub(crate) fn lcs_align<'a>(
    left: &'a [BoxValue],
    right: &'a [BoxValue],
) -> Vec<(Option<&'a BoxValue>, Option<&'a BoxValue>)> {
    let n = left.len();
    let m = right.len();
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if left[i] == right[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut i = 0;
    let mut j = 0;
    let mut out = Vec::new();
    while i < n && j < m {
        if left[i] == right[j] {
            out.push((Some(&left[i]), Some(&right[j])));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            out.push((Some(&left[i]), None));
            i += 1;
        } else {
            out.push((None, Some(&right[j])));
            j += 1;
        }
    }
    while i < n {
        out.push((Some(&left[i]), None));
        i += 1;
    }
    while j < m {
        out.push((None, Some(&right[j])));
        j += 1;
    }
    out
}
