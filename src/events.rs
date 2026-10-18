//! Event types: [`DownloadEvent`] (emitted by [`DownloadBuilder::spawn`]) and
//! [`JsonEvent`], the versioned JSON Lines protocol printed by `kget --jsonl`.
//!
//! [`DownloadBuilder::spawn`]: crate::DownloadBuilder::spawn
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

/// Version of the JSONL protocol (the `"v"` field of every event).
pub const JSONL_VERSION: u32 = 1;

/// One file inside a torrent (`files` event).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileEntry {
    /// Path relative to the torrent root
    pub name: String,
    /// Size in bytes
    pub size: u64,
}

/// Progress of one file inside a torrent (`file_progress` event).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileProgressEntry {
    /// Index in the `files` event
    pub idx: usize,
    /// Bytes downloaded
    pub downloaded: u64,
    /// File size in bytes
    pub size: u64,
    /// 0.0 – 100.0
    pub pct: f64,
}

/// A machine-readable event printed by `kget --jsonl`, one JSON object per line.
///
/// Every line carries `"v": 1` and an `"event"` tag:
///
/// ```text
/// {"v":1,"event":"started","url":"…","protocol":"http","advanced":false,"output":"/abs/x.iso"}
/// {"v":1,"event":"progress","bytes":524288,"total":1048576,"percent":50.0,"speed_bps":3145728,"eta_secs":12}
/// {"v":1,"event":"saved","path":"/abs/x.iso","bytes":1048576}
/// {"v":1,"event":"completed"}
/// {"v":1,"event":"error","code":"http_404","message":"…","retryable":false}
/// ```
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum JsonEvent {
    /// The download started.
    Started {
        url: String,
        protocol: String,
        advanced: bool,
        output: String,
    },
    /// Remote metadata became known.
    Meta {
        total: Option<u64>,
        filename: String,
        resumable: bool,
    },
    /// Periodic progress update (throttled to ~5 per second).
    Progress {
        bytes: u64,
        total: Option<u64>,
        percent: Option<f64>,
        speed_bps: u64,
        eta_secs: Option<u64>,
        /// Legacy 0.0–1.0 fraction (kept for pre-1.7.1 consumers)
        progress: Option<f64>,
    },
    /// Human-readable status message.
    Status { message: String },
    /// Files contained in a torrent.
    Files { files: Vec<FileEntry> },
    /// Per-file progress of a torrent (about once per second).
    FileProgress { files: Vec<FileProgressEntry> },
    /// Result of a checksum verification.
    Verified {
        algorithm: String,
        hash: String,
        ok: bool,
    },
    /// The file was written to its final location.
    Saved { path: String, bytes: u64 },
    /// The download finished successfully.
    Completed,
    /// The download failed.
    Error {
        code: String,
        message: String,
        retryable: bool,
    },
}

impl JsonEvent {
    /// Serialize to a single JSON line (including `"v":1`).
    pub fn to_line(&self) -> String {
        let mut v = serde_json::to_value(self).unwrap_or_default();
        if let Some(obj) = v.as_object_mut() {
            obj.insert("v".into(), JSONL_VERSION.into());
        }
        v.to_string()
    }

    /// Print the event to stdout as one line.
    pub fn emit(&self) {
        use std::io::Write;
        let line = self.to_line();
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }
}

/// Tracks bytes over time to compute speed and ETA for progress events.
#[derive(Debug, Clone)]
pub struct SpeedMeter {
    start: std::time::Instant,
    start_bytes: Option<u64>,
}

impl Default for SpeedMeter {
    fn default() -> Self {
        Self::new()
    }
}

impl SpeedMeter {
    /// Start measuring now.
    pub fn new() -> Self {
        Self {
            start: std::time::Instant::now(),
            start_bytes: None,
        }
    }

    /// Build a `progress` event for `bytes` of `total`.
    pub fn progress(&mut self, bytes: u64, total: Option<u64>) -> JsonEvent {
        let base = *self.start_bytes.get_or_insert(bytes);
        let secs = self.start.elapsed().as_secs_f64().max(0.001);
        let speed_bps = (bytes.saturating_sub(base) as f64 / secs) as u64;
        let percent = total.map(|t| (bytes as f64 / t.max(1) as f64 * 100.0).min(100.0));
        let eta_secs = match total {
            Some(t) if speed_bps > 0 && t > bytes => Some((t - bytes) / speed_bps),
            Some(t) if t <= bytes => Some(0),
            _ => None,
        };
        JsonEvent::Progress {
            bytes,
            total,
            percent,
            speed_bps,
            eta_secs,
            progress: percent.map(|p| p / 100.0),
        }
    }
}
