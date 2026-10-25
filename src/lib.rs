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
//! use kget::{download, DownloadOptions, ProxyConfig, Optimizer};
//!
//! // Simple download
//! let proxy = ProxyConfig::default();
//! let optimizer = Optimizer::new();
//! let options = DownloadOptions::default();
//! download("https://example.com/file.zip", proxy, optimizer, options, None).unwrap();
//! ```
//!
//! ## Advanced Download with Progress
//!
//! ```rust,no_run
//! use kget::{AdvancedDownloader, ProxyConfig, Optimizer};
//!
//! let mut downloader = AdvancedDownloader::new(
//!     "https://example.com/large.iso".to_string(),
//!     "large.iso".to_string(),
//!     false,
//!     ProxyConfig::default(),
//!     Optimizer::new(),
//! )?;
//!
//! // Set progress callback
//! downloader.set_progress_callback(|progress| {
//!     println!("Progress: {:.1}%", progress * 100.0);
//! });
//!
//! downloader.download()?;
//! # Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
//! ```
//!
//! Interrupted downloads are kept as `<file>.part` (plus a `<file>.kget`
//! resume state for turbo mode) and continue where they stopped on the next run.
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
//! - `keychain` (default) - Save passwords in the OS credential store ([`credentials`])

// Library code runs inside long-lived hosts (the daemon, the MCP server, apps
// embedding `kget-ffi`) where a panic takes the host down: new `unwrap` /
// `expect` / `panic!` calls are flagged outside tests. Each remaining one has
// an `#[allow]` with the reason it cannot fail. See `guard` for the policy on
// poisoned locks and spawned threads.
#![cfg_attr(
    not(test),
    warn(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented
    )
)]

// Core modules
pub mod advanced_download;
pub mod app;
pub mod builder;
pub mod checksum;
pub mod cli;
pub mod config;
pub mod credentials;
pub mod daemon;
pub mod dedupe;
pub mod download;
pub mod error;
pub mod events;
pub mod fetch;
#[doc(hidden)]
pub mod fuzz_api;
pub(crate) mod guard;
pub mod http;
pub mod jobs;
pub mod mcp;
pub mod metalink;
pub mod native_host;
pub mod optimization;
pub mod organize;
pub mod progress;
pub mod protocol;
pub mod queue;
pub mod rules;
pub mod runner;
pub mod utils;
pub mod zip_preview;

// Protocol modules
pub mod ftp;
pub mod hf;
pub mod ollama;
pub mod sftp;
pub mod torrent;
pub mod webdav;
pub mod ytdlp;

// Re-exports: Configuration
pub use config::{Config, ProxyConfig, ProxyType};

// Re-exports: Core download functionality
pub use advanced_download::{AdvancedDownloader, ResumePolicy};
pub use download::{download, download_to_path, verify_file_sha256, verify_iso_integrity};
pub use optimization::Optimizer;
pub use progress::{Throttle, create_progress_bar};
pub use protocol::Protocol;

// Re-exports: Torrent types (when available)
pub use torrent::{TorrentCallbacks, download_magnet};

// Re-exports: Utilities
pub use utils::{
    auto_extract, expand_tilde, get_filename_from_url_or_default, is_extractable, print,
    resolve_output_path, sanitize_filename,
};
pub use zip_preview::{ZipEntryInfo, inspect_zip_target};

// Re-exports: Protocol helpers
pub use webdav::is_webdav_url;
pub use ytdlp::{is_video_url, ytdlp_available, ytdlp_binary};

// Re-exports: High-level builder API (v1.7.0+)
pub use builder::{
    Backoff, BatchBuilder, BatchResult, ComputedChecksums, DownloadBuilder, DownloadResult,
    RetryConfig,
};
pub use checksum::ChecksumAlgorithm;
pub use error::KgetError;
pub use events::{DownloadEvent, JsonEvent};

// Re-exports: Job service shared by every frontend (v1.8.0+)
pub use jobs::{DownloadState, JobError, JobEvent, JobId, JobInfo, JobManager, JobProgress};
pub use runner::{DownloadJob, Reporter, RunContext};

/// Create a [`DownloadBuilder`] for a single URL — the recommended API entry point.
///
/// # Example
///
/// ```rust,no_run
/// use kget::KgetError;
///
/// let result = kget::builder("https://example.com/file.zip")
///     .output("./downloads/")
///     .connections(4)
///     .sha256("abc123...")
///     .download()?;
/// # Ok::<(), KgetError>(())
/// ```
pub fn builder(url: impl Into<String>) -> DownloadBuilder {
    DownloadBuilder::new(url)
}

/// Create a [`BatchBuilder`] for downloading multiple URLs concurrently.
///
/// # Example
///
/// ```rust,no_run
/// let results = kget::batch(["https://a.com/f1.zip", "https://b.com/f2.iso"])
///     .concurrency(4)
///     .output_dir("./downloads/")
///     .download_all();
/// ```
pub fn batch(urls: impl IntoIterator<Item = impl Into<String>>) -> BatchBuilder {
    BatchBuilder::new(urls)
}

/// What to do when the destination file already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OverwritePolicy {
    /// Replace the existing file (default, wget-compatible).
    #[default]
    Overwrite,
    /// Keep the existing file and skip the download (`--no-clobber`).
    Skip,
    /// Save under a new name: `file (1).zip`, `file (2).zip`…
    Rename,
}

/// Callback receiving `(bytes_downloaded, total_bytes)` during a transfer.
pub type ByteProgressCallback = std::sync::Arc<dyn Fn(u64, Option<u64>) + Send + Sync>;

/// Options for configuring a download operation.
///
/// Always build it with `..Default::default()` so new fields added in future
/// versions do not break your code.
///
/// # Example
///
/// ```rust
/// use kget::DownloadOptions;
///
/// let options = DownloadOptions {
///     quiet_mode: true,
///     output_path: Some("./downloads/file.zip".to_string()),
///     ..Default::default()
/// };
/// ```
#[derive(Clone)]
pub struct DownloadOptions {
    /// Suppress progress output to stdout
    pub quiet_mode: bool,
    /// Custom output path (uses URL filename if None)
    pub output_path: Option<String>,
    /// Automatically verify SHA-256 for ISO files
    pub verify_iso: bool,
    /// Expected SHA-256 hash for automatic integrity comparison
    pub expected_sha256: Option<String>,
    /// Extra HTTP headers sent with every request (e.g. `("Referer", "https://…")`)
    pub extra_headers: Vec<(String, String)>,
    /// Continue an existing `<file>.part` instead of starting over (`-c`)
    pub resume: bool,
    /// Behaviour when the destination already exists
    pub overwrite: OverwritePolicy,
    /// Maximum attempts per request (default 3)
    pub retries: u32,
    /// Connect / per-read timeout in seconds (default 60, 0 = no read timeout)
    pub timeout_secs: u64,
    /// Set to `true` from another thread to stop the download (the `.part` file is kept)
    pub cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Structured progress callback `(bytes, total)`, throttled to ~5 calls per second
    pub progress_callback: Option<ByteProgressCallback>,
}

impl std::fmt::Debug for DownloadOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DownloadOptions")
            .field("quiet_mode", &self.quiet_mode)
            .field("output_path", &self.output_path)
            .field("verify_iso", &self.verify_iso)
            .field("expected_sha256", &self.expected_sha256)
            .field("extra_headers", &self.extra_headers)
            .field("resume", &self.resume)
            .field("overwrite", &self.overwrite)
            .field("retries", &self.retries)
            .field("timeout_secs", &self.timeout_secs)
            .field("cancel", &self.cancel.is_some())
            .field("progress_callback", &self.progress_callback.is_some())
            .finish()
    }
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            quiet_mode: false,
            output_path: None,
            verify_iso: false,
            expected_sha256: None,
            extra_headers: Vec::new(),
            resume: false,
            overwrite: OverwritePolicy::default(),
            retries: http::DEFAULT_RETRIES,
            timeout_secs: http::DEFAULT_TIMEOUT_SECS,
            cancel: None,
            progress_callback: None,
        }
    }
}

/// Type alias for progress callbacks (0.0 to 1.0)
pub type ProgressCallback = std::sync::Arc<dyn Fn(f32) + Send + Sync>;

/// Type alias for status message callbacks
pub type StatusCallback = std::sync::Arc<dyn Fn(String) + Send + Sync>;

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::torrent::{TorrentCallbacks, download_magnet};
    pub use crate::{
        AdvancedDownloader, Config, DownloadOptions, KgetError, Optimizer, OverwritePolicy,
        ProgressCallback, Protocol, ProxyConfig, ProxyType, StatusCallback, download,
        download_to_path, verify_iso_integrity,
    };
}
