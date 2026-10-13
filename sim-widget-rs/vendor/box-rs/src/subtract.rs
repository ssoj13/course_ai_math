use serde::{Deserialize, Serialize};

use crate::config::BoxConfig;
use crate::diff::{DiffOptions, values_equal};
use crate::error::SubtractError;
use crate::merge::{MergeOptions, merge_values};
use crate::path::{PathKey, Segment};
use crate::value::BoxValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubtractMode {
    KeyDifference,
    ValueMatch,
    StructuralMinus,
    AntiMerge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArraySubtractPolicy {
    RemoveMatchingElements,
    IndexMask,
    ByDiff,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtractOptions {
    pub mode: SubtractMode,
    pub array: ArraySubtractPolicy,
    pub diff: DiffOptions,
    pub anti_merge: MergeOptions,
}

impl Default for SubtractOptions {
    fn default() -> Self {
        Self {
            mode: SubtractMode::KeyDifference,
            array: ArraySubtractPolicy::RemoveMatchingElements,
            diff: DiffOptions::default(),
            anti_merge: MergeOptions::default(),
        }
    }
}

pub fn subtract_values(
    left: &BoxValue,
    right: &BoxValue,
    config: &BoxConfig,
    options: &SubtractOptions,
) -> Result<BoxValue, SubtractError> {
    match options.mode {
        SubtractMode::KeyDifference => subtract_key_diff(left, right, options),
        SubtractMode::ValueMatch => subtract_value_match(left, right, options),
        SubtractMode::StructuralMinus => {
            let report = crate::diff::diff_values(left, right, &options.diff);
            apply_removals(left, &report.changes)
        }
        SubtractMode::AntiMerge => anti_merge(left, right, config, options),
    }
}

fn subtract_key_diff(
    left: &BoxValue,
    right: &BoxValue,
    options: &SubtractOptions,
) -> Result<BoxValue, SubtractError> {
    match (left, right) {
        (BoxValue::Object(l), BoxValue::Object(r)) => {
            let mut out = (**l).clone();
            for k in r.keys() {
                out.remove(k);
            }
            Ok(BoxValue::obj(out))
        }
        (BoxValue::Array(l), BoxValue::Array(r)) => subtract_arrays(l, r, options),
        _ => Ok(left.clone()),
    }
}

fn subtract_value_match(
    left: &BoxValue,
    right: &BoxValue,
    options: &SubtractOptions,
) -> Result<BoxValue, SubtractError> {
    match (left, right) {
        (BoxValue::Object(l), BoxValue::Object(r)) => {
            let mut out = (**l).clone();
            for (k, rv) in r.iter() {
                if let Some(lv) = l.get(k) {
                    if values_equal(lv, rv, &options.diff) {
                        out.remove(k);
                    } else {
                        out.insert(k.clone(), subtract_value_match(lv, rv, options)?);
                    }
                }
            }
            Ok(BoxValue::obj(out))
        }
        (BoxValue::Array(l), BoxValue::Array(r)) => {
            let mut out = Vec::new();
            for item in l.iter() {
                let remove = r.iter().any(|rv| values_equal(item, rv, &options.diff));
                if !remove {
                    out.push(item.clone());
                }
            }
            Ok(BoxValue::array(out))
        }
        _ => {
            if values_equal(left, right, &options.diff) {
                Ok(BoxValue::Null)
            } else {
                Ok(left.clone())
            }
        }
    }
}

fn subtract_arrays(
    left: &[BoxValue],
    right: &[BoxValue],
    options: &SubtractOptions,
) -> Result<BoxValue, SubtractError> {
    match options.array {
        ArraySubtractPolicy::RemoveMatchingElements => {
            let mut out = Vec::new();
            for item in left {
                let remove = right.iter().any(|r| values_equal(item, r, &options.diff));
                if !remove {
                    out.push(item.clone());
                }
            }
            Ok(BoxValue::array(out))
        }
        ArraySubtractPolicy::IndexMask => {
            let mut indices: Vec<usize> = right
                .iter()
                .filter_map(|v| v.as_i64().map(|n| n as usize))
                .collect();
            indices.sort_unstable();
            indices.dedup();
            let mut out = Vec::new();
            for (i, item) in left.iter().enumerate() {
                if !indices.contains(&i) {
                    out.push(item.clone());
                }
            }
            Ok(BoxValue::array(out))
        }
        ArraySubtractPolicy::ByDiff => {
            let report = crate::diff::diff_values(
                &BoxValue::array(left.to_vec()),
                &BoxValue::array(right.to_vec()),
                &options.diff,
            );
            apply_removals(&BoxValue::array(left.to_vec()), &report.changes)
        }
    }
}

fn apply_removals(
    left: &BoxValue,
    changes: &[crate::diff::Change],
) -> Result<BoxValue, SubtractError> {
    use crate::diff::ChangeKind;

    let mut out = left.clone();
    for ch in changes {
        if !matches!(
            ch.kind,
            ChangeKind::Removed | ChangeKind::Changed | ChangeKind::TypeChanged
        ) {
            continue;
        }
        let path = crate::path::parse_path(&ch.path).map_err(|e| SubtractError::Failed {
            path: ch.path.clone(),
            reason: e.to_string(),
        })?;
        remove_at(&mut out, &path)?;
    }
    Ok(out)
}

fn remove_at(value: &mut BoxValue, path: &PathKey) -> Result<(), SubtractError> {
    if path.is_empty() {
        *value = BoxValue::Null;
        return Ok(());
    }
    let parent_path = path.parent();
    let parent = navigate_mut(value, &parent_path).ok_or_else(|| SubtractError::Failed {
        path: path.format_dot(),
        reason: "parent not found".into(),
    })?;
    match path.last() {
        Some(Segment::Key(k)) => {
            if let Some(map) = parent.as_object_mut() {
                map.remove(k);
            }
        }
        Some(Segment::Index(i)) => {
            if let Some(arr) = parent.as_array_mut()
                && *i < arr.len()
            {
                arr.remove(*i);
            }
        }
        None => {}
    }
    Ok(())
}

fn navigate_mut<'a>(value: &'a mut BoxValue, path: &PathKey) -> Option<&'a mut BoxValue> {
    if path.is_empty() {
        return Some(value);
    }
    let mut cur = value;
    for seg in path.segments() {
        cur = match seg {
            Segment::Key(k) => cur.as_object_mut()?.get_mut(k)?,
            Segment::Index(i) => cur.as_array_mut()?.get_mut(*i)?,
        };
    }
    Some(cur)
}

fn anti_merge(
    left: &BoxValue,
    right: &BoxValue,
    config: &BoxConfig,
    options: &SubtractOptions,
) -> Result<BoxValue, SubtractError> {
    let merged =
        merge_values(left, right, config, &options.anti_merge).map_err(SubtractError::Merge)?;
    let report = crate::diff::diff_values(&merged, left, &options.diff);
    apply_removals(left, &report.changes)
}
