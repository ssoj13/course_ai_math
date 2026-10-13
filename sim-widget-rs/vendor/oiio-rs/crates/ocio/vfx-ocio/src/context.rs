//! Context variables for dynamic path resolution.
//!
//! OCIO configs can use context variables like `$SHOT`, `$SEQ` in file paths.
//! This module handles variable substitution from environment or explicit values.
//!
//! # Example
//!
//! ```
//! use vfx_ocio::Context;
//!
//! let mut ctx = Context::new();
//! ctx.set("SHOT", "sh010");
//! ctx.set("SEQ", "sq01");
//!
//! let resolved = ctx.resolve("/shows/$SEQ/shots/$SHOT/luts/grade.csp");
//! assert_eq!(resolved, "/shows/sq01/shots/sh010/luts/grade.csp");
//! ```

use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::EnvironmentMode;
use crate::io_proxy::ConfigIOProxy;

/// Context for variable substitution in file paths.
#[derive(Debug, Clone, Default)]
pub struct Context {
    /// User-defined variables (take precedence over environment).
    vars: HashMap<String, String>,
    /// Whether to check environment variables as fallback.
    use_env: bool,
    /// Search paths for resolving file locations.
    search_paths: Vec<String>,
    /// Working directory for relative paths.
    working_dir: String,
    /// Environment loading mode.
    environment_mode: EnvironmentMode,
    /// Config IO proxy.
    io_proxy: Option<Arc<dyn ConfigIOProxy>>,
}

impl Context {
    /// Creates a new empty context.
    ///
    /// By default, environment variables are used as fallback.
    #[inline]
    pub fn new() -> Self {
        Self {
            vars: HashMap::new(),
            use_env: true,
            search_paths: Vec::new(),
            working_dir: ".".to_string(),
            environment_mode: EnvironmentMode::LoadPredefined,
            io_proxy: None,
        }
    }

    /// Creates a context that ignores environment variables.
    #[inline]
    pub fn without_env() -> Self {
        Self {
            vars: HashMap::new(),
            use_env: false,
            search_paths: Vec::new(),
            working_dir: ".".to_string(),
            environment_mode: EnvironmentMode::LoadPredefined,
            io_proxy: None,
        }
    }

    /// Creates an editable copy (OCIO parity).
    #[inline]
    pub fn create_editable_copy(&self) -> Self {
        self.clone()
    }

    /// Returns the context cache ID.
    pub fn cache_id(&self) -> String {
        use xxhash_rust::xxh3::xxh3_128;

        let mut cache = String::new();
        if !self.search_paths.is_empty() {
            cache.push_str("Search Path ");
            for path in &self.search_paths {
                cache.push_str(path);
                cache.push(' ');
            }
        }
        cache.push_str("Working Dir ");
        cache.push_str(&self.working_dir);
        cache.push(' ');
        cache.push_str("Environment Mode ");
        cache.push_str(match self.environment_mode {
            EnvironmentMode::LoadPredefined => "0",
            EnvironmentMode::LoadAll => "1",
        });
        cache.push(' ');

        let mut keys: Vec<&String> = self.vars.keys().collect();
        keys.sort();
        for key in keys {
            if let Some(value) = self.vars.get(key) {
                cache.push_str(key);
                cache.push('=');
                cache.push_str(value);
                cache.push(' ');
            }
        }

        let hash = xxh3_128(cache.as_bytes());
        let low = hash as u64;
        let high = (hash >> 64) as u64;
        format!("{:x}{:x}", low, high)
    }

    /// Sets a context variable.
    ///
    /// This takes precedence over environment variables with the same name.
    #[inline]
    pub fn set(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.vars.insert(name.into(), value.into());
    }

    /// Sets multiple context variables at once.
    #[inline]
    pub fn set_vars<I, K, V>(&mut self, vars: I)
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        for (k, v) in vars {
            self.set(k, v);
        }
    }

    /// Adds all string vars from another context.
    pub fn add_string_vars(&mut self, other: &Context) {
        for (k, v) in other.vars() {
            self.set(k, v);
        }
    }

    /// Gets a context variable value.
    ///
    /// Checks user-defined variables first, then environment if enabled.
    pub fn get(&self, name: &str) -> Option<String> {
        // Check user-defined first
        if let Some(v) = self.vars.get(name) {
            return Some(v.clone());
        }
        // Fallback to environment
        if self.use_env {
            env::var(name).ok()
        } else {
            None
        }
    }

    /// Gets a context variable as &str (only user-defined vars).
    #[inline]
    pub fn get_string_var(&self, name: &str) -> Option<&str> {
        self.vars.get(name).map(|s| s.as_str())
    }

    /// Returns the number of string vars.
    #[inline]
    pub fn num_string_vars(&self) -> usize {
        self.vars.len()
    }

    /// Returns string var name by index (sorted).
    pub fn string_var_name_by_index(&self, index: usize) -> Option<&str> {
        let mut keys: Vec<&String> = self.vars.keys().collect();
        keys.sort();
        keys.get(index).map(|k| k.as_str())
    }

    /// Returns string var value by index (sorted by key).
    pub fn string_var_by_index(&self, index: usize) -> Option<&str> {
        let mut keys: Vec<&String> = self.vars.keys().collect();
        keys.sort();
        keys.get(index)
            .and_then(|k| self.vars.get(*k))
            .map(|v| v.as_str())
    }

    /// Checks if a variable is defined.
    #[inline]
    pub fn contains(&self, name: &str) -> bool {
        self.vars.contains_key(name) || (self.use_env && env::var(name).is_ok())
    }

    /// Removes a context variable.
    #[inline]
    pub fn remove_var(&mut self, name: &str) {
        self.vars.remove(name);
    }

    /// Sets whether environment variables are used as fallback.
    #[inline]
    pub fn set_use_env(&mut self, use_env: bool) {
        self.use_env = use_env;
    }

    /// Returns whether environment variables are used as fallback.
    #[inline]
    pub fn use_env(&self) -> bool {
        self.use_env
    }

    /// Sets the environment mode.
    #[inline]
    pub fn set_environment_mode(&mut self, mode: EnvironmentMode) {
        self.environment_mode = mode;
    }

    /// Returns the environment mode.
    #[inline]
    pub fn environment_mode(&self) -> EnvironmentMode {
        self.environment_mode
    }

    /// Loads environment variables based on the current mode.
    pub fn load_environment(&mut self) {
        match self.environment_mode {
            EnvironmentMode::LoadAll => {
                for (k, v) in env::vars() {
                    self.vars.insert(k, v);
                }
            }
            EnvironmentMode::LoadPredefined => {
                let keys: Vec<String> = self.vars.keys().cloned().collect();
                for key in keys {
                    if let Ok(value) = env::var(&key) {
                        self.vars.insert(key, value);
                    }
                }
            }
        }
    }

    /// Sets the search path list from a string.
    pub fn set_search_path(&mut self, path: impl Into<String>) {
        self.search_paths = split_search_path_string(&path.into())
            .into_iter()
            .map(|s| s.to_string())
            .collect();
    }

    /// Returns the search path list joined by `:`.
    pub fn search_path(&self) -> String {
        self.search_paths.join(":")
    }

    /// Returns the number of search paths.
    pub fn num_search_paths(&self) -> usize {
        self.search_paths.len()
    }

    /// Returns search path by index.
    pub fn search_path_at(&self, index: usize) -> Option<&str> {
        self.search_paths.get(index).map(|s| s.as_str())
    }

    /// Clears all search paths.
    pub fn clear_search_paths(&mut self) {
        self.search_paths.clear();
    }

    /// Adds a search path.
    pub fn add_search_path(&mut self, path: impl Into<String>) {
        self.search_paths.push(path.into());
    }

    /// Sets the working directory.
    pub fn set_working_dir(&mut self, dir: impl Into<String>) {
        self.working_dir = dir.into();
    }

    /// Returns the working directory.
    pub fn working_dir(&self) -> &str {
        &self.working_dir
    }

    /// Sets the config IO proxy.
    pub fn set_config_io_proxy(&mut self, proxy: Option<Arc<dyn ConfigIOProxy>>) {
        self.io_proxy = proxy;
    }

    /// Returns the config IO proxy.
    pub fn config_io_proxy(&self) -> Option<Arc<dyn ConfigIOProxy>> {
        self.io_proxy.clone()
    }

    /// Resolves all `$VAR`, `${VAR}`, and `%VAR%` references in a string.
    ///
    /// Unknown variables are left as-is (not substituted).
    pub fn resolve(&self, input: &str) -> String {
        let mut used = Context::without_env();
        self.resolve_recursive(input, &mut used)
    }

    /// Alias for `resolve()`.
    #[inline]
    pub fn resolve_string_var(&self, input: &str) -> String {
        self.resolve(input)
    }

    /// Resolves context variables and returns the used context variables.
    pub fn resolve_string_var_with_used_context(&self, input: &str) -> (String, Context) {
        let mut used = Context::without_env();
        let result = self.resolve_recursive(input, &mut used);
        (result, used)
    }

    /// One substitution pass over `input`, matching OCIO's own
    /// `ResolveContextVariablesImpl` (`ContextVariableUtils.cpp:127-175`)
    /// algorithm exactly rather than the identifier-boundary token scan this
    /// used to be: literal substring replacement of `${key}`, `$key`, and
    /// `%key%` for each key in [`Self::candidate_map`], tried LONGEST KEY
    /// FIRST (`EnvMapKey`'s ordering, `ContextVariableUtils.h:25-44` — "so
    /// that recursive string expansion will deal with similar prefixed keys
    /// as expected", e.g. `$TESTING` must match the `TESTING` entry, not
    /// get cannibalized by a shorter `TEST` entry matching its prefix
    /// first).
    ///
    /// This is deliberately NOT identifier-boundary-aware — same as the
    /// reference. `$STUDIO_$PROJECT` with only `STUDIO` and `PROJECT`
    /// declared resolves correctly (`$STUDIO` is found as a literal
    /// substring regardless of the `_` that follows it), which the old
    /// token-scanner could not do (it greedily consumed `_` into the
    /// candidate name, same failure mode as shell `$VAR` expansion).
    ///
    /// Records every variable actually substituted into `used`. Returns
    /// `(result, changed)` — `changed` is `false` only when nothing in
    /// `input` could be resolved (a fixed point).
    fn resolve_once(&self, input: &str, used: &mut Context) -> (String, bool) {
        let map = self.candidate_map(input);
        let mut s = input.to_string();
        let mut changed = false;

        for (key, value) in &map {
            let mut hit = false;
            if replace_all_in_place(&mut s, &format!("${{{key}}}"), value) {
                hit = true;
            }
            if replace_all_in_place(&mut s, &format!("${key}"), value) {
                hit = true;
            }
            if replace_all_in_place(&mut s, &format!("%{key}%"), value) {
                hit = true;
            }
            if hit {
                used.set(key.clone(), value.clone());
                changed = true;
            }
        }

        (s, changed)
    }

    /// Builds the key→value map [`Self::resolve_once`] substitutes from,
    /// ordered longest-key-first (ties broken lexicographically, matching
    /// `EnvMapKey`). ALL declared context variables ([`Self::vars`]) are
    /// always included — bounded, cheap, matches OCIO's `m_envMap` for the
    /// common (config-authored) case exactly. Environment variables are
    /// NOT pre-loaded wholesale (OCIO's `LoadEnvironment` does load the
    /// entire OS environment into `m_envMap` up front, but doing that here
    /// would mean re-scanning every OS env var on every resolve call for no
    /// benefit — the reference only pays that cost once, at `Context`
    /// construction). Instead, `input` is scanned once for candidate
    /// `$NAME`/`${NAME}`/`%NAME%` tokens (identifier-boundary, same shape
    /// as the old resolver) and each candidate not already covered by a
    /// declared var is looked up via [`Self::get`] (context override, then
    /// `std::env::var`) — same net behavior (a real env var referenced by
    /// name resolves) without the wholesale up-front cost.
    fn candidate_map(&self, input: &str) -> Vec<(String, String)> {
        let mut map: HashMap<String, String> = self.vars.clone();

        for name in scan_candidate_var_names(input) {
            if !map.contains_key(&name)
                && self.use_env
                && let Ok(value) = env::var(&name)
            {
                map.insert(name, value);
            }
        }

        let mut entries: Vec<(String, String)> = map.into_iter().collect();
        entries.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
        entries
    }

    /// Repeats [`Self::resolve_once`] until the string stops changing.
    ///
    /// OCIO's `ResolveContextVariablesImpl` (`ContextVariableUtils.cpp:127-175`)
    /// re-expands the WHOLE string after every substitution round, so a
    /// context variable whose own value references another variable (e.g.
    /// `$SHOW` = `"$STUDIO_$PROJECT"`) resolves fully — not left with the
    /// inner `$STUDIO`/`$PROJECT` tokens verbatim in the output. A single
    /// [`Self::resolve_once`] pass alone cannot do this: it never re-scans
    /// text it just substituted in. Guarded at depth 32, matching the
    /// reference's own recursion cap (protects against `$A` = `"$B"`,
    /// `$B` = `"$A"` cycles).
    fn resolve_recursive(&self, input: &str, used: &mut Context) -> String {
        let mut current = input.to_string();
        for _ in 0..32 {
            let (next, changed) = self.resolve_once(&current, used);
            if !changed || next == current {
                return next;
            }
            current = next;
        }
        current
    }

    /// Resolves a file location using search paths and working directory.
    ///
    /// OCIO parity: GetAbsoluteSearchPaths order, FileExists (proxy + filesystem).
    pub fn resolve_file_location(&self, filename: &str) -> Option<PathBuf> {
        use crate::path_utils;

        let resolved = self.resolve(filename);
        let path = Path::new(&resolved);

        // Absolute path: check existence (proxy or filesystem) independent of search paths
        if path.is_absolute() {
            if path_utils::file_exists(&resolved, self) {
                return Some(norm_path(PathBuf::from(&resolved)));
            }
            return None;
        }

        // Relative path: build search path list per OCIO GetAbsoluteSearchPaths
        let search_paths = get_absolute_search_paths(self);
        for base in &search_paths {
            let candidate = base.join(&resolved);
            let candidate_str = candidate.to_string_lossy();
            if path_utils::file_exists(&candidate_str, self) {
                return Some(norm_path(candidate));
            }
        }

        None
    }

    /// Resolves a file location and returns used context variables.
    pub fn resolve_file_location_with_used_context(
        &self,
        filename: &str,
    ) -> (Option<PathBuf>, Context) {
        let (resolved, used) = self.resolve_string_var_with_used_context(filename);
        let resolved_path = self.resolve_file_location(&resolved);
        (resolved_path, used)
    }

    /// Returns all user-defined variables as a HashMap reference.
    #[inline]
    pub fn vars_map(&self) -> &HashMap<String, String> {
        &self.vars
    }

    /// Returns an iterator over all user-defined variables.
    #[inline]
    pub fn vars(&self) -> impl Iterator<Item = (&str, &str)> {
        self.vars.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Clears all user-defined variables.
    #[inline]
    pub fn clear(&mut self) {
        self.vars.clear();
    }

    /// Alias for `set` - sets a context variable.
    #[inline]
    pub fn set_var(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.set(name, value);
    }

    /// Gets a variable value as a string reference.
    ///
    /// Only checks user-defined variables (not environment).
    #[inline]
    pub fn get_var(&self, name: &str) -> Option<&str> {
        self.vars.get(name).map(|s| s.as_str())
    }

    /// Resolves variables strictly, returning error if any are unresolved.
    ///
    /// Unlike `resolve()`, this method returns an error if any variables
    /// in the input cannot be resolved.
    ///
    /// # Errors
    /// Returns a list of unresolved variable names if any cannot be resolved.
    pub fn resolve_strict(&self, input: &str) -> Result<String, Vec<String>> {
        let unresolved = self.unresolved_vars(input);
        if unresolved.is_empty() {
            Ok(self.resolve(input))
        } else {
            Err(unresolved)
        }
    }

    /// Checks if a string contains unresolved context variables.
    ///
    /// Returns `true` if any `$VAR` or `${VAR}` cannot be resolved.
    pub fn has_unresolved(&self, input: &str) -> bool {
        !self.unresolved_vars(input).is_empty()
    }

    /// Returns list of unresolved variable names in a string.
    ///
    /// Scans for `$VAR` and `${VAR}` patterns and returns names
    /// that cannot be resolved from context or environment.
    pub fn unresolved_vars(&self, input: &str) -> Vec<String> {
        let mut unresolved = Vec::new();
        let mut chars = input.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '$' {
                if chars.peek() == Some(&'{') {
                    chars.next(); // consume '{'
                    let var_name: String = chars.by_ref().take_while(|&c| c != '}').collect();
                    if !var_name.is_empty() && self.get(&var_name).is_none() {
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
                    if !var_name.is_empty() && self.get(&var_name).is_none() {
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
                if found_end && valid && !var_name.is_empty() && self.get(&var_name).is_none() {
                    unresolved.push(var_name);
                }
            }
        }

        unresolved
    }

    /// Returns the number of user-defined variables.
    #[inline]
    pub fn len(&self) -> usize {
        self.vars.len()
    }

    /// Checks if no variables are defined.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.vars.is_empty()
    }
}

/// Scans `input` for `$NAME`, `${NAME}`, and `%NAME%` token bodies
/// (identifier-boundary: alnum + `_`), without checking whether any of
/// them resolve to anything. Used only to build a bounded candidate list
/// for environment-variable lookups in [`Context::candidate_map`] — the
/// actual substitution in [`Context::resolve_once`] is literal substring
/// replacement over the map this feeds, not driven by this scan directly
/// (a declared context variable substring-matches even if this scan would
/// have drawn its token boundary differently, matching OCIO).
fn scan_candidate_var_names(input: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '$' {
            if chars.peek() == Some(&'{') {
                chars.next();
                let name: String = chars.by_ref().take_while(|&c| c != '}').collect();
                if !name.is_empty() {
                    names.push(name);
                }
            } else {
                let mut name = String::new();
                while let Some(&ch) = chars.peek() {
                    if ch.is_alphanumeric() || ch == '_' {
                        name.push(ch);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if !name.is_empty() {
                    names.push(name);
                }
            }
        } else if c == '%' {
            let name: String = chars.by_ref().take_while(|&c| c != '%').collect();
            if name.chars().all(|c| c.is_alphanumeric() || c == '_') && !name.is_empty() {
                names.push(name);
            }
        }
    }

    names
}

/// Replaces every non-overlapping occurrence of `pattern` in `s` with
/// `replacement`, in place. Returns whether anything was replaced. Mirrors
/// OCIO's `StringUtils::ReplaceInPlace` (a plain substring search-and-
/// replace, no identifier-boundary awareness — see
/// [`Context::resolve_once`]'s doc comment for why that's intentional).
fn replace_all_in_place(s: &mut String, pattern: &str, replacement: &str) -> bool {
    if pattern.is_empty() || !s.contains(pattern) {
        return false;
    }
    *s = s.replace(pattern, replacement);
    true
}

/// OCIO GetAbsoluteSearchPaths: when search_paths empty -> [working_dir];
/// else for each path resolve vars, if relative join with working_dir.
fn get_absolute_search_paths(context: &Context) -> Vec<PathBuf> {
    let working_dir = Path::new(&context.working_dir);

    if context.search_paths.is_empty() {
        return vec![working_dir.to_path_buf()];
    }

    context
        .search_paths
        .iter()
        .map(|p| {
            let resolved = context.resolve(p);
            let trimmed = resolved.trim().trim_end_matches('/');
            if trimmed.is_empty() {
                working_dir.to_path_buf()
            } else {
                let path = Path::new(trimmed);
                if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    working_dir.join(path)
                }
            }
        })
        .map(norm_path)
        .collect()
}

/// Normalizes path (OCIO normpath parity): removes . and resolves ..
fn norm_path(path: PathBuf) -> PathBuf {
    use std::path::Component;

    let mut components = Vec::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(components.last(), Some(Component::Normal(_))) {
                    components.pop();
                } else {
                    components.push(comp);
                }
            }
            c => components.push(c),
        }
    }
    components.into_iter().collect()
}

fn split_search_path_string(raw: &str) -> Vec<&str> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_simple_var() {
        let mut ctx = Context::without_env();
        ctx.set("SHOT", "sh010");

        assert_eq!(ctx.resolve("/path/$SHOT/file"), "/path/sh010/file");
    }

    #[test]
    fn resolve_braced_var() {
        let mut ctx = Context::without_env();
        ctx.set("SEQ", "sq01");

        assert_eq!(ctx.resolve("/path/${SEQ}_data"), "/path/sq01_data");
    }

    #[test]
    fn resolve_multiple_vars() {
        let mut ctx = Context::without_env();
        ctx.set("A", "alpha");
        ctx.set("B", "beta");

        assert_eq!(ctx.resolve("$A-${B}-$A"), "alpha-beta-alpha");
    }

    #[test]
    fn unresolved_left_as_is() {
        let ctx = Context::without_env();
        assert_eq!(ctx.resolve("$UNKNOWN"), "$UNKNOWN");
        assert_eq!(ctx.resolve("${UNKNOWN}"), "${UNKNOWN}");
    }

    #[test]
    fn dollar_at_end() {
        let ctx = Context::without_env();
        assert_eq!(ctx.resolve("test$"), "test$");
    }

    #[test]
    fn unresolved_vars_detection() {
        let mut ctx = Context::without_env();
        ctx.set("SHOT", "sh010");

        // No unresolved
        assert!(!ctx.has_unresolved("/path/$SHOT/file"));
        assert!(ctx.unresolved_vars("/path/$SHOT/file").is_empty());

        // Has unresolved
        assert!(ctx.has_unresolved("/path/$SHOT/$SEQ/file"));
        assert_eq!(ctx.unresolved_vars("/path/$SHOT/$SEQ/file"), vec!["SEQ"]);

        // Multiple unresolved
        let unresolved = ctx.unresolved_vars("$A/${B}/$SHOT");
        assert_eq!(unresolved, vec!["A", "B"]);
    }

    #[test]
    fn resolve_strict_success() {
        let mut ctx = Context::without_env();
        ctx.set("SHOT", "sh010");
        ctx.set("SEQ", "sq01");

        let result = ctx.resolve_strict("/path/$SEQ/$SHOT");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "/path/sq01/sh010");
    }

    #[test]
    fn resolve_strict_error() {
        let mut ctx = Context::without_env();
        ctx.set("SHOT", "sh010");

        let result = ctx.resolve_strict("/path/$SEQ/$SHOT");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), vec!["SEQ"]);
    }

    #[test]
    fn resolve_percent_var() {
        let mut ctx = Context::without_env();
        ctx.set("SHOW", "showA");

        assert_eq!(ctx.resolve("/%SHOW%/shots"), "/showA/shots");
        assert_eq!(ctx.resolve("%MISSING%"), "%MISSING%");
    }

    /// Matches OCIO's literal-substring substitution semantics exactly
    /// (`ResolveContextVariablesImpl`, `StringUtils::ReplaceInPlace`) rather
    /// than the identifier-boundary token scan this used to be: `$STUDIO`
    /// is found and replaced as a literal substring of `$STUDIO_$PROJECT`
    /// EVEN WITHOUT a disambiguating brace, because the reference doesn't
    /// scan for identifier boundaries at all — it just searches for the
    /// literal text `"$" + key`. The old scanner greedily consumed the
    /// trailing `_` into an undefined "STUDIO_" name and left the whole
    /// token un-substituted; this must not regress.
    #[test]
    fn resolve_matches_variable_as_a_literal_substring_without_braces() {
        let mut ctx = Context::without_env();
        ctx.set("STUDIO", "acme");
        ctx.set("PROJECT", "widget");

        assert_eq!(
            ctx.resolve("/shows/$STUDIO_$PROJECT/data"),
            "/shows/acme_widget/data"
        );
    }

    /// The longest-key-first ordering (`EnvMapKey`) exists specifically so
    /// a longer variable isn't cannibalized by a shorter one that happens
    /// to be a prefix of it. With `TE` and `TESTING` both declared,
    /// `$TESTING` must resolve via the `TESTING` entry, not have its first
    /// two characters eaten by `TE` first (which would leave a mangled
    /// `<TE-value>STING` in the output).
    #[test]
    fn resolve_prefers_the_longer_key_when_one_is_a_prefix_of_another() {
        let mut ctx = Context::without_env();
        ctx.set("TE", "SHORT");
        ctx.set("TESTING", "LONG");

        assert_eq!(ctx.resolve("$TESTING"), "LONG");
    }

    /// A declared context variable is found even when it isn't `$`/`${`/`%`
    /// -adjacent in a way the old identifier-boundary scanner would have
    /// tokenized — proving substitution now goes through the declared-var
    /// map directly (`Context::candidate_map` always includes ALL of
    /// `self.vars`), not only through variables the scan happens to guess
    /// as candidates.
    #[test]
    fn resolve_finds_every_declared_var_even_with_unusual_adjacent_text() {
        let mut ctx = Context::without_env();
        ctx.set("A", "1");
        ctx.set("B", "2");

        assert_eq!(ctx.resolve("$A$B$A"), "121");
    }

    /// OCIO's `ResolveContextVariablesImpl` (`ContextVariableUtils.cpp:127-175`)
    /// re-expands the whole string after every substitution round — a
    /// context variable whose value itself references another variable
    /// resolves all the way through, not just one level. A single-pass
    /// scan (what `resolve()` used to be, before `resolve_recursive`) would
    /// leave the inner `$STUDIO`/`$PROJECT` tokens un-substituted in the
    /// output — this pins the fix.
    #[test]
    fn resolve_recursively_expands_nested_variable_values() {
        let mut ctx = Context::without_env();
        ctx.set("STUDIO", "acme");
        ctx.set("PROJECT", "widget");
        // Braces disambiguate the boundary between `$STUDIO` and the `_`
        // that follows — `$VAR` (no braces) greedily consumes identifier
        // characters including `_`, same as shell `$VAR` expansion, so
        // `$STUDIO_$PROJECT` would read "STUDIO_" as one (undefined) name.
        ctx.set("SHOW", "${STUDIO}_${PROJECT}");

        assert_eq!(ctx.resolve("/shows/$SHOW/data"), "/shows/acme_widget/data");
    }

    /// A three-level chain (`$A` -> `$B` -> `$C` -> literal) must also fully
    /// resolve, proving this isn't just a one-extra-pass special case.
    #[test]
    fn resolve_recursively_expands_a_three_level_chain() {
        let mut ctx = Context::without_env();
        ctx.set("C", "leaf");
        ctx.set("B", "$C");
        ctx.set("A", "$B");

        assert_eq!(ctx.resolve("$A"), "leaf");
    }

    /// A cyclic definition (`$A` = `"$B"`, `$B` = `"$A"`) must terminate
    /// (via the depth-32 guard, matching OCIO's own recursion cap) rather
    /// than loop forever or panic.
    #[test]
    fn resolve_recursive_terminates_on_a_cycle() {
        let mut ctx = Context::without_env();
        ctx.set("A", "$B");
        ctx.set("B", "$A");

        // Must return promptly with SOME string, not hang or panic.
        let result = ctx.resolve("$A");
        assert!(result == "$A" || result == "$B");
    }

    /// `resolve_string_var_with_used_context` must record every variable
    /// consumed across ALL recursion rounds, not just the first pass — the
    /// recorded value for each is its own RAW (unexpanded) definition, same
    /// as OCIO's `resolveStringVar` (`used[entry.first] = entry.second`
    /// where `entry.second` is the env-map entry, not further resolved).
    #[test]
    fn resolve_with_used_context_records_vars_from_nested_expansion() {
        let mut ctx = Context::without_env();
        ctx.set("STUDIO", "acme");
        ctx.set("SHOW", "${STUDIO}");

        let (resolved, used) = ctx.resolve_string_var_with_used_context("$SHOW");
        assert_eq!(resolved, "acme");
        assert_eq!(used.get_var("SHOW"), Some("${STUDIO}"));
        assert_eq!(used.get_var("STUDIO"), Some("acme"));
    }
}
