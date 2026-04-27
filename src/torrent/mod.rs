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

use crate::config::ProxyConfig;
use crate::optimization::Optimizer;

mod external;
mod settings;

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
    _output_dir: &str,
    _quiet: bool,
    _proxy: ProxyConfig,
    _optimizer: Optimizer,
    cb: TorrentCallbacks,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    if !is_supported_magnet_link(magnet) {
        return Err("Invalid or unsupported BitTorrent magnet link".into());
    }

    emit_progress(&cb, 0.0);

    match selected_backend().as_str() {
        "native" => {
            #[cfg(feature = "torrent-native")]
            {
                return native::download_magnet_native(
                    magnet,
                    _output_dir,
                    _quiet,
                    _proxy,
                    _optimizer,
                    cb,
                );
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
                    magnet,
                    _output_dir,
                    _quiet,
                    _proxy,
                    _optimizer,
                    cb,
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
            "Opening magnet link in your default torrent client (output folder may be managed by that client): {}",
            magnet
        ),
    );

    external::open_magnet_in_default_client(magnet)?;
    emit_progress(&cb, 1.0);
    Ok(())
}
