//! Config IO proxy support (OCIO parity).
//!
//! Allows redirecting LUT/config file reads to custom storage.

use std::fmt::Debug;

/// Config IO proxy for resolving LUT and config data.
///
/// OCIO ref: ConfigIOProxy in OpenColorIO.
pub trait ConfigIOProxy: Send + Sync + Debug {
    /// Returns LUT file bytes for the given path.
    fn get_lut_data(&self, path: &str) -> Option<Vec<u8>>;

    /// Returns config file bytes for the given path.
    fn get_config_data(&self, path: &str) -> Option<Vec<u8>>;

    /// Returns fast LUT file hash for cache lookups.
    fn get_fast_lut_file_hash(&self, path: &str) -> Option<String>;
}
