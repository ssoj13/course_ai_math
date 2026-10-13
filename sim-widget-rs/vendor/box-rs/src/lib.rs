//! Nested data container: path/bracket/attr access, merge, diff, subtract.
//!
//! See [`SPEC.md`](../SPEC.md) for the locked policy set.

mod access;
mod config;
#[cfg(feature = "safetensors")]
pub mod container;
mod convert;
mod diff;
mod error;
mod merge;
mod path;
mod subtract;
mod value;

pub use access::{BoxMut, BoxRef};
pub use config::{BoxConfig, BoxDuplicatesPolicy, DEFAULT_BOX_SAFE_PREFIX, RecastRule};
#[cfg(feature = "safetensors")]
pub use container::{ContainerError, load_safetensors, save_safetensors};
pub use diff::{
    ArrayDiffPolicy, Change, ChangeKind, DiffOptions, DiffReport, ObjectDiffPolicy, PathFormat,
    ScalarComparePolicy, ScalarCompareResolver,
};
pub use error::{BoxError, DiffError, MergeError, SubtractError};
pub use merge::{
    ArrayMergePolicy, MergeMode, MergeOptions, MissingKeyPolicy, NullMergePolicy,
    ObjectMergePolicy, ScalarConflictPolicy, ScalarMergeResolver, TypeConflictPolicy, ZipPadPolicy,
    ZipTailPolicy,
};
pub use path::{PathKey, Segment};
pub use subtract::{ArraySubtractPolicy, SubtractMode, SubtractOptions};
pub use value::{BoxList, BoxMap, BoxValue, BufData, DType, ValueKind};

use std::ops::{Index, IndexMut};

use serde::{Deserialize, Serialize};

use crate::access::{get_at_cloned, get_at_ref, navigate_mut};
use crate::convert::{attr_from_storage_key, prepare_value_for_set, storage_key_for_write};
use crate::path::parse_path;

/// Root container (python-box `Box` equivalent).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RustBox {
    #[serde(default)]
    root: BoxValue,
    #[serde(skip)]
    config: BoxConfig,
}

impl Default for RustBox {
    fn default() -> Self {
        Self {
            root: BoxValue::object(),
            config: BoxConfig::default(),
        }
    }
}

impl RustBox {
    pub fn new() -> Self {
        Self::default()
    }

    /// Alias for [`Self::new`] (python-box / nodes-core convention).
    pub fn empty() -> Self {
        Self::new()
    }

    pub fn with_config(config: BoxConfig) -> Self {
        Self {
            root: BoxValue::object(),
            config,
        }
    }

    pub fn from_value(value: BoxValue, config: BoxConfig) -> Self {
        Self {
            root: value,
            config,
        }
    }

    pub fn config(&self) -> &BoxConfig {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut BoxConfig {
        &mut self.config
    }

    pub fn value(&self) -> &BoxValue {
        &self.root
    }

    pub fn value_mut(&mut self) -> &mut BoxValue {
        &mut self.root
    }

    pub fn into_value(self) -> BoxValue {
        self.root
    }

    /// Parse a path once, to reuse with the `*_path_key` methods below.
    ///
    /// Worth doing for a path that is a constant on a hot loop: parsing walks
    /// the string and allocates a `String` per segment plus the segment array,
    /// which on a per-tick write outweighs the lookup it precedes.
    pub fn path_key(path: &str) -> Result<PathKey, BoxError> {
        parse_path(path)
    }

    /// Read at an already-parsed path.
    pub fn get_path_key(&self, key: &PathKey) -> Option<&BoxValue> {
        get_at_ref(&self.root, key, &self.config)
    }

    /// Read at an already-parsed path, with `default_box_none_transform` /
    /// `frozen_box` applied (owned value).
    pub fn get_path_key_cloned(&self, key: &PathKey) -> Option<BoxValue> {
        get_at_cloned(&self.root, key, &self.config)
    }

    /// Write at an already-parsed path. Same rules as [`Self::set_path`],
    /// which is this with the parse in front.
    pub fn set_path_key(&mut self, key: &PathKey, value: BoxValue) -> Result<(), BoxError> {
        self.config.ensure_mutable()?;
        let prepared = prepare_value_for_set(key, value, &self.config)?;
        let slot = navigate_mut(&mut self.root, key, &self.config, true)
            .ok_or_else(|| BoxError::PathNotFound(key.format_dot()))?;
        *slot = prepared;
        Ok(())
    }

    /// Mutable access at an already-parsed path, creating what is missing.
    pub fn get_path_key_mut(&mut self, key: &PathKey) -> Result<&mut BoxValue, BoxError> {
        navigate_mut(&mut self.root, key, &self.config, true)
            .ok_or_else(|| BoxError::PathNotFound(key.format_dot()))
    }

    pub fn get_path(&self, path: &str) -> Option<&BoxValue> {
        let key = parse_path(path).ok()?;
        self.get_path_key(&key)
    }

    /// Read path with `default_box_none_transform` / `frozen_box` applied (owned value).
    pub fn get_path_cloned(&self, path: &str) -> Option<BoxValue> {
        let key = parse_path(path).ok()?;
        self.get_path_key_cloned(&key)
    }

    pub fn get_bool(&self, path: &str) -> Option<bool> {
        self.get_path(path).and_then(|v| v.as_bool())
    }

    pub fn get_i64(&self, path: &str) -> Option<i64> {
        self.get_path(path).and_then(|v| v.as_i64())
    }

    pub fn get_f64(&self, path: &str) -> Option<f64> {
        self.get_path(path).and_then(|v| v.as_f64())
    }

    pub fn get_str(&self, path: &str) -> Option<&str> {
        self.get_path(path).and_then(|v| v.as_str())
    }

    pub fn get_path_mut(&mut self, path: &str) -> Result<&mut BoxValue, BoxError> {
        let key = parse_path(path)?;
        self.get_path_key_mut(&key)
    }

    pub fn set_path(&mut self, path: &str, value: BoxValue) -> Result<(), BoxError> {
        let key = parse_path(path)?;
        self.set_path_key(&key, value)
    }

    pub fn set_f64(&mut self, path: &str, value: f64) -> Result<(), BoxError> {
        self.set_path(path, BoxValue::from(value))
    }

    pub fn set_i64(&mut self, path: &str, value: i64) -> Result<(), BoxError> {
        self.set_path(path, BoxValue::from(value))
    }

    pub fn set_bool(&mut self, path: &str, value: bool) -> Result<(), BoxError> {
        self.set_path(path, BoxValue::from(value))
    }

    pub fn set_str(&mut self, path: &str, value: impl Into<String>) -> Result<(), BoxError> {
        self.set_path(path, BoxValue::from(value.into()))
    }

    /// Remove key/index at path; respects `default_box_no_key_error`.
    pub fn pop(&mut self, path: &str) -> Result<BoxValue, BoxError> {
        self.config.ensure_mutable()?;
        match self.remove_path(path) {
            Ok(v) => Ok(v),
            Err(BoxError::PathNotFound(_)) if self.config.default_box_no_key_error => {
                Ok(BoxValue::Null)
            }
            Err(e) => Err(e),
        }
    }

    pub fn remove_path(&mut self, path: &str) -> Result<BoxValue, BoxError> {
        self.config.ensure_mutable()?;
        let key = parse_path(path)?;
        if key.is_empty() {
            return Err(BoxError::InvalidPath("cannot remove root".into()));
        }
        let parent_path = key.parent();
        let parent = navigate_mut(&mut self.root, &parent_path, &self.config, false)
            .ok_or_else(|| BoxError::PathNotFound(path.to_string()))?;
        match key
            .last()
            .ok_or_else(|| BoxError::InvalidPath(path.to_string()))?
        {
            Segment::Key(k) => {
                let storage = storage_key_for_write(k, &self.config)?;
                parent
                    .as_object_mut()
                    .and_then(|m| m.remove(&storage).or_else(|| m.remove(k)))
                    .ok_or_else(|| BoxError::PathNotFound(path.to_string()))
            }
            Segment::Index(i) => parent
                .as_array_mut()
                .and_then(|a| {
                    if *i < a.len() {
                        Some(a.remove(*i))
                    } else {
                        None
                    }
                })
                .ok_or_else(|| BoxError::PathNotFound(path.to_string())),
        }
    }

    pub fn contains_path(&self, path: &str) -> bool {
        parse_path(path)
            .ok()
            .and_then(|k| get_at_ref(&self.root, &k, &self.config))
            .is_some()
    }

    /// Chain-style attr access: `box.attr("a").attr("b")`.
    pub fn attr(&self, key: &str) -> BoxRef<'_> {
        BoxRef::new(self, &self.root, key)
    }

    pub fn attr_mut(&mut self, key: &str) -> Result<BoxMut<'_>, BoxError> {
        self.config.ensure_mutable()?;
        Ok(BoxMut::new(self, key))
    }

    pub fn merge(&self, other: &Self, options: MergeOptions) -> Result<Self, MergeError> {
        merge::merge_values(&self.root, &other.root, &self.config, &options).map(|root| Self {
            root,
            config: self.config.clone(),
        })
    }

    /// 3-way merge: `base` → common ancestor, `self` = left, `other` = right.
    pub fn merge_symmetric(
        &self,
        base: &Self,
        other: &Self,
        mut options: MergeOptions,
    ) -> Result<Self, MergeError> {
        options.mode = MergeMode::Symmetric;
        options.base = Some(base.root.clone());
        merge::merge_values(&self.root, &other.root, &self.config, &options).map(|root| Self {
            root,
            config: self.config.clone(),
        })
    }

    pub fn merge_into(&mut self, other: &Self, options: MergeOptions) -> Result<(), MergeError> {
        self.config.ensure_mutable()?;
        let merged = merge::merge_values(&self.root, &other.root, &self.config, &options)?;
        self.root = merged;
        Ok(())
    }

    pub fn diff(&self, other: &Self, options: DiffOptions) -> DiffReport {
        diff::diff_values(&self.root, &other.root, &options)
    }

    pub fn subtract(&self, other: &Self, options: SubtractOptions) -> Result<Self, SubtractError> {
        subtract::subtract_values(&self.root, &other.root, &self.config, &options).map(|root| {
            Self {
                root,
                config: self.config.clone(),
            }
        })
    }

    pub fn to_json_value(&self) -> serde_json::Value {
        self.root.to_json_value()
    }

    pub fn from_json_value(value: serde_json::Value, config: BoxConfig) -> Self {
        Self {
            root: BoxValue::from_json_value(value),
            config,
        }
    }

    /// List object keys at `path` (attr names when `conversion_box` is enabled).
    pub fn keys(&self, path: &str) -> Vec<String> {
        self.get_path(path)
            .and_then(|v| v.as_object())
            .map(|m| {
                m.keys()
                    .map(|k| attr_from_storage_key(k, &self.config))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Mark box immutable (`frozen_box = true`). Further writes return [`BoxError::Frozen`].
    pub fn freeze(mut self) -> Self {
        self.config.frozen_box = true;
        self
    }
}

impl Index<&str> for RustBox {
    type Output = BoxValue;

    fn index(&self, key: &str) -> &Self::Output {
        self.bracket_get(key)
            .expect("box-rs: key/path not found (use get_path for Option)")
    }
}

impl IndexMut<&str> for RustBox {
    fn index_mut(&mut self, key: &str) -> &mut Self::Output {
        self.bracket_get_mut(key)
            .expect("box-rs: key/path not found")
    }
}

impl RustBox {
    fn bracket_get(&self, key: &str) -> Option<&BoxValue> {
        if self.config.box_dots && key.contains('.') {
            self.get_path(key)
        } else {
            get_at_ref(&self.root, &PathKey::single_key(key), &self.config)
        }
    }

    fn bracket_get_mut(&mut self, key: &str) -> Option<&mut BoxValue> {
        if self.config.box_dots && key.contains('.') {
            self.get_path_mut(key).ok()
        } else {
            navigate_mut(
                &mut self.root,
                &PathKey::single_key(key),
                &self.config,
                true,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bracket_chain_and_dots() {
        let mut b = RustBox::new();
        b.set_path("a.b.c", BoxValue::from(42)).unwrap();
        assert_eq!(b["a"]["b"]["c"].as_i64(), Some(42));

        let mut dotted = RustBox::with_config(BoxConfig {
            box_dots: true,
            ..Default::default()
        });
        dotted.set_path("x.y", BoxValue::from("ok")).unwrap();
        assert_eq!(dotted["x.y"].as_str(), Some("ok"));
    }

    #[test]
    fn merge_deep_prefer_right() {
        let mut left = RustBox::new();
        left.set_path("a.x", BoxValue::from(1)).unwrap();
        left.set_path("a.y", BoxValue::from(2)).unwrap();

        let mut right = RustBox::new();
        right.set_path("a.y", BoxValue::from(9)).unwrap();
        right.set_path("b", BoxValue::from(3)).unwrap();

        let merged = left
            .merge(
                &right,
                MergeOptions {
                    object: ObjectMergePolicy::Deep,
                    scalar: ScalarConflictPolicy::PreferRight,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(merged.get_path("a.x").unwrap().as_i64(), Some(1));
        assert_eq!(merged.get_path("a.y").unwrap().as_i64(), Some(9));
        assert_eq!(merged.get_path("b").unwrap().as_i64(), Some(3));
    }

    #[test]
    fn diff_and_subtract() {
        let mut a = RustBox::new();
        a.set_path("keep", BoxValue::from(1)).unwrap();
        a.set_path("gone", BoxValue::from(2)).unwrap();

        let mut b = RustBox::new();
        b.set_path("keep", BoxValue::from(1)).unwrap();
        b.set_path("new", BoxValue::from(3)).unwrap();

        let report = a.diff(
            &b,
            DiffOptions {
                array: ArrayDiffPolicy::ByIndex,
                ..Default::default()
            },
        );
        assert!(report.changes.iter().any(|c| c.kind == ChangeKind::Removed));
        assert!(report.changes.iter().any(|c| c.kind == ChangeKind::Added));

        let sub = a
            .subtract(
                &b,
                SubtractOptions {
                    mode: SubtractMode::StructuralMinus,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(!sub.contains_path("new"));
    }

    #[test]
    fn symmetric_three_way_merge() {
        let base = RustBox::from_json_value(
            serde_json::json!({"a": 1, "b": 2, "c": 3}),
            BoxConfig::default(),
        );
        let left = RustBox::from_json_value(
            serde_json::json!({"a": 1, "b": 20, "c": 3}),
            BoxConfig::default(),
        );
        let right = RustBox::from_json_value(
            serde_json::json!({"a": 10, "b": 2, "c": 3}),
            BoxConfig::default(),
        );
        let merged = left
            .merge_symmetric(&base, &right, MergeOptions::default())
            .unwrap();
        assert_eq!(merged.get_path("a").unwrap().as_i64(), Some(10));
        assert_eq!(merged.get_path("b").unwrap().as_i64(), Some(20));
        assert_eq!(merged.get_path("c").unwrap().as_i64(), Some(3));
    }

    #[test]
    fn overlay_root_keys() {
        let mut left = RustBox::new();
        left.set_path("a.x", BoxValue::from(1)).unwrap();
        left.set_path("b", BoxValue::from(2)).unwrap();
        let mut right = RustBox::new();
        right.set_path("a.y", BoxValue::from(9)).unwrap();
        right.set_path("b", BoxValue::from(3)).unwrap();
        let merged = left
            .merge(
                &right,
                MergeOptions {
                    mode: MergeMode::Overlay,
                    object: ObjectMergePolicy::Deep,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(merged.get_path("a.x").unwrap().as_i64(), Some(1));
        assert_eq!(merged.get_path("a.y").unwrap().as_i64(), Some(9));
        assert_eq!(merged.get_path("b").unwrap().as_i64(), Some(3));
    }

    #[test]
    fn box_recast_on_set() {
        let mut cfg = BoxConfig::default();
        cfg.box_recast.insert("count".into(), RecastRule::I64);
        let mut b = RustBox::with_config(cfg);
        b.set_path("count", BoxValue::from("42")).unwrap();
        assert_eq!(b.get_i64("count"), Some(42));
    }

    #[test]
    fn frozen_rejects_write() {
        let mut b = RustBox::new().freeze();
        assert!(matches!(
            b.set_path("x", BoxValue::from(1)),
            Err(BoxError::Frozen)
        ));
    }

    #[test]
    fn diff_apply_patch() {
        let mut a = RustBox::new();
        a.set_path("x", BoxValue::from(1)).unwrap();
        let mut b = RustBox::new();
        b.set_path("x", BoxValue::from(2)).unwrap();
        b.set_path("y", BoxValue::from(3)).unwrap();
        let report = a.diff(&b, DiffOptions::default());
        report.apply_to(&mut a).unwrap();
        assert_eq!(a.get_i64("x"), Some(2));
        assert_eq!(a.get_i64("y"), Some(3));
    }
}
