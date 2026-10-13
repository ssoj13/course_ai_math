//! Path utility helpers for OCIO parity.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use crate::context::Context;
use crate::io_proxy::ConfigIOProxy;

#[cfg(any(windows, not(any(unix, windows))))]
use std::collections::hash_map::DefaultHasher;
#[cfg(any(windows, not(any(unix, windows))))]
use std::hash::{Hash, Hasher};

/// Hash function type used by GetFastFileHash.
///
/// This mirrors OCIO's ComputeHashFunction.
pub type ComputeHashFunction = Box<dyn Fn(&str) -> String + Send + 'static>;

static HASH_FN: OnceLock<Mutex<Option<ComputeHashFunction>>> = OnceLock::new();
static FILE_HASH_CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

fn hash_fn_lock() -> &'static Mutex<Option<ComputeHashFunction>> {
    HASH_FN.get_or_init(|| Mutex::new(None))
}

fn cache_lock() -> &'static Mutex<HashMap<String, String>> {
    FILE_HASH_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Sets the compute hash function (OCIO parity).
pub fn set_compute_hash_function(func: ComputeHashFunction) {
    *hash_fn_lock().lock().unwrap() = Some(func);
}

/// Resets the compute hash function to the default (OCIO parity).
pub fn reset_compute_hash_function() {
    *hash_fn_lock().lock().unwrap() = None;
}

/// Clears the internal path caches (OCIO parity helper).
#[allow(dead_code)]
pub fn clear_path_caches() {
    cache_lock().lock().unwrap().clear();
}

/// Returns true if the file exists (OCIO FileExists parity).
///
/// Uses get_fast_file_hash: non-empty hash means file exists (proxy or filesystem).
pub fn file_exists(filename: &str, context: &Context) -> bool {
    !get_fast_file_hash(filename, context).is_empty()
}

/// File existence check with explicit proxy (for Config::resolve_file when Context not fully built).
/// OCIO parity: proxy first, then filesystem for absolute paths.
pub fn file_exists_with_proxy(filename: &str, proxy: Option<&dyn ConfigIOProxy>) -> bool {
    if let Some(p) = proxy {
        if let Some(hash) = p.get_fast_lut_file_hash(filename)
            && !hash.is_empty()
        {
            return true;
        }
        if !Path::new(filename).is_absolute() {
            return false;
        }
    }
    !default_file_hash(filename).is_empty()
}

/// Returns a fast hash for a file path, using ConfigIOProxy or the compute hash function.
pub fn get_fast_file_hash(filename: &str, context: &Context) -> String {
    {
        let cache = cache_lock().lock().unwrap();
        if let Some(value) = cache.get(filename) {
            return value.clone();
        }
    }

    let hash = compute_fast_hash(filename, context);

    let mut cache = cache_lock().lock().unwrap();
    cache.insert(filename.to_string(), hash.clone());

    hash
}

fn compute_fast_hash(filename: &str, context: &Context) -> String {
    if let Some(proxy) = context.config_io_proxy() {
        if let Some(hash) = proxy.get_fast_lut_file_hash(filename)
            && !hash.is_empty()
        {
            return hash;
        }

        if std::path::Path::new(filename).is_absolute() {
            return compute_hash(filename);
        }

        return String::new();
    }

    compute_hash(filename)
}

fn compute_hash(filename: &str) -> String {
    if let Some(ref func) = *hash_fn_lock().lock().unwrap() {
        return func(filename);
    }

    default_file_hash(filename)
}

fn default_file_hash(filename: &str) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Ok(meta) = std::fs::metadata(filename) {
            return format!("{}:{}", meta.dev(), meta.ino());
        }
        return String::new();
    }

    #[cfg(windows)]
    {
        if std::fs::metadata(filename).is_ok() {
            let mut hasher = DefaultHasher::new();
            filename.hash(&mut hasher);
            return format!("0:{}", hasher.finish());
        }
        String::new()
    }

    #[cfg(not(any(unix, windows)))]
    {
        if std::fs::metadata(filename).is_ok() {
            let mut hasher = DefaultHasher::new();
            filename.hash(&mut hasher);
            return format!("0:{}", hasher.finish());
        }
        String::new()
    }
}
