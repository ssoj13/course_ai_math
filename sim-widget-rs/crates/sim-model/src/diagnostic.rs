use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCode {
    Schema,
    Limit,
    InvalidId,
    NonFinite,
    Type,
    Unit,
    Range,
    Reference,
    Duplicate,
    Cycle,
    Capability,
    Singular,
    Overflow,
    Time,
    StaleRevision,
    Transaction,
    History,
    Permission,
    Replay,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub path: String,
    pub message: String,
    #[serde(default)]
    pub affected: Vec<String>,
}
impl Diagnostic {
    pub fn new(code: DiagnosticCode, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code,
            path: path.into(),
            message: message.into(),
            affected: vec![],
        }
    }
    pub fn affected(mut self, ids: impl IntoIterator<Item = String>) -> Self {
        self.affected = ids.into_iter().collect();
        self
    }
    pub fn at(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }
}
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} at {}: {}", self.code, self.path, self.message)
    }
}
impl std::error::Error for Diagnostic {}
pub type Result<T> = std::result::Result<T, Diagnostic>;
