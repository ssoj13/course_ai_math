use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::BoxError;

pub const DEFAULT_BOX_SAFE_PREFIX: &str = "x";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoxDuplicatesPolicy {
    Ignore,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RecastRule {
    Bool,
    I64,
    F64,
    String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoxConfig {
    pub conversion_box: bool,
    pub box_safe_prefix: String,
    pub box_duplicates: BoxDuplicatesPolicy,
    pub default_box: bool,
    pub default_box_create_on_get: bool,
    pub default_box_none_transform: bool,
    pub default_box_no_key_error: bool,
    pub box_dots: bool,
    pub camel_killer_box: bool,
    pub frozen_box: bool,
    pub box_recast: HashMap<String, RecastRule>,
    pub box_intact_types: Vec<String>,
}

impl Default for BoxConfig {
    fn default() -> Self {
        Self {
            conversion_box: true,
            box_safe_prefix: DEFAULT_BOX_SAFE_PREFIX.to_string(),
            box_duplicates: BoxDuplicatesPolicy::Ignore,
            default_box: false,
            default_box_create_on_get: true,
            default_box_none_transform: true,
            default_box_no_key_error: true,
            box_dots: false,
            camel_killer_box: false,
            frozen_box: false,
            box_recast: HashMap::new(),
            box_intact_types: Vec::new(),
        }
    }
}

impl BoxConfig {
    pub fn ensure_mutable(&self) -> Result<(), BoxError> {
        if self.frozen_box {
            Err(BoxError::Frozen)
        } else {
            Ok(())
        }
    }
}
