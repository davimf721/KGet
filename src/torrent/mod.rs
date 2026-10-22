//! BitTorrent download support.
//!
//! This module provides torrent downloading capabilities through multiple backends:
//!
//! - **Native** (`torrent-native` feature): Built-in BitTorrent client using librqbit
//! - **Transmission** (`torrent-transmission` feature): Transmission daemon RPC
//! - **External**: Opens magnet links in the system's default torrent client
//!
//! # Native Torrent Client
//!
//! The native client provides full BitTorrent protocol support:
//! - Magnet link parsing and metadata download
//! - DHT (Distributed Hash Table) for peer discovery
//! - Parallel piece downloading
//! - Progress callbacks for UI integration
//!
//! # Example
//!
//! ```rust,no_run
//! use kget::torrent::{download_magnet, TorrentCallbacks};
//! use kget::{ProxyConfig, Optimizer};
//! use std::sync::Arc;
//!
//! let callbacks = TorrentCallbacks {
//!     status: Some(Arc::new(|msg| println!("{}", msg))),
//!     progress: Some(Arc::new(|p| println!("{:.1}%", p * 100.0))),
//! };
//!
//! download_magnet(
//!     "magnet:?xt=urn:btih:...",
//!     "./downloads",
//!     false,
//!     ProxyConfig::default(),
//!     Optimizer::new(),
//!     callbacks,
//! ).expect("Torrent download failed");
//! ```
//!
//! # Backend Selection
//!
//! The backend is selected via the `KGET_TORRENT_BACKEND` environment variable:
//! - `native`: Use built-in client (requires `torrent-native` feature)
//! - `transmission`: Use Transmission RPC (requires `torrent-transmission` feature)
//! - Any other value: Open in system's default torrent client
//!
//! If not set, defaults to `native` when available, otherwise `external`.

use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::config::{ProxyConfig, TorrentConfig};
use crate::optimization::Optimizer;

mod external;
pub mod selection;
mod settings;

pub use selection::{
    FileSelection, SeedLimits, SeedStop, TorrentJobOptions, glob_match, parse_select,
};

#[cfg(feature = "torrent-transmission")]
mod transmission;

#[cfg(feature = "torrent-native")]
mod native;

/// Type alias for status message callbacks.
pub type StatusCb = Arc<dyn Fn(String) + Send + Sync>;

/// Type alias for progress callbacks (0.0 to 1.0).
pub type ProgressCb = Arc<dyn Fn(f32) + Send + Sync>;

/// Callbacks for torrent download progress and status updates.
///
/// Both callbacks are optional. If not provided, no updates are sent.
///
/// # Example
///
/// ```rust
/// use kget::torrent::TorrentCallbacks;
/// use std::sync::Arc;
///
/// // With callbacks
/// let callbacks = TorrentCallbacks {
///     status: Some(Arc::new(|msg| println!("Status: {}", msg))),
///     progress: Some(Arc::new(|p| println!("Progress: {:.1}%", p * 100.0))),
/// };
///
/// // Without callbacks
/// let silent = TorrentCallbacks::default();
/// ```
#[derive(Default, Clone)]
pub struct TorrentCallbacks {
    /// Callback for human-readable status messages
    pub status: Option<StatusCb>,
    /// Callback for progress updates (0.0 to 1.0)
    pub progress: Option<ProgressCb>,
}

/// Callback receiving the torrent's file list `(relative name, size)`.
pub type FilesCb = Arc<dyn Fn(Vec<(String, u64)>) + Send + Sync>;

/// Callback receiving the bytes downloaded per file (same order as the file list).
pub type FileProgressCb = Arc<dyn Fn(Vec<u64>) + Send + Sync>;

/// Callback receiving swarm statistics (peers, speeds, ratio) about once per
/// second while downloading and seeding.
pub type StatsCb = Arc<dyn Fn(crate::events::TorrentStats) + Send + Sync>;

/// Extra settings for [`download_torrent`].
#[derive(Clone, Default)]
pub struct TorrentOptions {
    /// Port, DHT and peer settings (native backend)
    pub config: TorrentConfig,
    /// Set to `true` to stop the download
    pub cancel: Option<Arc<AtomicBool>>,
    /// Called once the file list is known (native backend)
    pub on_files: Option<FilesCb>,
    /// Called about once per second with per-file progress (native backend)
    pub on_file_progress: Option<FileProgressCb>,
    /// Called about once per second with swarm statistics (native and
    /// Transmission backends)
    pub on_stats: Option<StatsCb>,
    /// Files to download (empty = all)
    pub selection: FileSelection,
    /// Seed after downloading until one of these limits is reached (native
    /// backend; Transmission applies the ratio on the daemon)
    pub seed: SeedLimits,
}

/// Fetch the metadata of a magnet link or `.torrent` and return its files
/// `(path inside the torrent, size)` in torrent order, without downloading
/// (`kget --list-files`).
///
/// Requires the native client (`torrent-native` feature).
#[cfg_attr(not(feature = "torrent-native"), allow(unused_variables))]
pub fn list_files(
    source: &str,
    config: &TorrentConfig,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<Vec<(String, u64)>, Box<dyn Error + Send + Sync>> {
    let source = source.trim();
    if !is_supported_torrent_source(source) {
        return Err("Invalid or unsupported BitTorrent magnet link or .torrent file".into());
    }
    #[cfg(feature = "torrent-native")]
    {
        native::list_files_native(source, config, cancel)
    }
    #[cfg(not(feature = "torrent-native"))]
    {
        Err("Listing torrent files needs the native torrent client (rebuild with --features torrent-native)".into())
    }
}

/// How a torrent request ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TorrentOutcome {
    /// KGet downloaded the data itself.
    Downloaded,
    /// The link/file was opened in an external torrent client; nothing was
    /// downloaded by KGet.
    HandedOff,
}

fn emit_status(cb: &TorrentCallbacks, msg: impl Into<String>) {
    if let Some(f) = &cb.status {
        f(msg.into());
    }
}

fn emit_progress(cb: &TorrentCallbacks, p: f32) {
    if let Some(f) = &cb.progress {
        f(p.clamp(0.0, 1.0));
    }
}

fn selected_backend() -> String {
    std::env::var("KGET_TORRENT_BACKEND")
        .unwrap_or_else(|_| {
            // Default to native if available, otherwise external
            #[cfg(feature = "torrent-native")]
            {
                "native".to_string()
            }
            #[cfg(not(feature = "torrent-native"))]
            {
                "external".to_string()
            }
        })
        .to_lowercase()
}

/// Return true when a magnet link looks like a supported BitTorrent magnet.
pub fn is_supported_magnet_link(magnet: &str) -> bool {
    let lower = magnet.to_ascii_lowercase();
    lower.starts_with("magnet:?")
        && (lower.contains("xt=urn:btih:") || lower.contains("xt=urn:btmh:"))
}

/// Return true for a `.torrent` file path or an `http(s)://…/.torrent` URL.
pub fn is_torrent_file(source: &str) -> bool {
    let lower = source.trim().to_ascii_lowercase();
    let path = lower.split(['?', '#']).next().unwrap_or(&lower);
    path.ends_with(".torrent")
}

/// Return true when `source` is a magnet link or a `.torrent` file/URL.
pub fn is_supported_torrent_source(source: &str) -> bool {
    is_supported_magnet_link(source) || is_torrent_file(source)
}

/// Download a torrent from a magnet link.
///
/// This function automatically selects the best available backend:
/// 1. Native client (if `torrent-native` feature is enabled)
/// 2. Transmission RPC (if `torrent-transmission` feature is enabled)
/// 3. External client (opens in system default torrent app)
///
/// Override the backend with `KGET_TORRENT_BACKEND` environment variable.
///
/// # Arguments
///
/// * `magnet` - Magnet link starting with `magnet:?`
/// * `output_dir` - Directory to save downloaded files
/// * `quiet` - Suppress console output
/// * `proxy` - Proxy configuration (native backend only)
/// * `optimizer` - Optimizer for peer limits
/// * `cb` - Callbacks for progress and status updates
///
/// # Example
///
/// ```rust,no_run
/// use kget::torrent::{download_magnet, TorrentCallbacks};
/// use kget::{ProxyConfig, Optimizer};
/// use std::sync::Arc;
///
/// // Simple download
/// download_magnet(
///     "magnet:?xt=urn:btih:HASH&dn=filename",
///     "/home/user/Downloads",
///     false,
///     ProxyConfig::default(),
///     Optimizer::new(),
///     TorrentCallbacks::default(),
/// ).unwrap();
///
/// // With progress tracking
/// let callbacks = TorrentCallbacks {
///     status: Some(Arc::new(|msg| println!("{}", msg))),
///     progress: Some(Arc::new(|p| {
///         update_progress_bar(p);
///     })),
/// };
///
/// download_magnet(
///     "magnet:?xt=urn:btih:HASH",
///     "./downloads",
///     true, // quiet mode
///     ProxyConfig::default(),
///     Optimizer::new(),
///     callbacks,
/// ).unwrap();
///
/// fn update_progress_bar(p: f32) {
///     // Update UI
/// }
/// ```
///
/// # Errors
///
/// Returns an error if:
/// - Magnet link is invalid
/// - Network connection fails
/// - Output directory cannot be accessed
/// - Download is interrupted
pub fn download_magnet(
    magnet: &str,
    output_dir: &str,
    quiet: bool,
    proxy: ProxyConfig,
    optimizer: Optimizer,
    cb: TorrentCallbacks,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    download_torrent(
        magnet,
        output_dir,
        quiet,
        proxy,
        optimizer,
        cb,
        TorrentOptions::default(),
    )
    .map(|_| ())
}

/// Download a magnet link or `.torrent` file/URL, honouring [`TorrentOptions`].
///
/// Unlike [`download_magnet`], the result tells whether KGet downloaded the
/// data ([`TorrentOutcome::Downloaded`]) or only opened it in an external
/// client ([`TorrentOutcome::HandedOff`]).
#[cfg_attr(not(feature = "torrent-native"), allow(unused_variables))]
pub fn download_torrent(
    source: &str,
    output_dir: &str,
    quiet: bool,
    proxy: ProxyConfig,
    optimizer: Optimizer,
    cb: TorrentCallbacks,
    opts: TorrentOptions,
) -> Result<TorrentOutcome, Box<dyn Error + Send + Sync>> {
    let magnet = source.trim();
    if !is_supported_torrent_source(magnet) {
        return Err("Invalid or unsupported BitTorrent magnet link or .torrent file".into());
    }

    emit_progress(&cb, 0.0);

    match selected_backend().as_str() {
        "native" => {
            #[cfg(feature = "torrent-native")]
            {
                return native::download_magnet_native(
                    magnet, output_dir, quiet, proxy, optimizer, cb, opts,
                )
                .map(|_| TorrentOutcome::Downloaded);
            }

            #[cfg(not(feature = "torrent-native"))]
            {
                emit_status(
                    &cb,
                    "Native torrent backend not available (compile with --features torrent-native). Falling back to external client.",
                );
            }
        }
        "transmission" => {
            #[cfg(feature = "torrent-transmission")]
            {
                return transmission::download_via_transmission(
                    magnet, output_dir, quiet, proxy, optimizer, cb, opts,
                );
            }

            #[cfg(not(feature = "torrent-transmission"))]
            {
                emit_status(
                    &cb,
                    "Torrent backend 'transmission' not available (compile with --features torrent-transmission). Falling back to external client.",
                );
            }
        }
        _ => {}
    }

    emit_status(
        &cb,
        format!(
            "Opening in your default torrent client (KGet does not download it; the output folder is managed by that client): {}",
            magnet
        ),
    );

    external::open_magnet_in_default_client(magnet)?;
    Ok(TorrentOutcome::HandedOff)
}
