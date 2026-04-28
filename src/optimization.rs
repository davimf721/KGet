//! Download optimization through compression and caching.
//!
//! This module provides the [`Optimizer`] struct for configuring and applying
//! optimizations to downloads:
//!
//! - **Compression**: Automatic compression/decompression using Gzip, LZ4, or Brotli
//! - **Caching**: Store downloaded files locally to avoid redundant downloads
//! - **Speed limiting**: Control bandwidth usage
//!
//! # Example
//!
//! ```rust
//! use kget::Optimizer;
//!
//! // Create with default settings
//! let optimizer = Optimizer::new();
//!
//! // Check if compression is enabled
//! if optimizer.is_compression_enabled() {
//!     println!("Compression active");
//! }
//! ```

use crate::config::OptimizationConfig;
use flate2::write::{GzDecoder, GzEncoder};
use lz4::block::{CompressionMode, compress};
use std::error::Error;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;

/// Download optimizer for compression, caching, and speed limiting.
///
/// The `Optimizer` manages download performance features:
///
/// - **Compression**: Reduces storage size using configurable algorithms
/// - **Caching**: Stores files locally to avoid re-downloading
/// - **Speed limits**: Controls maximum download speed
///
/// # Compression Levels
///
/// | Level | Algorithm | Speed    | Ratio    |
/// |-------|-----------|----------|----------|
/// | 1-3   | Gzip      | Fast     | Moderate |
/// | 4-6   | LZ4       | Balanced | Good     |
/// | 7-9   | Brotli    | Slow     | Best     |
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
#[derive(Clone)]
pub struct Optimizer {
    config: OptimizationConfig,
    /// Speed limit in bytes per second (None = unlimited)
    pub speed_limit: Option<u64>,
}

impl Optimizer {
    /// Create a new `Optimizer` with the provided configuration.
    ///
    /// # Arguments
    ///
    /// * `config` - Optimization configuration settings
    ///
    /// # Example
    ///
    /// ```rust
    /// use kget::{Optimizer, Config};
    ///
    /// let config = Config::default();
    /// let optimizer = Optimizer::from_config(config.optimization);
    /// ```
    pub fn from_config(config: OptimizationConfig) -> Self {
        let speed_limit = config.speed_limit;
        Self {
            config,
            speed_limit,
        }
    }

    /// Compress data using the configured algorithm.
    ///
    /// The algorithm is selected based on `compression_level`:
    /// - Levels 1-3: Gzip (fast)
    /// - Levels 4-6: LZ4 (balanced)
    /// - Levels 7-9: Brotli (high compression)
    ///
    /// Returns the original data unchanged if compression is disabled.
    pub fn compress(&self, data: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
        if !self.config.compression {
            return Ok(data.to_vec());
        }
        let compressed = match self.config.compression_level {
            1..=3 => {
                let mut encoder = GzEncoder::new(Vec::new(), flate2::Compression::fast());
                encoder.write_all(data)?;
                encoder.finish()?
            }
            4..=6 => compress(data, Some(CompressionMode::FAST(0)), true)?,
            7..=9 => {
                let mut encoder = brotli::CompressorWriter::new(
                    Vec::new(),
                    self.config.compression_level as usize,
                    4096,
                    22,
                );
                encoder.write_all(data)?;
                encoder.into_inner()
            }
            _ => return Ok(data.to_vec()),
        };
        Ok(compressed)
    }

    /// Decompress data using the appropriate algorithm based on the file header
    ///
    /// Supports Gzip, Brotli, and LZ4
    pub fn decompress(&self, data: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
        if !self.config.compression {
            return Ok(data.to_vec());
        }
        let mut decompressed = Vec::new();
        if data.starts_with(&[0x1f, 0x8b]) {
            let mut decoder = GzDecoder::new(Vec::new());
            decoder.write_all(data)?;
            decompressed = decoder.finish()?;
        } else if data.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
            let mut decoder = brotli::Decompressor::new(data, 4096);
            decoder.read_to_end(&mut decompressed)?;
        } else {
            let mut decoder = lz4::Decoder::new(data)?;
            decoder.read_to_end(&mut decompressed)?;
        }
        Ok(decompressed)
    }

    /// Retrieve a file from the cache if it exists.
    ///
    /// # Arguments
    ///
    /// * `url` - The URL that was used to download the file
    ///
    /// # Returns
    ///
    /// - `Ok(Some(data))` if the file exists in cache
    /// - `Ok(None)` if caching is disabled or file doesn't exist
    /// - `Err` on I/O errors
    pub fn get_cached_file(&self, url: &str) -> Result<Option<Vec<u8>>, Box<dyn Error>> {
        if !self.config.cache_enabled {
            return Ok(None);
        }

        let _cache_dir = self.config.cache_dir.as_str();
        let cache_path = self.get_cache_path(url)?;
        if cache_path.exists() {
            let mut file = File::open(cache_path)?;
            let mut contents = Vec::new();
            file.read_to_end(&mut contents)?;
            return Ok(Some(contents));
        }
        Ok(None)
    }

    /// Store a file in the cache.
    ///
    /// Does nothing if caching is disabled.
    pub fn cache_file(&self, url: &str, data: &[u8]) -> Result<(), Box<dyn Error>> {
        if !self.config.cache_enabled {
            return Ok(());
        }
        let cache_path = self.get_cache_path(url)?;
        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = File::create(cache_path)?;
        file.write_all(data)?;
        Ok(())
    }

    /// Generate the cache file path for a URL using a simple hash.
    fn get_cache_path(&self, url: &str) -> Result<PathBuf, Box<dyn Error>> {
        let mut cache_dir = PathBuf::from(if self.config.cache_dir.is_empty() {
            "~/.cache/kget".to_string()
        } else {
            self.config.cache_dir.clone()
        });

        if cache_dir.starts_with("~") {
            if let Some(home) = dirs::home_dir() {
                cache_dir = home.join(cache_dir.strip_prefix("~").unwrap());
            }
        }

        // Simple hash function to generate a unique filename
        let mut hash = 0u64;
        for byte in url.bytes() {
            hash = hash.wrapping_mul(31).wrapping_add(byte as u64);
        }

        cache_dir.push(format!("{:x}", hash));
        Ok(cache_dir)
    }

    /// Get the peer connection limit for torrent downloads.
    ///
    /// Uses the speed limit as a proxy for connection capacity.
    /// Returns 50 if no speed limit is set.
    pub fn get_peer_limit(&self) -> usize {
        self.speed_limit.unwrap_or(50) as usize
    }

    /// Maximum parallel HTTP connections to use for advanced downloads.
    pub fn max_connections(&self) -> usize {
        self.config.max_connections.clamp(1, 32)
    }

    /// Check if compression is enabled.
    pub fn is_compression_enabled(&self) -> bool {
        self.config.compression
    }

    /// Create a new `Optimizer` with default settings.
    ///
    /// Equivalent to `Optimizer::default()`.
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
    /// Create an `Optimizer` with sensible defaults:
    /// - Compression enabled at level 6 (LZ4)
    /// - Caching enabled in ~/.cache/kget
    /// - No speed limit
    /// - 4 max connections
    fn default() -> Self {
        Self {
            config: OptimizationConfig {
                compression: true,
                compression_level: 6,
                cache_enabled: true,
                cache_dir: "~/.cache/kget".to_string(),
                speed_limit: None,
                max_connections: 4,
            },
            speed_limit: None,
        }
    }
}
