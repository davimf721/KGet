//! Download tuning: speed limit and parallel connection count.
//!
//! The cache and compression helpers that used to live here were never wired
//! into any download path; since 1.7.1 they are deprecated no-ops kept only for
//! API compatibility.
//!
//! # Example
//!
//! ```rust
//! use kget::Optimizer;
//!
//! let optimizer = Optimizer::new();
//! assert_eq!(optimizer.max_connections(), 4);
//! ```

use crate::config::OptimizationConfig;
use std::error::Error;

/// Download tuning shared by all downloaders: speed limit and connection count.
///
/// # Example
///
/// ```rust
/// use kget::{Optimizer, Config};
///
/// // From config
/// let config = Config::default();
/// let optimizer = Optimizer::from_config(config.optimization);
///
/// // Or with defaults
/// let optimizer = Optimizer::new();
/// ```
#[derive(Clone, Debug)]
pub struct Optimizer {
    config: OptimizationConfig,
    /// Speed limit in bytes per second (None = unlimited)
    pub speed_limit: Option<u64>,
}

impl Optimizer {
    /// Create a new `Optimizer` with the provided configuration.
    pub fn from_config(config: OptimizationConfig) -> Self {
        let speed_limit = config.speed_limit.filter(|l| *l > 0);
        Self {
            config,
            speed_limit,
        }
    }

    /// Deprecated: compression was never applied to downloads. Returns `data` unchanged.
    #[deprecated(
        since = "1.7.1",
        note = "compression was never applied to downloads; this is a no-op"
    )]
    pub fn compress(&self, data: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
        Ok(data.to_vec())
    }

    /// Deprecated: returns `data` unchanged.
    #[deprecated(
        since = "1.7.1",
        note = "compression was never applied to downloads; this is a no-op"
    )]
    pub fn decompress(&self, data: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
        Ok(data.to_vec())
    }

    /// Deprecated: the download cache was never used. Always returns `Ok(None)`.
    #[deprecated(
        since = "1.7.1",
        note = "the download cache was never used; this is a no-op"
    )]
    pub fn get_cached_file(&self, _url: &str) -> Result<Option<Vec<u8>>, Box<dyn Error>> {
        Ok(None)
    }

    /// Deprecated: does nothing.
    #[deprecated(
        since = "1.7.1",
        note = "the download cache was never used; this is a no-op"
    )]
    pub fn cache_file(&self, _url: &str, _data: &[u8]) -> Result<(), Box<dyn Error>> {
        Ok(())
    }

    /// Get the peer connection limit for torrent downloads.
    pub fn get_peer_limit(&self) -> usize {
        50
    }

    /// Maximum parallel HTTP connections to use for advanced downloads.
    pub fn max_connections(&self) -> usize {
        self.config.max_connections.clamp(1, 32)
    }

    /// Deprecated: always `false` since 1.7.1.
    #[deprecated(since = "1.7.1", note = "compression is no longer supported")]
    pub fn is_compression_enabled(&self) -> bool {
        false
    }

    /// Create a new `Optimizer` with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Deprecated alias for `from_config`. Use `Optimizer::from_config()` instead.
    #[doc(hidden)]
    pub fn with_config(config: OptimizationConfig) -> Self {
        Self::from_config(config)
    }
}

impl Default for Optimizer {
    /// No speed limit, 4 connections.
    fn default() -> Self {
        Self::from_config(OptimizationConfig::default())
    }
}
