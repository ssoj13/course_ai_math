//! Free helper functions for the OCIO config parser.
//!
//! Extracted from `config.rs` during the STRUCT.2 split (TODO6.md).
//! Three groups of helpers live here:
//! - **YAML accessors** (`yaml_str`, `yaml_bool`, `yaml_int`,
//!   `yaml_str_list`, `yaml_get`, etc.) — typed
//!   readers over the saphyr YAML node tree.
//! - **OCIO field parsers** (`parse_environment`, `parse_search_paths`,
//!   `parse_luma`,
//!   `parse_bit_depth`, `parse_direction`, `parse_interpolation`,
//!   `parse_negative_style`, `parse_fixed_function_style`,
//!   `scalar_f64`, `ocio_f64_vec`, `vec4_or_scalar`, `log_param`, `parse_grading_rgbm`,
//!   `parse_grading_rgbmsw`, `parse_bspline_curve` (OCIO's value shapes and messages),
//!   `parse_shared_views`, `parse_virtual_display`, `parse_viewing_rules`,
//!   and friends) — turn YAML fragments into
//!   typed config items.
//! - **Path / file rule / ICC helpers** (`convert_to_regular_expression`,
//!   `build_file_rule_regex`, `file_rule_basic_matches`,
//!   `is_archivable_path`, `supported_lut_extensions`,
//!   `resolve_icc_profile_path`, `icc_search_paths`, `find_icc_in_dir`,
//!   `normalize_path`, `unwrap_tagged`,
//!   `resolve_env_value`, `unresolved_env_vars`,
//!   `bit_depth_to_ocio_str`, `split_search_path_string`,
//!   `yaml_f32_list`).
//!
//! All marked `pub(super)` so the parent `impl Config` can call them
//! across the file boundary.

use super::*;
use saphyr::{Scalar, Yaml};
use std::env;
use std::path::{Path, PathBuf};

/// Ports OCIO's `ConvertToRegularExpression` (`FileRules.cpp:56-193`).
///
/// Translates a glob pattern into a regex fragment. When `ignore_case` is
/// true (used for the extension half of a rule — ref always passes `true`
/// there, `false` for the path-pattern half), every ASCII letter is
/// expanded into a `[aA]`-style bracket class *unless the glob itself
/// already contains a `[`, `*` or `?` anywhere* — in that case the ref
/// leaves the whole string untouched (case-sensitive) rather than folding
/// it, exactly matching the quirky "respectCase" escape hatch in the C++.
pub(super) fn convert_to_regular_expression(
    glob_pattern: &str,
    ignore_case: bool,
) -> Option<String> {
    let working: String = if ignore_case {
        let respect_case = glob_pattern.chars().any(|c| matches!(c, '[' | '*' | '?'));
        if respect_case {
            glob_pattern.to_string()
        } else {
            let mut s = String::new();
            for c in glob_pattern.chars() {
                if c.is_ascii_alphabetic() {
                    s.push('[');
                    s.push(c.to_ascii_lowercase());
                    s.push(c.to_ascii_uppercase());
                    s.push(']');
                } else {
                    s.push(c);
                }
            }
            s
        }
    } else {
        glob_pattern.to_string()
    };

    let chars: Vec<char> = working.chars().collect();
    let n = chars.len();
    let mut out = String::new();
    let mut i = 0;
    while i < n {
        match chars[i] {
            '.' => out.push_str("\\."),
            '?' => out.push('.'),
            '*' => out.push_str(".*"),
            '+' => out.push_str("\\+"),
            '^' => out.push_str("\\^"),
            '$' => out.push_str("\\$"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            '|' => out.push_str("\\|"),
            ']' => return None, // unmatched ']' — invalid, same as ref's ThrowInvalidRegex
            '[' => {
                // Bracket expression: copy verbatim to the matching ']',
                // translating '!' -> '^' and escaping regex metacharacters
                // inside it (ref FileRules.cpp:119-176).
                let mut sub = String::from("[");
                let mut end = i + 1;
                while end < n && chars[end] != ']' {
                    let ec = chars[end];
                    match ec {
                        '!' => sub.push('^'),
                        '+' | '^' | '$' | '{' | '}' | '(' | ')' | '|' => {
                            sub.push('\\');
                            sub.push(ec);
                        }
                        '\\' => sub.push_str("\\\\"),
                        '.' | '?' | '*' => {
                            // Only legal escaped (ref requires the preceding
                            // char to be a literal backslash in the glob).
                            if end == 0 || chars[end - 1] != '\\' {
                                return None;
                            }
                            sub.push(ec);
                        }
                        '[' => return None,
                        _ => sub.push(ec),
                    }
                    end += 1;
                }
                if end >= n {
                    return None;
                }
                sub.push(']');
                if sub == "[]" || sub == "[^]" {
                    return None;
                }
                out.push_str(&sub);
                i = end + 1;
                continue;
            }
            c => out.push(c),
        }
        i += 1;
    }
    Some(out)
}

/// Ports OCIO's `BuildRegularExpression` (`FileRules.cpp:205-260`): a Basic
/// file rule is ONE regex, `^(pattern)(\.extension)$`, matched against the
/// WHOLE path — not a glob match on the path followed by a separate,
/// independent extension comparison. An empty pattern becomes `(.*)`; an
/// empty extension becomes `(\..*)` (i.e. the path must still end in
/// SOME `.something` — extension is never truly optional in the reference).
/// The path-pattern half is case-sensitive, the extension half is
/// case-insensitive (`ConvertToRegularExpression`'s `ignoreCase` argument).
pub(super) fn build_file_rule_regex(pattern: &str, extension: &str) -> Option<String> {
    let mut s = String::from("^(");
    if pattern.is_empty() {
        s.push_str("(.*)");
    } else {
        s.push('(');
        s.push_str(&convert_to_regular_expression(pattern, false)?);
        s.push(')');
    }
    if extension.is_empty() {
        s.push_str("(\\..*)");
    } else {
        s.push_str("(\\.");
        s.push_str(&convert_to_regular_expression(extension, true)?);
        s.push(')');
    }
    s.push_str(")$");
    Some(s)
}

/// Ports OCIO's `ParseColorSpaceFromString` (`PathUtils.cpp:182-224`), used
/// by the `ColorSpaceNamePathSearch` file rule and by
/// `Config::parseColorSpaceFromString`. Scans `path` (case-insensitively)
/// for every colorspace name and alias, and returns the one whose match
/// ends FURTHEST RIGHT in the string — ties broken by the LONGEST matching
/// name. Returns `None` when nothing matches (ref returns `-1`).
pub(super) fn parse_colorspace_from_string<'a>(
    colorspaces: &'a [ColorSpace],
    path: &str,
) -> Option<&'a str> {
    let full = path.to_lowercase();
    let mut right_most_end: i64 = -1;
    let mut right_most_len: usize = 0;
    let mut result: Option<&'a str> = None;

    let mut candidates: Vec<(String, &'a str)> = Vec::new();
    for cs in colorspaces {
        let name = cs.name();
        candidates.push((name.to_lowercase(), name));
        for alias in cs.aliases() {
            candidates.push((alias.to_lowercase(), name));
        }
    }

    for (candidate_lower, real_name) in &candidates {
        if candidate_lower.is_empty() {
            continue;
        }
        // Right-most occurrence: rfind on the lowercased haystack.
        if let Some(start) = full.rfind(candidate_lower.as_str()) {
            let end = (start + candidate_lower.len()) as i64;
            if end > right_most_end
                || (end == right_most_end && candidate_lower.len() > right_most_len)
            {
                right_most_end = end;
                right_most_len = candidate_lower.len();
                result = Some(real_name);
            }
        }
    }

    result
}

/// Matches a Basic file rule's pattern+extension against a path using the
/// single combined regex `build_file_rule_regex` produces — the one true
/// implementation; do not re-derive pattern/extension matching elsewhere
/// (that produced three divergent copies before, see config.rs history).
pub(super) fn file_rule_basic_matches(
    file_path: &str,
    pattern: &str,
    extension: Option<&str>,
) -> bool {
    let ext = extension.unwrap_or("").trim_start_matches('.');
    let Some(expr) = build_file_rule_regex(pattern, ext) else {
        return false;
    };
    let Ok(re) = regex::Regex::new(&expr) else {
        return false;
    };
    re.is_match(file_path)
}

pub(super) fn is_archivable_path(working_dir: &Path, path: &Path) -> bool {
    let candidate = if let Ok(rel) = path.strip_prefix(working_dir) {
        rel.to_path_buf()
    } else {
        path.to_path_buf()
    };

    if candidate.is_absolute() {
        return false;
    }

    let mut had_curdir_prefix = false;
    let mut first_non_curdir: Option<String> = None;
    for comp in candidate.components() {
        match comp {
            std::path::Component::CurDir => {
                if first_non_curdir.is_none() {
                    had_curdir_prefix = true;
                }
            }
            std::path::Component::ParentDir => return false,
            std::path::Component::RootDir | std::path::Component::Prefix(_) => return false,
            std::path::Component::Normal(p) => {
                first_non_curdir = Some(p.to_string_lossy().to_string());
                break;
            }
        }
    }

    if let Some(first) = first_non_curdir
        && (first.starts_with('$') || first.starts_with('%'))
        && !had_curdir_prefix
    {
        return false;
    }

    true
}

pub(super) fn supported_lut_extensions() -> [&'static str; 10] {
    [
        "cube", "3dl", "csp", "spi1d", "spi3d", "clf", "ctf", "cc", "ccc", "cdl",
    ]
}

pub(super) fn resolve_icc_profile_path(monitor_name: &str) -> OcioResult<PathBuf> {
    let direct = PathBuf::from(monitor_name);
    if direct.exists() {
        return Ok(direct);
    }

    let needle = monitor_name.to_ascii_lowercase();
    let mut candidates: Vec<PathBuf> = Vec::new();
    for dir in icc_search_paths() {
        if dir.exists() {
            find_icc_in_dir(&dir, &needle, &mut candidates);
        }
    }
    if let Some(path) = candidates.into_iter().next() {
        return Ok(path);
    }

    Err(OcioError::Validation(format!(
        "Unable to resolve ICC profile for monitor '{}'",
        monitor_name
    )))
}

pub(super) fn icc_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    #[cfg(target_os = "windows")]
    {
        paths.push(PathBuf::from(r"C:\Windows\System32\spool\drivers\color"));
    }
    #[cfg(target_os = "macos")]
    {
        paths.push(PathBuf::from("/Library/ColorSync/Profiles"));
        if let Some(home) = std::env::var_os("HOME") {
            paths.push(PathBuf::from(home).join("Library/ColorSync/Profiles"));
        }
    }
    #[cfg(target_os = "linux")]
    {
        paths.push(PathBuf::from("/usr/share/color/icc"));
        paths.push(PathBuf::from("/usr/local/share/color/icc"));
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            paths.push(home.join(".local/share/color/icc"));
            paths.push(home.join(".color/icc"));
        }
    }
    paths
}

pub(super) fn find_icc_in_dir(dir: &Path, needle: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            find_icc_in_dir(&path, needle, out);
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if file_name == needle
            || file_name == format!("{}.icc", needle)
            || file_name == format!("{}.icm", needle)
        {
            out.push(path);
            return;
        }
    }
}

// ============================================================================
// YAML helper functions for saphyr
// ============================================================================

/// Unwraps a tagged value, returning the inner value.
pub(super) fn unwrap_tagged<'a>(yaml: &'a Yaml<'a>) -> &'a Yaml<'a> {
    match yaml {
        Yaml::Tagged(_, inner) => inner.as_ref(),
        _ => yaml,
    }
}

/// Gets a string value from a YAML mapping.
pub(super) fn yaml_str<'a>(yaml: &'a Yaml<'a>, key: &str) -> Option<&'a str> {
    yaml_get(yaml, key).and_then(|v| yaml_as_str(v))
}

/// Gets a boolean value from a YAML mapping.
pub(super) fn yaml_bool<'a>(yaml: &'a Yaml<'a>, key: &str) -> Option<bool> {
    yaml_get(yaml, key).and_then(|v| match unwrap_tagged(v) {
        Yaml::Value(Scalar::Boolean(b)) => Some(*b),
        _ => None,
    })
}

/// Gets a list of f64 values from a YAML mapping.
pub(super) fn yaml_f64_list<'a>(yaml: &'a Yaml<'a>, key: &str) -> Vec<f64> {
    yaml_get(yaml, key)
        .map(|v| match unwrap_tagged(v) {
            Yaml::Sequence(seq) => seq.iter().filter_map(|x| yaml_as_f64(x)).collect(),
            // A single number reads as a 1-element list.
            scalar => yaml_as_f64(scalar).map(|n| vec![n]).unwrap_or_default(),
        })
        .unwrap_or_default()
}

/// Gets a list of strings from a YAML mapping.
pub(super) fn yaml_str_list<'a>(yaml: &'a Yaml<'a>, key: &str) -> Vec<String> {
    yaml_get(yaml, key)
        .map(|v| match unwrap_tagged(v) {
            Yaml::Sequence(seq) => seq
                .iter()
                .filter_map(|x| yaml_as_str(x).map(|s: &str| s.to_string()))
                .collect(),
            _ => Vec::new(),
        })
        .unwrap_or_default()
}

/// Gets a value from a YAML mapping by key.
pub(super) fn yaml_get<'a>(yaml: &'a Yaml<'a>, key: &str) -> Option<&'a Yaml<'a>> {
    match unwrap_tagged(yaml) {
        Yaml::Mapping(map) => {
            for (k, v) in map.iter() {
                if yaml_as_str(k) == Some(key) {
                    return Some(v);
                }
            }
            None
        }
        _ => None,
    }
}

/// Converts a YAML value to a string.
pub(super) fn yaml_as_str<'a>(yaml: &'a Yaml<'a>) -> Option<&'a str> {
    match unwrap_tagged(yaml) {
        Yaml::Value(Scalar::String(s)) => Some(s.as_ref()),
        // Handle raw representation (unresolved scalars)
        Yaml::Representation(s, _, _) => Some(s.as_ref()),
        _ => None,
    }
}

/// Converts a YAML value to a String (owned), handling numbers too.
pub(super) fn yaml_to_string<'a>(yaml: &'a Yaml<'a>) -> Option<String> {
    match unwrap_tagged(yaml) {
        Yaml::Value(Scalar::String(s)) => Some(s.to_string()),
        Yaml::Value(Scalar::Integer(i)) => Some(i.to_string()),
        Yaml::Value(Scalar::FloatingPoint(f)) => Some(f.to_string()),
        Yaml::Representation(s, _, _) => Some(s.to_string()),
        _ => None,
    }
}

/// Converts a YAML value to f64.
pub(super) fn yaml_as_f64<'a>(yaml: &'a Yaml<'a>) -> Option<f64> {
    match unwrap_tagged(yaml) {
        Yaml::Value(Scalar::Integer(i)) => Some(*i as f64),
        Yaml::Value(Scalar::FloatingPoint(f)) => Some(f.into_inner()),
        _ => None,
    }
}

// ============================================================================
// Config parsing helpers (environment + search paths)
// ============================================================================

pub(super) fn parse_environment<'a>(
    root: &'a Yaml<'a>,
) -> (Vec<(String, String)>, Option<EnvironmentMode>) {
    // OCIO: an `environment` map predefines the variables (ENV_ENVIRONMENT_LOAD_PREDEFINED);
    // without one, every process variable is loaded (ENV_ENVIRONMENT_LOAD_ALL).
    let mut vars = Vec::new();
    let Some(env_yaml) = yaml_get(root, "environment") else {
        return (vars, Some(EnvironmentMode::LoadAll));
    };
    if let Yaml::Mapping(map) = unwrap_tagged(env_yaml) {
        for (k, v) in map.iter() {
            if let (Some(key), Some(value)) = (yaml_as_str(k), yaml_to_string(v)) {
                vars.push((key.to_string(), value));
            }
        }
    }
    (vars, Some(EnvironmentMode::LoadPredefined))
}

pub(super) fn parse_search_paths<'a>(
    root: &'a Yaml<'a>,
    working_dir: &Path,
    context: &Context,
) -> Vec<PathBuf> {
    // OCIO also reads the v1 spelling `resource_path`.
    let Some(paths_yaml) =
        yaml_get(root, "search_path").or_else(|| yaml_get(root, "resource_path"))
    else {
        return Vec::new();
    };

    match unwrap_tagged(paths_yaml) {
        Yaml::Sequence(seq) => seq
            .iter()
            .filter_map(|v| yaml_as_str(v))
            .map(|p| context.resolve(p))
            .filter(|p| !p.is_empty())
            .map(|p| working_dir.join(p))
            .collect(),
        _ => {
            let Some(raw) = yaml_as_str(paths_yaml) else {
                return Vec::new();
            };
            split_search_path_string(raw)
                .into_iter()
                .map(|p| context.resolve(p))
                .filter(|p| !p.is_empty())
                .map(|p| working_dir.join(p))
                .collect()
        }
    }
}

pub(super) fn split_search_path_string(raw: &str) -> Vec<&str> {
    if raw.contains(';') {
        return raw.split(';').filter(|p| !p.is_empty()).collect();
    }
    if raw.contains(':') {
        if raw.len() >= 2 && raw.as_bytes()[1] == b':' {
            return vec![raw];
        }
        return raw.split(':').filter(|p| !p.is_empty()).collect();
    }
    vec![raw]
}

/// OCIO `SanitizeNewlines` (`OCIOYaml.cpp`): descriptions drop their trailing newlines on load
/// and save, because YAML reads them back inconsistently (a `|` block keeps exactly one).
pub(crate) fn sanitize_description(s: &str) -> String {
    s.trim_end_matches('\n').to_string()
}

/// OCIO `saveDescription`: nothing for an empty description, a `|` literal block for a
/// multi-line one, a (quoted if needed) scalar otherwise. `indent` is the key's indentation.
pub(super) fn write_description(out: &mut String, indent: &str, desc: &str) {
    let desc = sanitize_description(desc);
    if desc.is_empty() {
        return;
    }
    if desc.contains('\n') {
        out.push_str(indent);
        out.push_str("description: |\n");
        for line in desc.split('\n') {
            if !line.is_empty() {
                out.push_str(indent);
                out.push_str("  ");
                out.push_str(line);
            }
            out.push('\n');
        }
    } else {
        let needs_quotes = desc
            .starts_with(['"', '\'', '|', '>', '!', '&', '*', '-', '[', '@', '`'])
            || desc
                .chars()
                .any(|c| c == ':' || c == '#' || c == '{' || c == '}' || c == '\\')
            || desc != desc.trim();
        if needs_quotes {
            out.push_str(&format!(
                "{indent}description: \"{}\"\n",
                yaml_dq_escape(&desc)
            ));
        } else {
            out.push_str(&format!("{indent}description: {desc}\n"));
        }
    }
}

/// Escapes `\` and `"` for a double-quoted YAML scalar.
pub(super) fn yaml_dq_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

pub(super) fn resolve_env_value(value: &str) -> Result<String, Vec<String>> {
    let mut result = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    let mut unresolved = Vec::new();

    while let Some(c) = chars.next() {
        if c == '$' {
            if chars.peek() == Some(&'{') {
                chars.next();
                let var_name: String = chars.by_ref().take_while(|&c| c != '}').collect();
                if let Ok(env_val) = env::var(&var_name) {
                    result.push_str(&env_val);
                } else {
                    unresolved.push(var_name.clone());
                    result.push_str("${");
                    result.push_str(&var_name);
                    result.push('}');
                }
            } else {
                let mut var_name = String::new();
                while let Some(&ch) = chars.peek() {
                    if ch.is_alphanumeric() || ch == '_' {
                        var_name.push(ch);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if var_name.is_empty() {
                    result.push('$');
                } else if let Ok(env_val) = env::var(&var_name) {
                    result.push_str(&env_val);
                } else {
                    unresolved.push(var_name.clone());
                    result.push('$');
                    result.push_str(&var_name);
                }
            }
        } else if c == '%' {
            let mut var_name = String::new();
            let mut valid = true;
            let mut found_end = false;
            for ch in chars.by_ref() {
                if ch == '%' {
                    found_end = true;
                    break;
                }
                if !(ch.is_alphanumeric() || ch == '_') {
                    valid = false;
                }
                var_name.push(ch);
            }
            if found_end && valid && !var_name.is_empty() {
                if let Ok(env_val) = env::var(&var_name) {
                    result.push_str(&env_val);
                } else {
                    unresolved.push(var_name.clone());
                    result.push('%');
                    result.push_str(&var_name);
                    result.push('%');
                }
            } else {
                result.push('%');
                result.push_str(&var_name);
                if found_end {
                    result.push('%');
                }
            }
        } else {
            result.push(c);
        }
    }

    if unresolved.is_empty() {
        Ok(result)
    } else {
        Err(unresolved)
    }
}

pub(super) fn unresolved_env_vars(value: &str) -> Vec<String> {
    let mut unresolved = Vec::new();
    let mut chars = value.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '$' {
            if chars.peek() == Some(&'{') {
                chars.next();
                let var_name: String = chars.by_ref().take_while(|&c| c != '}').collect();
                if !var_name.is_empty() && env::var(&var_name).is_err() {
                    unresolved.push(var_name);
                }
            } else {
                let mut var_name = String::new();
                while let Some(&ch) = chars.peek() {
                    if ch.is_alphanumeric() || ch == '_' {
                        var_name.push(ch);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if !var_name.is_empty() && env::var(&var_name).is_err() {
                    unresolved.push(var_name);
                }
            }
        } else if c == '%' {
            let mut var_name = String::new();
            let mut valid = true;
            let mut found_end = false;
            for ch in chars.by_ref() {
                if ch == '%' {
                    found_end = true;
                    break;
                }
                if !(ch.is_alphanumeric() || ch == '_') {
                    valid = false;
                }
                var_name.push(ch);
            }
            if found_end && valid && !var_name.is_empty() && env::var(&var_name).is_err() {
                unresolved.push(var_name);
            }
        }
    }

    unresolved
}

// ============================================================================
// Helper functions for parsing transform data
// ============================================================================

pub(super) fn parse_luma(root: &Yaml) -> Option<[f32; 3]> {
    let vals = yaml_f64_list(root, "luma");
    if vals.len() == 3 {
        Some([vals[0] as f32, vals[1] as f32, vals[2] as f32])
    } else {
        None
    }
}

pub(super) fn parse_bit_depth(s: &str) -> vfx_core::BitDepth {
    // OCIO `BitDepthFromString` (`ParseUtils.cpp`): these names, case-insensitive; anything else
    // (and `14ui`, which this port has no depth for) is unknown.
    match s.to_lowercase().as_str() {
        "8ui" => vfx_core::BitDepth::U8,
        "10ui" => vfx_core::BitDepth::U10,
        "12ui" => vfx_core::BitDepth::U12,
        "16ui" => vfx_core::BitDepth::U16,
        "32ui" => vfx_core::BitDepth::U32,
        "16f" => vfx_core::BitDepth::F16,
        "32f" => vfx_core::BitDepth::F32,
        _ => vfx_core::BitDepth::Unknown,
    }
}

pub(super) fn bit_depth_to_ocio_str(depth: vfx_core::BitDepth) -> &'static str {
    // OCIO `BitDepthToString`.
    match depth {
        vfx_core::BitDepth::Unknown => "unknown",
        vfx_core::BitDepth::U8 => "8ui",
        vfx_core::BitDepth::U10 => "10ui",
        vfx_core::BitDepth::U12 => "12ui",
        vfx_core::BitDepth::U16 => "16ui",
        vfx_core::BitDepth::U32 => "32ui",
        vfx_core::BitDepth::F16 => "16f",
        vfx_core::BitDepth::F32 => "32f",
    }
}

/// Parses direction string to TransformDirection.
pub(super) fn parse_direction(dir: Option<&str>) -> OcioResult<TransformDirection> {
    // OCIO `TransformDirectionFromString` (`ParseUtils.cpp:140`): case-insensitive, and an
    // unknown string fails the load. Absent means forward.
    match dir.map(|d| (d, d.to_lowercase())) {
        None => Ok(TransformDirection::Forward),
        Some((_, l)) if l == "forward" => Ok(TransformDirection::Forward),
        Some((_, l)) if l == "inverse" => Ok(TransformDirection::Inverse),
        Some((d, _)) => Err(invalid(format!("Unrecognized transform direction: '{d}'."))),
    }
}

/// A value OCIO's YAML reader rejects: fails the whole load (see `parse_optional_transform`).
fn invalid(msg: String) -> OcioError {
    OcioError::Validation(msg)
}

/// A case-insensitive lookup of `s` in `table`, or OCIO's error message.
fn lookup<T: Copy>(s: &str, table: &[(&str, T)], err: impl FnOnce() -> String) -> OcioResult<T> {
    let l = s.to_lowercase();
    table
        .iter()
        .find(|(k, _)| *k == l)
        .map(|(_, v)| *v)
        .ok_or_else(|| invalid(err()))
}

/// OCIO `CDLStyleFromString` (`ParseUtils.cpp:320`).
pub(super) fn parse_cdl_style(s: &str) -> OcioResult<CdlStyle> {
    lookup(
        s,
        &[("asc", CdlStyle::AscCdl), ("noclamp", CdlStyle::NoClamp)],
        || format!("Wrong CDL style: '{s}'."),
    )
}

/// OCIO `RangeStyleFromString` (`ParseUtils.cpp:340`).
pub(super) fn parse_range_style(s: &str) -> OcioResult<RangeStyle> {
    lookup(
        s,
        &[
            ("noclamp", RangeStyle::NoClamp),
            ("clamp", RangeStyle::Clamp),
        ],
        || format!("Wrong Range style '{s}'."),
    )
}

/// OCIO `ExposureContrastStyleFromString` (`ParseUtils.cpp:478`).
pub(super) fn parse_exposure_contrast_style(s: &str) -> OcioResult<ExposureContrastStyle> {
    lookup(
        s,
        &[
            ("linear", ExposureContrastStyle::Linear),
            ("video", ExposureContrastStyle::Video),
            ("log", ExposureContrastStyle::Logarithmic),
        ],
        || format!("Unknown exposure contrast style: '{s}'."),
    )
}

/// OCIO `GradingStyleFromString` (`ParseUtils.cpp:443`): `linear` / `log` / `video`, as the
/// index 0 / 1 / 2 the three grading transforms map to their own style enums.
pub(super) fn parse_grading_style(s: &str) -> OcioResult<usize> {
    lookup(s, &[("linear", 0), ("log", 1), ("video", 2)], || {
        format!("Unknown grading style: '{s}'.")
    })
}

/// Parses interpolation string.
pub(super) fn parse_interpolation(interp: Option<&str>) -> OcioResult<Interpolation> {
    // OCIO `InterpolationFromString`: case-insensitive; absent is INTERP_DEFAULT. `cubic` and
    // unknown names load in OCIO and fail when the LUT is built; this port has no cubic and
    // refuses them at load.
    let Some(s) = interp else {
        return Ok(Interpolation::Default);
    };
    lookup(
        s,
        &[
            ("nearest", Interpolation::Nearest),
            ("linear", Interpolation::Linear),
            ("tetrahedral", Interpolation::Tetrahedral),
            ("best", Interpolation::Best),
            ("default", Interpolation::Default),
        ],
        || format!("Unsupported interpolation '{s}'."),
    )
}

/// Parses NegativeStyle string for ExponentTransform.
pub(super) fn parse_negative_style(style: &str) -> OcioResult<NegativeStyle> {
    // OCIO `NegativeStyleFromString` (`ParseUtils.cpp:515`).
    lookup(
        style,
        &[
            ("mirror", NegativeStyle::Mirror),
            ("pass_thru", NegativeStyle::PassThru),
            ("clamp", NegativeStyle::Clamp),
            ("linear", NegativeStyle::Linear),
        ],
        || format!("Unknown exponent style: '{style}'."),
    )
}

/// Parses FixedFunction style string.
pub(super) fn parse_fixed_function_style(style: &str) -> OcioResult<FixedFunctionStyle> {
    // OCIO `FixedFunctionStyleFromString` (`ParseUtils.cpp:390`): exactly these names,
    // case-insensitive. The inverse styles are the forward name with `direction: inverse`.
    use FixedFunctionStyle as F;
    lookup(
        style,
        &[
            ("aces_redmod03", F::AcesRedMod03),
            ("aces_redmod10", F::AcesRedMod10),
            ("aces_glow03", F::AcesGlow03),
            ("aces_glow10", F::AcesGlow10),
            ("aces_darktodim10", F::AcesDarkToDim10),
            ("aces_gamutcomp13", F::AcesGamutComp13),
            ("aces2_outputtransform", F::AcesOutputTransform20),
            ("aces2_rgb_to_jmh", F::AcesRgbToJmh20),
            ("aces2_tonescalecompress", F::AcesTonescaleCompress20),
            ("aces2_gamutcompress", F::AcesGamutCompress20),
            ("rec2100_surround", F::Rec2100Surround),
            ("rgb_to_hsv", F::RgbToHsv),
            ("xyz_to_xyy", F::XyzToXyy),
            ("xyz_to_uvy", F::XyzToUvy),
            ("xyz_to_luv", F::XyzToLuv),
            ("lin_to_pq", F::LinToPq),
            ("lin_to_gammalog", F::LinToGammaLog),
            ("lin_to_doublelog", F::LinToDoubleLog),
            ("rgb_to_hsy_lin", F::RgbToHsyLin),
            ("rgb_to_hsy_log", F::RgbToHsyLog),
            ("rgb_to_hsy_vid", F::RgbToHsyVid),
        ],
        || format!("Unknown Fixed FunctionOp style: '{style}'."),
    )
}

/// Parses shared_views from YAML (OCIO v2.3+).
///
/// Returns a list of [`View`] entries — vfx-rs models shared views as
/// regular `View`s stored on the [`Config`] rather than on a `Display`,
/// matching OCIO C++ `Config::Impl::m_sharedViews: std::vector<View>`.
/// The `<USE_DISPLAY_NAME>` colorspace sentinel is resolved into a
/// typed `use_display_name` flag at parse time so it never leaks past
/// the loader.
pub(super) fn parse_shared_views(root: &Yaml) -> Vec<crate::display::View> {
    use crate::display::View;
    let mut views = Vec::new();
    if let Some(Yaml::Sequence(seq)) = yaml_get(root, "shared_views") {
        for item in seq {
            let item = unwrap_tagged(item);
            if let Some(name) = yaml_str(item, "name") {
                let raw_cs = yaml_str(item, "colorspace")
                    .or_else(|| yaml_str(item, "display_colorspace"))
                    .unwrap_or("");
                let use_display_name = raw_cs == "<USE_DISPLAY_NAME>";
                let colorspace = if use_display_name {
                    String::new()
                } else {
                    raw_cs.to_string()
                };
                let mut v = View::new(name, colorspace).with_use_display_name(use_display_name);
                if let Some(vt) = yaml_str(item, "view_transform")
                    && !vt.is_empty()
                {
                    v = v.with_view_transform(vt);
                }
                if let Some(looks) = yaml_str(item, "looks").or_else(|| yaml_str(item, "look"))
                    && !looks.is_empty()
                {
                    v = v.with_look(looks);
                }
                if let Some(rule) = yaml_str(item, "rule")
                    && !rule.is_empty()
                {
                    v = v.with_rule(rule);
                }
                if let Some(desc) = yaml_str(item, "description")
                    && !desc.is_empty()
                {
                    v = v.with_description(desc);
                }
                views.push(v);
            }
        }
    }
    views
}

/// Parses virtual_display from YAML (OCIO v2.0+).
///
/// Virtual display serves as a template for ICC-profile-instantiated displays.
/// Example YAML:
/// ```yaml
/// virtual_display:
///   - !<View> {name: ACES, view_transform: ACES-sdr-video,
///              display_colorspace: <USE_DISPLAY_NAME>}
///   - !<View> {name: Log, colorspace: lg10}
///   - !<Views> [ Raw ]
/// ```
pub(super) fn parse_virtual_display(root: &Yaml) -> Option<VirtualDisplay> {
    let seq = match yaml_get(root, "virtual_display") {
        Some(Yaml::Sequence(seq)) => seq,
        _ => return None,
    };

    let mut views = Vec::new();
    let mut shared_view_refs = Vec::new();

    for item in seq {
        if let Yaml::Tagged(tag, inner) = item
            && tag.suffix == "Views"
        {
            if let Yaml::Sequence(refs) = inner.as_ref() {
                for r in refs {
                    if let Some(name) = yaml_to_string(r) {
                        shared_view_refs.push(name);
                    }
                }
            }
            continue;
        }

        let item = unwrap_tagged(item);

        // Parse individual view (!<View>)
        if let Some(name) = yaml_str(item, "name") {
            let display_colorspace = yaml_str(item, "colorspace")
                .or_else(|| yaml_str(item, "display_colorspace"))
                .unwrap_or("")
                .to_string();
            views.push(VirtualDisplayView {
                name: name.to_string(),
                view_transform: yaml_str(item, "view_transform").map(|s| s.to_string()),
                display_colorspace,
                looks: yaml_str(item, "looks")
                    .or_else(|| yaml_str(item, "look"))
                    .map(|s| s.to_string()),
                rule: yaml_str(item, "rule").map(|s| s.to_string()),
                description: yaml_str(item, "description").map(|s| s.to_string()),
            });
        }
    }

    if views.is_empty() && shared_view_refs.is_empty() {
        None
    } else {
        Some(VirtualDisplay {
            views,
            shared_view_refs,
        })
    }
}

/// Parses viewing_rules from YAML (OCIO v2.0+).
///
/// Viewing rules define which views are applicable based on colorspace encoding.
/// Example YAML:
/// ```yaml
/// viewing_rules:
///   - !<Rule> {name: Any Scene-linear or Log, encodings: [log, scene-linear]}
///   - !<Rule> {name: Any Video, encodings: [sdr-video, hdr-video]}
///   - !<Rule> {name: ACEScg Only, colorspaces: [ACEScg]}
/// ```
pub(super) fn parse_viewing_rules(root: &Yaml) -> Vec<ViewingRule> {
    let mut rules = Vec::new();
    if let Some(Yaml::Sequence(seq)) = yaml_get(root, "viewing_rules") {
        for item in seq {
            let item = unwrap_tagged(item);
            if let Some(name) = yaml_str(item, "name") {
                // Parse colorspaces list
                // A single value is allowed for both lists (OCIO load(ViewingRules)).
                let one_or_list = |key: &str| match yaml_str(item, key) {
                    Some(one) => vec![one.to_string()],
                    None => yaml_str_list(item, key),
                };
                let colorspaces = one_or_list("colorspaces");

                // Parse encodings list
                let encodings = one_or_list("encodings");

                let custom_keys = parse_custom_keys(item);

                rules.push(ViewingRule {
                    name: name.to_string(),
                    colorspaces,
                    encodings,
                    custom_keys,
                });
            }
        }
    }
    rules
}

pub(super) fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

// ============================================================================
// Transform value readers with OCIO's shapes and messages (`OCIOYaml.cpp`)
// ============================================================================

/// `throwValueError(node.Tag(), key, msg)`.
pub(super) fn value_error(tag: &str, key: &str, msg: &str) -> OcioError {
    invalid(format!(
        "The value parsing of the key '{key}' from '{tag}' failed: {msg}"
    ))
}

/// A present, non-null value of `key`.
fn present<'a>(yaml: &'a Yaml<'a>, key: &str) -> Option<&'a Yaml<'a>> {
    yaml_get(yaml, key)
        .map(unwrap_tagged)
        .filter(|v| !matches!(v, Yaml::Value(Scalar::Null)))
}

/// A scalar number (`load(node, double&)`); `None` when absent.
pub(super) fn scalar_f64(yaml: &Yaml, key: &str, tag: &str) -> OcioResult<Option<f64>> {
    match present(yaml, key) {
        None => Ok(None),
        Some(v) => yaml_as_f64(v)
            .map(Some)
            .ok_or_else(|| value_error(tag, key, "expected a single number")),
    }
}

/// A sequence of numbers (`load(node, std::vector<double>&)`), of one of the `lens` when given.
pub(super) fn ocio_f64_vec(
    yaml: &Yaml,
    key: &str,
    lens: &[usize],
    tag: &str,
) -> OcioResult<Option<Vec<f64>>> {
    let Some(v) = present(yaml, key) else {
        return Ok(None);
    };
    let Yaml::Sequence(seq) = v else {
        return Err(value_error(tag, key, "expected a sequence of numbers"));
    };
    let vals = seq
        .iter()
        .map(|x| yaml_as_f64(unwrap_tagged(x)))
        .collect::<Option<Vec<f64>>>()
        .ok_or_else(|| value_error(tag, key, "expected a sequence of numbers"))?;
    if !lens.is_empty() && !lens.contains(&vals.len()) {
        return Err(value_error(
            tag,
            key,
            &format!(
                "'{key}' values must be {} floats. Found '{}'.",
                lens[0],
                vals.len()
            ),
        ));
    }
    Ok(Some(vals))
}

/// Four values, or one value broadcast to R, G, B with `alpha` for A (ExponentTransform `value`,
/// ExponentWithLinearTransform `gamma` / `offset`).
pub(super) fn vec4_or_scalar(
    yaml: &Yaml,
    key: &str,
    alpha: f64,
    tag: &str,
) -> OcioResult<Option<[f64; 4]>> {
    match present(yaml, key) {
        None => Ok(None),
        Some(Yaml::Sequence(_)) => {
            Ok(ocio_f64_vec(yaml, key, &[4], tag)?.map(|v| [v[0], v[1], v[2], v[3]]))
        }
        Some(v) => {
            let s = yaml_as_f64(v).ok_or_else(|| value_error(tag, key, "expected a number"))?;
            Ok(Some([s, s, s, alpha]))
        }
    }
}

/// `loadLogParam`: one value for the three channels, or exactly three.
pub(super) fn log_param(yaml: &Yaml, key: &str, tag: &str) -> OcioResult<Option<[f64; 3]>> {
    match present(yaml, key) {
        None => Ok(None),
        Some(Yaml::Sequence(seq)) => {
            let vals = ocio_f64_vec(yaml, key, &[], tag)?.unwrap_or_default();
            if vals.len() != 3 {
                return Err(invalid(format!(
                    "LogAffine/CameraTransform parse error, {key} value field must have 3 components. Found '{}'.",
                    seq.len()
                )));
            }
            Ok(Some([vals[0], vals[1], vals[2]]))
        }
        Some(v) => {
            let s = yaml_as_f64(v).ok_or_else(|| value_error(tag, key, "expected a number"))?;
            Ok(Some([s; 3]))
        }
    }
}

/// A nested mapping value (`pivot`, `clamp`, an RGBM, a curve).
pub(super) fn sub_map<'a>(node: &'a Yaml<'a>, tag: &str, key: &str) -> OcioResult<&'a Yaml<'a>> {
    let node = unwrap_tagged(node);
    if matches!(node, Yaml::Mapping(_)) {
        Ok(node)
    } else {
        Err(value_error(tag, key, "The value needs to be a map."))
    }
}

/// A `GradingRGBM` value `{rgb: [r, g, b], master: m}`, both required.
pub(super) fn parse_grading_rgbm(
    yaml: &Yaml,
    key: &str,
    tag: &str,
) -> OcioResult<Option<crate::dynamic::GradingRgbm>> {
    let Some(node) = present(yaml, key) else {
        return Ok(None);
    };
    let node = sub_map(node, tag, key)?;
    let rgb = ocio_f64_vec(node, "rgb", &[3], tag)?;
    let master = scalar_f64(node, "master", tag)?;
    match (rgb, master) {
        (Some(rgb), Some(m)) => Ok(Some(crate::dynamic::GradingRgbm::new(
            rgb[0], rgb[1], rgb[2], m,
        ))),
        _ => Err(value_error(
            tag,
            key,
            "Both rgb and master values are required.",
        )),
    }
}

/// A `GradingRGBMSW` zone `{rgb, master, start|center, width|pivot}`, all required.
pub(super) fn parse_grading_rgbmsw(
    yaml: &Yaml,
    key: &str,
    tag: &str,
    center: bool,
    pivot: bool,
) -> OcioResult<Option<GradingRgbmsw>> {
    let Some(node) = present(yaml, key) else {
        return Ok(None);
    };
    let node = sub_map(node, tag, key)?;
    let (start_key, width_key) = (
        if center { "center" } else { "start" },
        if pivot { "pivot" } else { "width" },
    );
    let rgb = ocio_f64_vec(node, "rgb", &[3], tag)?;
    let master = scalar_f64(node, "master", tag)?;
    let start = scalar_f64(node, start_key, tag)?;
    let width = scalar_f64(node, width_key, tag)?;
    match (rgb, master, start, width) {
        (Some(rgb), Some(master), Some(start), Some(width)) => Ok(Some(GradingRgbmsw {
            red: rgb[0],
            green: rgb[1],
            blue: rgb[2],
            master,
            start,
            width,
        })),
        _ => Err(value_error(
            tag,
            key,
            &format!("Rgb, master, {start_key}, and {width_key} values are required."),
        )),
    }
}

/// A parsed B-spline curve: its `(x, y)` control points and its per-point slopes.
pub(super) type BsplineCurve = (Vec<[f64; 2]>, Vec<f64>);

/// A B-spline curve `{control_points: [x0, y0, x1, y1, ...], slopes: [...]}` (control points
/// required; OCIO stores them as `float`). All-zero slopes are OCIO's "default" (computed).
pub(super) fn parse_bspline_curve(
    yaml: &Yaml,
    key: &str,
    tag: &str,
) -> OcioResult<Option<BsplineCurve>> {
    let Some(node) = present(yaml, key) else {
        return Ok(None);
    };
    let node = sub_map(node, tag, key)?;
    let Some(vals) = ocio_f64_vec(node, "control_points", &[], tag)? else {
        return Err(value_error(tag, key, "control_points is required."));
    };
    if vals.len() % 2 != 0 {
        return Err(value_error(
            tag,
            "control_points",
            "An even number of float values is required.",
        ));
    }
    let f = |v: f64| v as f32 as f64;
    let points: Vec<[f64; 2]> = vals.as_chunks::<2>().0.iter().map(|p| [f(p[0]), f(p[1])]).collect();
    let mut slopes = Vec::new();
    if let Some(s) = ocio_f64_vec(node, "slopes", &[], tag)? {
        if s.len() != points.len() {
            return Err(value_error(
                tag,
                "slopes",
                "Number of slopes must match number of control points.",
            ));
        }
        if s.iter().any(|&v| v as f32 != 0.0) {
            slopes = s.into_iter().map(f).collect();
        }
    }
    Ok(Some((points, slopes)))
}

/// A rule's `custom: {key: value, ...}` map (file and viewing rules, `OCIOYaml.cpp`).
pub(super) fn parse_custom_keys(item: &Yaml) -> Vec<(String, String)> {
    let mut keys = Vec::new();
    if let Some(Yaml::Mapping(map)) = yaml_get(item, "custom").map(unwrap_tagged) {
        for (k, v) in map.iter() {
            if let (Some(key), Some(val)) = (yaml_as_str(k), yaml_to_string(v)) {
                keys.push((key.to_string(), val));
            }
        }
    }
    keys
}
