//! Thread-safe processor caching.
//!
//! Cache compiled processors to avoid re-compilation on repeated access.
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use vfx_ocio::{Config, ProcessorCache, builtin};
//!
//! let config = builtin::default_config();
//! let cache = ProcessorCache::new();
//!
//! // First call compiles the processor
//! let proc1 = cache.get_or_create(&config, "ACEScg", "sRGB - Display")?;
//!
//! // Second call returns cached version (fast)
//! let proc2 = cache.get_or_create(&config, "ACEScg", "sRGB - Display")?;
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::sync::RwLock;

use crate::config::Config;
use crate::error::OcioResult;
use crate::processor::{OptimizationLevel, Processor, ProcessorOp};

/// Cache key for processor lookup.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct CacheKey {
    src: String,
    dst: String,
    looks: String,
}

/// Cached processor data (ops + metadata).
#[derive(Debug, Clone)]
struct CachedOps {
    ops: Vec<ProcessorOp>,
}

/// Thread-safe processor cache.
///
/// Caches compiled processor operations by (src, dst, looks) tuple.
/// Use when creating many processors for the same conversions.
#[derive(Debug, Default)]
pub struct ProcessorCache {
    cache: RwLock<HashMap<CacheKey, CachedOps>>,
}

impl ProcessorCache {
    /// Create empty cache.
    pub fn new() -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// Get or create processor for src->dst conversion.
    ///
    /// Returns cached processor if available, otherwise creates and caches.
    pub fn get_or_create(&self, config: &Config, src: &str, dst: &str) -> OcioResult<Processor> {
        self.get_or_create_with_looks(config, src, dst, "")
    }

    /// Get or create processor with looks.
    pub fn get_or_create_with_looks(
        &self,
        config: &Config,
        src: &str,
        dst: &str,
        looks: &str,
    ) -> OcioResult<Processor> {
        let key = CacheKey {
            src: src.to_string(),
            dst: dst.to_string(),
            looks: looks.to_string(),
        };

        // Try read lock first (fast path)
        {
            let cache = self.cache.read().unwrap();
            if let Some(cached) = cache.get(&key) {
                return Ok(Processor::from_ops(cached.ops.clone()));
            }
        }

        // Cache miss - create processor
        let processor = if looks.is_empty() {
            config.processor(src, dst)?
        } else {
            config.processor_with_looks(src, dst, looks)?
        };

        // Cache the ops
        {
            let mut cache = self.cache.write().unwrap();
            cache.insert(
                key,
                CachedOps {
                    ops: processor.ops().to_vec(),
                },
            );
        }

        Ok(processor)
    }

    /// Get or create optimized processor.
    pub fn get_or_create_optimized(
        &self,
        config: &Config,
        src: &str,
        dst: &str,
        optimization: OptimizationLevel,
    ) -> OcioResult<Processor> {
        // For optimized processors, include opt level in cache key
        let key = CacheKey {
            src: src.to_string(),
            dst: format!("{}@{:?}", dst, optimization),
            looks: String::new(),
        };

        {
            let cache = self.cache.read().unwrap();
            if let Some(cached) = cache.get(&key) {
                return Ok(Processor::from_ops(cached.ops.clone()));
            }
        }

        let processor = config.processor_with_opts(src, dst, optimization)?;

        {
            let mut cache = self.cache.write().unwrap();
            cache.insert(
                key,
                CachedOps {
                    ops: processor.ops().to_vec(),
                },
            );
        }

        Ok(processor)
    }

    /// Clear all cached processors.
    pub fn clear(&self) {
        let mut cache = self.cache.write().unwrap();
        cache.clear();
    }

    /// Number of cached processors.
    pub fn len(&self) -> usize {
        let cache = self.cache.read().unwrap();
        cache.len()
    }

    /// Check if cache is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::{self, TestSchema};
    use std::sync::Arc;

    #[test]
    fn cache_hit() {
        let config = builtin::aces_2_0_studio_all_views();
        let cache = ProcessorCache::new();

        assert!(cache.is_empty());

        let _proc1 = cache
            .get_or_create(
                &config,
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        assert_eq!(cache.len(), 1);

        let _proc2 = cache
            .get_or_create(
                &config,
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        assert_eq!(cache.len(), 1);

        let _proc3 = cache
            .get_or_create(
                &config,
                &TestSchema::shared().interchange,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn cache_clear() {
        let config = builtin::aces_2_0_studio_all_views();
        let cache = ProcessorCache::new();

        cache
            .get_or_create(
                &config,
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();
        assert_eq!(cache.len(), 1);

        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn thread_safe() {
        use std::thread;

        let config = Arc::new(builtin::aces_2_0_studio_all_views());
        let cache = Arc::new(ProcessorCache::new());

        let handles: Vec<_> = (0..4)
            .map(|_| {
                let config = Arc::clone(&config);
                let cache = Arc::clone(&cache);
                thread::spawn(move || {
                    for _ in 0..10 {
                        cache
                            .get_or_create(
                                &config,
                                &TestSchema::shared().working_space,
                                &TestSchema::shared().display_encoded_srgb,
                            )
                            .unwrap();
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(cache.len(), 1);
    }
}
