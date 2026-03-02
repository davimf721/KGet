//! # KGet - A Powerful Download Library for Rust
//!
//! `kget` provides robust downloading capabilities for modern applications:
//!
//! - **HTTP/HTTPS downloads** with parallel connections (up to 32x speed)
//! - **FTP/SFTP support** for legacy and secure file transfers  
//! - **BitTorrent** via magnet links with native client (requires `torrent-native` feature)
//! - **ISO verification** with automatic SHA-256 integrity checking
//! - **Auto-optimization** based on file type and network conditions
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use kget::{download, DownloadOptions, Config, ProxyConfig, Optimizer};
//!
//! // Simple download
//! let config = Config::default();
//! let proxy = ProxyConfig::default();
//! let optimizer = Optimizer::new();
//! download("https://example.com/file.zip", false, None, &config, proxy, optimizer).unwrap();
//! ```
//!
//! ## Advanced Download with Progress
//!
//! ```rust,no_run
//! use kget::{AdvancedDownloader, ProxyConfig, Optimizer};
//! use std::sync::Arc;
//!
//! let mut downloader = AdvancedDownloader::new(
//!     "https://example.com/large.iso".to_string(),
//!     "large.iso".to_string(),
//!     false,
//!     ProxyConfig::default(),
//!     Optimizer::new(),
//! );
//!
//! // Set progress callback
//! downloader.set_progress_callback(Arc::new(|progress| {
//!     println!("Progress: {:.1}%", progress * 100.0);
//! }));
//!
//! downloader.download().unwrap();
//! ```
//!
//! ## Torrent Downloads
//!
//! ```rust,no_run
//! use kget::torrent::{download_magnet, TorrentCallbacks};
//! use kget::{ProxyConfig, Optimizer};
//! use std::sync::Arc;
//!
//! let callbacks = TorrentCallbacks {
//!     status: Some(Arc::new(|msg| println!("Status: {}", msg))),
//!     progress: Some(Arc::new(|p| println!("Progress: {:.1}%", p * 100.0))),
//! };
//!
//! download_magnet(
//!     "magnet:?xt=urn:btih:...",
//!     "./downloads",
//!     false,
//!     ProxyConfig::default(),
//!     Optimizer::new(),
//!     callbacks,
//! ).unwrap();
//! ```
//!
//! ## Features
//!
//! - `gui` - Cross-platform GUI using egui (includes `torrent-native`)
//! - `torrent-native` - Native BitTorrent client using librqbit
//! - `torrent-transmission` - Transmission RPC integration

// Core modules
pub mod config;
pub mod download;
pub mod advanced_download;
pub mod optimization;
pub mod progress;
pub mod utils;

// Protocol modules
pub mod ftp;
pub mod sftp;
pub mod torrent;

// Re-exports: Configuration
pub use config::{Config, ProxyConfig, ProxyType};

// Re-exports: Core download functionality
pub use download::{download, verify_iso_integrity};
pub use advanced_download::AdvancedDownloader;
pub use optimization::Optimizer;
pub use progress::create_progress_bar;

// Re-exports: Torrent types (when available)
pub use torrent::{download_magnet, TorrentCallbacks};

// Re-exports: Utilities
pub use utils::{get_filename_from_url_or_default, resolve_output_path, print};

/// Options for configuring a download operation.
///
/// # Example
///
/// ```rust
/// use kget::DownloadOptions;
///
/// let options = DownloadOptions {
///     quiet_mode: true,
///     output_path: Some("./downloads/file.zip".to_string()),
///     verify_iso: false,
/// };
/// ```
#[derive(Debug, Clone)]
pub struct DownloadOptions {
    /// Suppress progress output to stdout
    pub quiet_mode: bool,
    /// Custom output path (uses URL filename if None)
    pub output_path: Option<String>,
    /// Automatically verify SHA-256 for ISO files
    pub verify_iso: bool,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            quiet_mode: false,
            output_path: None,
            verify_iso: false,
        }
    }
}

/// Type alias for progress callbacks (0.0 to 1.0)
pub type ProgressCallback = std::sync::Arc<dyn Fn(f32) + Send + Sync>;

/// Type alias for status message callbacks
pub type StatusCallback = std::sync::Arc<dyn Fn(String) + Send + Sync>;

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::{
        Config, ProxyConfig, ProxyType,
        AdvancedDownloader, Optimizer,
        DownloadOptions, ProgressCallback, StatusCallback,
        download, verify_iso_integrity,
    };
    pub use crate::torrent::{download_magnet, TorrentCallbacks};
}