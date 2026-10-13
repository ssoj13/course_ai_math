use std::ops::{Index, IndexMut};

use crate::RustBox;
use crate::config::BoxConfig;
use crate::convert::{camel_to_snake, snake_to_camel, storage_key_from_attr};
use crate::convert::{storage_key_for_write, transform_on_read};
use crate::error::BoxError;
use crate::path::{PathKey, Segment};
use crate::value::BoxValue;

pub fn get_at_ref<'a>(
    root: &'a BoxValue,
    path: &PathKey,
    config: &BoxConfig,
) -> Option<&'a BoxValue> {
    if path.is_empty() {
        return Some(root);
    }
    let mut cur = root;
    for seg in path.segments() {
        cur = match seg {
            Segment::Key(k) => resolve_key_get(cur, k, config)?,
            Segment::Index(i) => cur.as_array()?.get(*i)?,
        };
    }
    Some(cur)
}

/// Read path and apply read-time transforms (clone).
pub fn get_at_cloned(root: &BoxValue, path: &PathKey, config: &BoxConfig) -> Option<BoxValue> {
    get_at_ref(root, path, config).map(|v| transform_on_read(v, config))
}

pub fn navigate_mut<'a>(
    root: &'a mut BoxValue,
    path: &PathKey,
    config: &BoxConfig,
    create: bool,
) -> Option<&'a mut BoxValue> {
    if path.is_empty() {
        return Some(root);
    }
    let mut cur = root;
    for (idx, seg) in path.segments().iter().enumerate() {
        let is_last = idx + 1 == path.segments().len();
        match seg {
            Segment::Key(raw_key) => {
                let storage_key = storage_key_for_write(raw_key, config).ok()?;
                if !cur.is_object() {
                    if !create {
                        return None;
                    }
                    *cur = BoxValue::object();
                }
                let map = cur.as_object_mut()?;
                if is_last {
                    if create && !map.contains_key(&storage_key) {
                        map.insert(storage_key.clone(), default_child(config));
                    }
                    return map.get_mut(&storage_key);
                }
                if !map.contains_key(&storage_key) {
                    if !create {
                        return None;
                    }
                    map.insert(storage_key.clone(), default_child(config));
                }
                cur = map.get_mut(&storage_key)?;
            }
            Segment::Index(i) => {
                if !cur.is_array() {
                    if !create {
                        return None;
                    }
                    *cur = BoxValue::array(Vec::new());
                }
                let arr = cur.as_array_mut()?;
                while create && arr.len() <= *i {
                    arr.push(default_child(config));
                }
                if is_last {
                    return arr.get_mut(*i);
                }
                cur = arr.get_mut(*i)?;
            }
        }
    }
    Some(cur)
}

fn resolve_key_get<'a>(cur: &'a BoxValue, key: &str, config: &BoxConfig) -> Option<&'a BoxValue> {
    let obj = cur.as_object()?;
    if let Some(v) = obj.get(key) {
        return Some(v);
    }
    if config.camel_killer_box {
        let snake = camel_to_snake(key);
        if snake != key
            && let Some(v) = obj.get(&snake)
        {
            return Some(v);
        }
        let camel = snake_to_camel(key);
        if camel != key
            && let Some(v) = obj.get(&camel)
        {
            return Some(v);
        }
    }
    if config.conversion_box
        && let Ok(safe) = storage_key_from_attr(key, config)
        && let Some(v) = obj.get(&safe)
    {
        return Some(v);
    }
    None
}

fn default_child(config: &BoxConfig) -> BoxValue {
    if config.default_box {
        BoxValue::object()
    } else {
        BoxValue::Null
    }
}

pub struct BoxRef<'a> {
    owner: &'a RustBox,
    value: &'a BoxValue,
    config: &'a BoxConfig,
}

impl<'a> BoxRef<'a> {
    pub fn new(owner: &'a RustBox, value: &'a BoxValue, key: &str) -> Self {
        let resolved = get_at_ref(value, &PathKey::single_key(key), &owner.config).unwrap_or(value);
        Self {
            owner,
            value: resolved,
            config: &owner.config,
        }
    }

    pub fn get(&self) -> BoxValue {
        transform_on_read(self.value, self.config)
    }

    pub fn attr(&self, key: &str) -> Self {
        let next =
            get_at_ref(self.value, &PathKey::single_key(key), self.config).unwrap_or(self.value);
        Self {
            owner: self.owner,
            value: next,
            config: self.config,
        }
    }

    /// Fluent array navigation: `box.attr("inputs").idx(0).attr("value")`.
    pub fn idx(&self, i: usize) -> Self {
        let next = get_at_ref(
            self.value,
            &PathKey::empty().push(Segment::Index(i)),
            self.config,
        )
        .unwrap_or(self.value);
        Self {
            owner: self.owner,
            value: next,
            config: self.config,
        }
    }
}

impl Index<&str> for BoxValue {
    type Output = BoxValue;

    fn index(&self, key: &str) -> &Self::Output {
        // `.` / `[]` make this a path (namespaces); a plain key is a single
        // object lookup. `BoxValue` carries no config, so default path
        // semantics apply (per-box `box_dots`/`conversion_box` only affect
        // `RustBox` bracket access).
        if key.contains('.') || key.contains('[') {
            let path = crate::path::parse_path(key).expect("box-rs: bad path");
            get_at_ref(self, &path, &BoxConfig::default()).expect("box-rs: path not found")
        } else {
            self.as_object()
                .and_then(|m| m.get(key))
                .expect("box-rs: key not found")
        }
    }
}

impl Index<&str> for BoxRef<'_> {
    type Output = BoxValue;

    fn index(&self, key: &str) -> &Self::Output {
        get_at_ref(self.value, &PathKey::single_key(key), self.config)
            .expect("box-rs: key not found in BoxRef")
    }
}

pub struct BoxMut<'a> {
    owner: &'a mut RustBox,
    path: PathKey,
}

impl<'a> BoxMut<'a> {
    pub fn new(owner: &'a mut RustBox, key: &str) -> Self {
        let path = PathKey::single_key(key);
        if owner.config.default_box_create_on_get {
            let cfg = owner.config.clone();
            let _ = navigate_mut(&mut owner.root, &path, &cfg, true);
        }
        Self { owner, path }
    }

    pub fn attr_mut(mut self, key: &str) -> Result<Self, BoxError> {
        self.owner.config.ensure_mutable()?;
        self.path = self.path.push(Segment::Key(key.to_string()));
        if self.owner.config.default_box_create_on_get {
            let cfg = self.owner.config.clone();
            let _ = navigate_mut(&mut self.owner.root, &self.path, &cfg, true);
        }
        Ok(self)
    }

    pub fn get(&self) -> Option<BoxValue> {
        get_at_cloned(&self.owner.root, &self.path, &self.owner.config)
    }

    pub fn set(&mut self, value: BoxValue) -> Result<(), BoxError> {
        self.owner.set_path(&self.path.format_dot(), value)
    }

    /// Fluent array navigation (mutable): `box.attr_mut("inputs")?.idx_mut(0)?`.
    pub fn idx_mut(mut self, i: usize) -> Result<Self, BoxError> {
        self.owner.config.ensure_mutable()?;
        self.path = self.path.push(Segment::Index(i));
        if self.owner.config.default_box_create_on_get {
            let cfg = self.owner.config.clone();
            let _ = navigate_mut(&mut self.owner.root, &self.path, &cfg, true);
        }
        Ok(self)
    }
}

impl Index<&str> for BoxMut<'_> {
    type Output = BoxValue;

    fn index(&self, key: &str) -> &Self::Output {
        let child = self.path.push(Segment::Key(key.to_string()));
        get_at_ref(&self.owner.root, &child, &self.owner.config)
            .expect("box-rs: key not found in BoxMut")
    }
}

impl IndexMut<&str> for BoxMut<'_> {
    fn index_mut(&mut self, key: &str) -> &mut Self::Output {
        self.owner.config.ensure_mutable().expect("frozen");
        let child = self.path.push(Segment::Key(key.to_string()));
        navigate_mut(&mut self.owner.root, &child, &self.owner.config, true)
            .expect("box-rs: key not found")
    }
}

#[cfg(test)]
mod tests {
    use crate::{BoxValue, RustBox};

    #[test]
    fn boxvalue_index_splits_dots() {
        let mut arr = BoxValue::object();
        if let Some(m) = arr.as_object_mut() {
            m.insert("a".into(), BoxValue::array(vec![BoxValue::from(7)]));
        }
        assert_eq!(arr["a"][0].as_i64(), Some(7)); // single key + usize index
        assert_eq!(arr["a.0"].as_i64(), Some(7)); // dotted path (0 = array index)
    }

    #[test]
    fn fluent_attr_idx_chain() {
        let mut b = RustBox::new();
        b.set_path("inputs[0].value", BoxValue::from(5)).unwrap();
        let r = b.attr("inputs").idx(0).attr("value");
        assert_eq!(r.get().as_i64(), Some(5));
    }
}
