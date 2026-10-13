//! Minimal **`ch("…")` path extraction** (P4). Full expression grammar is intentionally out of scope.
//!
//! Helpers for resolving channel paths against [`crate::Subnet`] and [`RustBox`] parameters.

use crate::EditFocus;
use crate::address::{parse_address, resolve_address};
use crate::error::{CoreError, CoreResult};
use crate::registry::NodeTypeRegistry;
use crate::subnet::Subnet;

/// Extract the inner path string from a `ch("/path/to/node.parm")` or `ch('…')` call.
///
/// Leading/trailing whitespace is ignored on the outer string. Nested `ch()` inside the path is unsupported.
pub fn extract_ch_path(input: &str) -> Option<&str> {
    let mut s = input.trim();
    s = trim_prefix_ci(s, "ch")?;
    s = s.trim_start();
    s = s.strip_prefix('(')?;
    s = s.trim_start();
    // Either quote style opens the path; the closer has to match the opener,
    // so the choice is carried rather than searched for again.
    let (end_quote, inner) = match s.strip_prefix('"') {
        Some(rest) => ('"', rest),
        None => ('\'', s.strip_prefix('\'')?),
    };
    let end = inner.find(end_quote)?;
    Some(&inner[..end])
}

fn trim_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let p = prefix.as_bytes();
    let b = s.as_bytes();
    if b.len() < p.len() {
        return None;
    }
    if b[..p.len()].eq_ignore_ascii_case(p) {
        Some(&s[p.len()..])
    } else {
        None
    }
}

/// Parse the path inside `ch("…")` as an [`crate::address::OpAddress`].
pub fn parse_ch_address(input: &str) -> CoreResult<crate::address::OpAddress> {
    let inner = extract_ch_path(input).ok_or_else(|| {
        CoreError::InvalidPath(format!("expected ch(\"…\") or ch('…'), got {input:?}"))
    })?;
    parse_address(inner)
}

/// Resolve a `ch("…")` expression using the same rules as [`resolve_address`].
pub fn resolve_ch<'a>(
    root: &'a Subnet,
    focus_subnet: &'a Subnet,
    registry: &'a NodeTypeRegistry,
    input: &str,
) -> CoreResult<crate::address::ResolvedTarget<'a>> {
    let addr = parse_ch_address(input)?;
    resolve_address(root, focus_subnet, registry, &addr)
}

/// Resolve `ch("…")` after applying relative path rules from [`crate::address::resolve_relative`].
pub fn resolve_ch_relative<'a>(
    root: &'a Subnet,
    focus: &EditFocus,
    focus_subnet: &'a Subnet,
    registry: &'a NodeTypeRegistry,
    input: &str,
) -> CoreResult<crate::address::ResolvedTarget<'a>> {
    let inner = extract_ch_path(input).ok_or_else(|| {
        CoreError::InvalidPath(format!("expected ch(\"…\") or ch('…'), got {input:?}"))
    })?;
    crate::address::resolve_relative(root, focus, focus_subnet, registry, inner)
}
