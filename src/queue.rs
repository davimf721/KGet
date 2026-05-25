//! Persistent download history.
//!
//! Every download — successful or not — is appended to a JSON file in the
//! OS config directory so the record survives app restarts.
//!
//! **File locations**
//! - macOS:   `~/Library/Application Support/kget/history.json`
//! - Linux:   `~/.config/kget/history.json`
//! - Windows: `%APPDATA%\kget\history.json`
//!
//! # Example
//!
//! ```rust,no_run
//! use kget::queue::{DownloadHistory, EntryStatus, HistoryEntry};
//!
//! let mut history = DownloadHistory::load();
//!
//! let mut entry = HistoryEntry::new(
//!     "https://example.com/file.iso",
//!     "/home/user/Downloads",
//!     None,
//! );
//! history.record(entry, EntryStatus::Completed, None);
//! history.save().unwrap();
//!
//! for e in history.recent(10) {
//!     println!("{} {} {}", e.created_at_display(), e.status, e.filename);
//! }
//! ```

use crate::utils::get_filename_from_url_or_default;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// Public types
// ============================================================================

/// Outcome of a completed download attempt.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum EntryStatus {
    Completed,
    Failed,
    Cancelled,
}

impl std::fmt::Display for EntryStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EntryStatus::Completed => write!(f, "completed"),
            EntryStatus::Failed => write!(f, "failed"),
            EntryStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// A single record in the download history.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HistoryEntry {
    /// Short hex identifier derived from the URL and creation time.
    pub id: String,
    /// Source URL or magnet link.
    pub url: String,
    /// Filename extracted from the URL or Content-Disposition.
    pub filename: String,
    /// Directory where the file was (or would be) saved.
    pub output_dir: String,
    /// Final outcome.
    pub status: EntryStatus,
    /// File size in bytes, if known after the download.
    pub bytes_total: Option<u64>,
    /// SHA-256 of the completed file, if computed.
    pub sha256: Option<String>,
    /// Expected SHA-256 supplied by the user, if any.
    pub expected_sha256: Option<String>,
    /// Unix timestamp (seconds UTC) when the download was enqueued.
    pub created_at: u64,
    /// Unix timestamp (seconds UTC) when the download finished.
    pub finished_at: Option<u64>,
    /// Error message for failed downloads.
    pub error: Option<String>,
}

impl HistoryEntry {
    /// Create a new pending entry.  Call [`DownloadHistory::record`] to
    /// finalise it with a status and optional error once the download is done.
    pub fn new(url: &str, output_dir: &str, expected_sha256: Option<&str>) -> Self {
        let now = unix_now();
        let url_hash: u64 = url
            .bytes()
            .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
        let id = format!("{:08x}{:08x}", now as u32, url_hash as u32);
        let filename = get_filename_from_url_or_default(url, "download");
        Self {
            id,
            url: url.to_string(),
            filename,
            output_dir: output_dir.to_string(),
            status: EntryStatus::Completed,
            bytes_total: None,
            sha256: None,
            expected_sha256: expected_sha256.map(str::to_string),
            created_at: now,
            finished_at: None,
            error: None,
        }
    }

    /// Human-readable UTC string for `created_at` (e.g. `2026-05-21 14:32 UTC`).
    pub fn created_at_display(&self) -> String {
        format_unix(self.created_at)
    }

    /// Human-readable UTC string for `finished_at`, if set.
    pub fn finished_at_display(&self) -> Option<String> {
        self.finished_at.map(format_unix)
    }
}

/// Persistent download history backed by a JSON file in the OS config dir.
pub struct DownloadHistory {
    entries: Vec<HistoryEntry>,
    path: PathBuf,
}

impl DownloadHistory {
    /// Load history from disk.  Returns an empty history if the file does
    /// not exist or cannot be parsed.
    pub fn load() -> Self {
        let path = history_path();
        let entries = try_load(&path).unwrap_or_default();
        Self { entries, path }
    }

    /// Write the current history to disk.
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.entries)?;
        std::fs::write(&self.path, json)?;
        Ok(())
    }

    /// Finalise an entry with a status and optional error message, then
    /// append (or replace, if the same URL already has an entry) it.
    pub fn record(&mut self, mut entry: HistoryEntry, status: EntryStatus, error: Option<String>) {
        entry.status = status;
        entry.finished_at = Some(unix_now());
        entry.error = error;

        if let Some(pos) = self.entries.iter().position(|e| e.url == entry.url) {
            self.entries[pos] = entry;
        } else {
            self.entries.push(entry);
        }
    }

    /// All history entries in insertion order (oldest first).
    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    /// Up to `n` most-recent entries, newest first.
    pub fn recent(&self, n: usize) -> Vec<&HistoryEntry> {
        let mut v: Vec<&HistoryEntry> = self.entries.iter().collect();
        v.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        v.into_iter().take(n).collect()
    }

    /// Remove entries whose status is `Completed` or `Cancelled`.
    ///
    /// Returns the number of entries removed.
    pub fn clear_completed(&mut self) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| e.status == EntryStatus::Failed);
        before - self.entries.len()
    }

    /// Remove every history entry.
    ///
    /// Returns the number of entries removed.
    pub fn clear_all(&mut self) -> usize {
        let n = self.entries.len();
        self.entries.clear();
        n
    }

    /// Filesystem path of the history file (OS config dir).
    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

// ============================================================================
// Internal helpers
// ============================================================================

fn history_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("kget")
        .join("history.json")
}

fn try_load(path: &PathBuf) -> Option<Vec<HistoryEntry>> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Format a Unix timestamp as `YYYY-MM-DD HH:MM UTC` without external crates.
///
/// Uses the civil-date algorithm described by Howard Hinnant.
pub fn format_unix(secs: u64) -> String {
    let days_since_epoch = secs / 86400;
    let time_of_day = secs % 86400;
    let h = time_of_day / 3600;
    let m = (time_of_day % 3600) / 60;

    let (y, mo, d) = civil_date(days_since_epoch);
    format!("{:04}-{:02}-{:02} {:02}:{:02} UTC", y, mo, d, h, m)
}

/// Convert days since Unix epoch (1970-01-01) to (year, month, day).
fn civil_date(days: u64) -> (u64, u64, u64) {
    // Shift epoch to 0000-03-01 to simplify leap-year arithmetic.
    let z = days as i64 + 719468;
    let era: i64 = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64; // day of era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // year of era [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day of year [0, 365]
    let mp = (5 * doy + 2) / 153; // month of year (March-based) [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // day [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // month [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y as u64, m, d)
}
