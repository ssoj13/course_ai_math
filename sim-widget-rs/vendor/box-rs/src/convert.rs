use crate::config::{BoxConfig, BoxDuplicatesPolicy, RecastRule};
use crate::error::BoxError;
use crate::path::PathKey;
use crate::value::BoxValue;

const RESERVED: &[&str] = &[
    "get", "set", "merge", "diff", "keys", "values", "items", "pop", "clear", "copy", "update",
];

pub fn storage_key_from_attr(attr: &str, config: &BoxConfig) -> Result<String, BoxError> {
    if RESERVED.contains(&attr) {
        return Err(BoxError::ReservedKey(attr.to_string()));
    }
    if !config.conversion_box {
        return Ok(attr.to_string());
    }
    Ok(to_safe_attr_key(attr, &config.box_safe_prefix))
}

/// Key used when inserting into object maps (attr / path segment).
pub fn storage_key_for_write(key: &str, config: &BoxConfig) -> Result<String, BoxError> {
    let raw = if config.camel_killer_box {
        camel_to_snake(key)
    } else {
        key.to_string()
    };
    if config.conversion_box {
        storage_key_from_attr(&raw, config)
    } else {
        Ok(raw)
    }
}

pub fn attr_from_storage_key(key: &str, config: &BoxConfig) -> String {
    if !config.conversion_box {
        return key.to_string();
    }
    if key.starts_with(&config.box_safe_prefix) {
        return key[config.box_safe_prefix.len()..].to_string();
    }
    key.to_string()
}

fn to_safe_attr_key(raw: &str, prefix: &str) -> String {
    let mut out = String::new();
    let mut first = true;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            if first && ch.is_ascii_digit() {
                out.push_str(prefix);
            }
            out.push(ch);
            first = false;
        } else if ch.is_whitespace() {
            if !out.is_empty() {
                out.push('_');
            }
            first = false;
        }
    }
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    if out.is_empty() {
        out.push_str(prefix);
        out.push('k');
    }
    if RESERVED.contains(&out.as_str()) {
        out.insert_str(0, prefix);
    }
    out
}

pub fn check_duplicate_attr(a: &str, b: &str, config: &BoxConfig) -> Result<(), BoxError> {
    if !config.conversion_box {
        return Ok(());
    }
    let sa = to_safe_attr_key(a, &config.box_safe_prefix);
    let sb = to_safe_attr_key(b, &config.box_safe_prefix);
    if sa == sb && a != b {
        match config.box_duplicates {
            BoxDuplicatesPolicy::Ignore => {}
            BoxDuplicatesPolicy::Warn => {
                let _ = (a, b, sa);
            }
            BoxDuplicatesPolicy::Error => {
                return Err(BoxError::DuplicateAttribute {
                    key: a.to_string(),
                    other: b.to_string(),
                });
            }
        }
    }
    Ok(())
}

pub fn value_is_intact(value: &BoxValue, config: &BoxConfig) -> bool {
    if config.box_intact_types.is_empty() {
        return false;
    }
    let tag = value.type_name();
    config.box_intact_types.iter().any(|t| t == tag)
}

/// Apply `box_recast`, intact-type guard, and duplicate checks before assignment.
pub fn prepare_value_for_set(
    path: &PathKey,
    value: BoxValue,
    config: &BoxConfig,
) -> Result<BoxValue, BoxError> {
    if value_is_intact(&value, config) {
        return Ok(value);
    }
    if let Some(crate::path::Segment::Key(k)) = path.last() {
        let storage = storage_key_for_write(k, config)?;
        if k != &storage {
            check_duplicate_attr(k, &storage, config)?;
        }
    }
    let dot = path.format_dot();
    let recast = config
        .box_recast
        .get(&dot)
        .or_else(|| {
            path.last().and_then(|seg| match seg {
                crate::path::Segment::Key(k) => config.box_recast.get(k),
                _ => None,
            })
        })
        .cloned();
    if let Some(rule) = recast {
        return recast_value(value, &rule, &dot);
    }
    Ok(value)
}

pub fn recast_value(value: BoxValue, rule: &RecastRule, path: &str) -> Result<BoxValue, BoxError> {
    match rule {
        RecastRule::Bool => {
            value
                .as_bool()
                .map(BoxValue::Bool)
                .ok_or_else(|| BoxError::RecastFailed {
                    path: path.into(),
                    reason: "expected bool-coercible value".into(),
                })
        }
        RecastRule::I64 => {
            if let Some(n) = value.as_i64() {
                return Ok(BoxValue::I64(n));
            }
            if let Some(s) = value.as_str() {
                return s
                    .parse::<i64>()
                    .map(BoxValue::I64)
                    .map_err(|_| BoxError::RecastFailed {
                        path: path.into(),
                        reason: "expected integer-coercible value".into(),
                    });
            }
            if let Some(f) = value.as_f64() {
                return Ok(BoxValue::I64(f as i64));
            }
            Err(BoxError::RecastFailed {
                path: path.into(),
                reason: "expected integer-coercible value".into(),
            })
        }
        RecastRule::F64 => {
            if let Some(f) = value.as_f64() {
                return Ok(BoxValue::F64(f));
            }
            if let Some(s) = value.as_str() {
                return s
                    .parse::<f64>()
                    .map(BoxValue::F64)
                    .map_err(|_| BoxError::RecastFailed {
                        path: path.into(),
                        reason: "expected float-coercible value".into(),
                    });
            }
            if let Some(n) = value.as_i64() {
                return Ok(BoxValue::F64(n as f64));
            }
            Err(BoxError::RecastFailed {
                path: path.into(),
                reason: "expected float-coercible value".into(),
            })
        }
        RecastRule::String => Ok(BoxValue::String(match value {
            BoxValue::String(s) => s,
            BoxValue::Null => String::new(),
            BoxValue::Bool(b) => b.to_string(),
            BoxValue::I64(n) => n.to_string(),
            BoxValue::F64(f) => f.to_string(),
            other => other.to_json_value().to_string(),
        })),
    }
}

/// Transform value on read (`default_box_none_transform`, `default_box`).
pub fn transform_on_read(value: &BoxValue, config: &BoxConfig) -> BoxValue {
    if value.is_null() && config.default_box_none_transform && config.default_box {
        return BoxValue::object();
    }
    if config.frozen_box {
        return deep_freeze_clone(value);
    }
    value.clone()
}

/// An immutable snapshot: nothing reachable from the result can be changed
/// through the original, or the other way round.
///
/// This used to walk the tree rebuilding every `Vec` and `BTreeMap`, because
/// the containers were owned inline and a shallow clone would have shared
/// nothing — there was simply no sharing to speak of. Now that `Array` and
/// `Object` are copy-on-write ([`BoxValue`]), a plain clone already gives
/// exactly that guarantee: the two handles share the interior until one of them
/// writes, and the write copies. So the deep walk would produce an identical
/// value at O(tree) cost, and this is kept as the name for the guarantee rather
/// than for the work it once did.
pub fn deep_freeze_clone(value: &BoxValue) -> BoxValue {
    value.clone()
}

pub fn camel_to_snake(s: &str) -> String {
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

pub fn snake_to_camel(s: &str) -> String {
    let mut out = String::new();
    let mut upper_next = false;
    for ch in s.chars() {
        if ch == '_' {
            upper_next = true;
            continue;
        }
        if upper_next {
            out.push(ch.to_ascii_uppercase());
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}
