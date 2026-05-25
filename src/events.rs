//! Event types emitted by [`DownloadBuilder::spawn`].
//!
//! The channel-based API lets callers react to progress without dealing with
//! closure lifetimes or shared mutable state.
//!
//! # Example
//!
//! ```rust,no_run
//! use kget::{builder, DownloadEvent};
//!
//! let (handle, events) = kget::builder("https://example.com/file.zip").spawn();
//!
//! for event in events {
//!     match event {
//!         DownloadEvent::Progress { percent, speed_bps, .. } =>
//!             println!("{:.1}%  ({} B/s)", percent, speed_bps),
//!         DownloadEvent::Completed { path, .. } =>
//!             println!("Saved to {path}"),
//!         DownloadEvent::Error(msg) =>
//!             eprintln!("Failed: {msg}"),
//!         _ => {}
//!     }
//! }
//!
//! handle.join().unwrap().unwrap();
//! ```

/// An event emitted by an in-progress download.
#[derive(Debug)]
pub enum DownloadEvent {
    /// Periodic progress update.
    Progress {
        /// Completion percentage (0.0 – 100.0).
        percent: f64,
        /// Current transfer speed in bytes per second.
        speed_bps: u64,
        /// Estimated seconds remaining (`None` if unknown).
        eta_secs: Option<u64>,
    },

    /// Informational message from the download engine (e.g. "Connecting…").
    Status(String),

    /// The download finished successfully.
    Completed {
        /// Absolute path of the saved file.  Empty for in-memory downloads.
        path: String,
        /// SHA-256 digest of the file, if verification was requested.
        sha256: Option<String>,
    },

    /// The download failed.  Contains the error message.
    Error(String),
}
