use std::sync::Arc;

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::error::BoxError;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Segment {
    Key(String),
    Index(usize),
}

/// Parsed path, cheap to clone (`Arc` segments).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PathKey {
    segments: Arc<[Segment]>,
}

impl PathKey {
    pub fn empty() -> Self {
        Self {
            segments: Arc::new([]),
        }
    }

    pub fn single_key(key: &str) -> Self {
        Self {
            segments: Arc::from([Segment::Key(key.to_string())]),
        }
    }

    /// A path of plain object keys, without parsing.
    ///
    /// For paths that are constants in the caller: it cannot fail, so a caller
    /// with a fixed path needs neither an `unwrap` nor an error branch that can
    /// never be taken. Keys are used verbatim — no dot-splitting, no `[i]`
    /// syntax — so a key containing a dot addresses that one key, which
    /// `parse_path` could not express without the quoted form.
    pub fn from_keys<S: AsRef<str>>(keys: &[S]) -> Self {
        Self {
            segments: keys
                .iter()
                .map(|k| Segment::Key(k.as_ref().to_string()))
                .collect(),
        }
    }

    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    pub fn parent(&self) -> Self {
        if self.segments.is_empty() {
            Self::empty()
        } else {
            Self {
                segments: Arc::from(&self.segments[..self.segments.len() - 1]),
            }
        }
    }

    pub fn last(&self) -> Option<&Segment> {
        self.segments.last()
    }

    pub fn push(&self, seg: Segment) -> Self {
        let mut v: SmallVec<[Segment; 8]> = self.segments.iter().cloned().collect();
        v.push(seg);
        Self {
            segments: Arc::from(v.as_slice()),
        }
    }

    pub fn format_dot(&self) -> String {
        let mut out = String::new();
        for (i, seg) in self.segments.iter().enumerate() {
            match seg {
                Segment::Key(k) => {
                    if i > 0 {
                        out.push('.');
                    }
                    out.push_str(k);
                }
                Segment::Index(n) => {
                    out.push('[');
                    out.push_str(&n.to_string());
                    out.push(']');
                }
            }
        }
        out
    }
}

pub fn parse_path(input: &str) -> Result<PathKey, BoxError> {
    if input.is_empty() {
        return Ok(PathKey::empty());
    }
    let mut segments: SmallVec<[Segment; 8]> = SmallVec::new();
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'.' {
            i += 1;
            continue;
        }
        if bytes[i] == b'[' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                let start = i + 2;
                let end = input[start..]
                    .find("\"]")
                    .ok_or_else(|| BoxError::InvalidPath(input.to_string()))?;
                let key = &input[start..start + end];
                segments.push(Segment::Key(key.to_string()));
                i = start + end + 2;
                continue;
            }
            let start = i + 1;
            let end = input[start..]
                .find(']')
                .ok_or_else(|| BoxError::InvalidPath(input.to_string()))?;
            let idx_str = &input[start..start + end];
            let idx: usize = idx_str
                .parse()
                .map_err(|_| BoxError::InvalidPath(input.to_string()))?;
            segments.push(Segment::Index(idx));
            i = start + end + 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i] != b'.' && bytes[i] != b'[' {
            i += 1;
        }
        let key = &input[start..i];
        if key.is_empty() {
            return Err(BoxError::InvalidPath(input.to_string()));
        }
        // A bare all-digit segment in a dotted path is an array index
        // (e.g. `inputs.0.value`). Use the quoted form `["0"]` for a literal
        // numeric object key.
        if key.bytes().all(|b| b.is_ascii_digit())
            && let Ok(idx) = key.parse::<usize>()
        {
            segments.push(Segment::Index(idx));
            continue;
        }
        segments.push(Segment::Key(key.to_string()));
    }
    Ok(PathKey {
        segments: Arc::from(segments.as_slice()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_dotted_and_indexed() {
        let p = parse_path("a.b[2].c").unwrap();
        assert_eq!(p.segments().len(), 4);
        assert_eq!(p.format_dot(), "a.b[2].c");
    }

    #[test]
    fn bare_numeric_segment_is_index() {
        let p = parse_path("inputs.0.value").unwrap();
        assert_eq!(p.segments().len(), 3);
        assert!(matches!(p.segments()[1], Segment::Index(0)));
    }

    #[test]
    fn from_keys_takes_each_key_verbatim() {
        let p = PathKey::from_keys(&["tmp", "status"]);
        assert_eq!(p.segments().len(), 2);
        assert_eq!(p.format_dot(), "tmp.status");

        // A dot inside a key is part of the key, not a separator — the case
        // `parse_path` needs the quoted form for.
        let dotted = PathKey::from_keys(&["a.b"]);
        assert_eq!(dotted.segments().len(), 1);
        assert!(matches!(&dotted.segments()[0], Segment::Key(k) if k == "a.b"));

        // A numeric key stays a KEY here, where `parse_path` would read it as
        // an array index.
        let numeric = PathKey::from_keys(&["0"]);
        assert!(matches!(&numeric.segments()[0], Segment::Key(k) if k == "0"));
    }

    #[test]
    fn parse_quoted_key_segment() {
        let p = parse_path(r#"a["weird key"].b"#).unwrap();
        assert_eq!(p.segments().len(), 3);
        assert!(matches!(&p.segments()[1], Segment::Key(k) if k == "weird key"));
    }
}
