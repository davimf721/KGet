//! Persistent download history.
//!
//! Every download — successful or not — is appended to a JSON file in the
//! OS config directory (or `KGET_CONFIG_DIR`) so the record survives app
//! restarts.
//!
//! Writes are atomic and protected by a lock file, so several `kget`
//! processes running at once (e.g. the macOS app) never lose entries, and a
//! corrupt file is moved aside to `history.json.corrupt` instead of being
//! overwritten. Credentials embedded in URLs are never stored. The history
//! keeps the most recent 1000 entries.
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
//! let entry = HistoryEntry::new(
//!     "https://example.com/file.iso",
//!     "/home/user/Downloads",
//!     None,
//! );
//! // Safe even with other kget processes writing at the same time
//! DownloadHistory::append(entry, EntryStatus::Completed, None).unwrap();
//!
//! let history = DownloadHistory::load();
//! for e in history.recent(10) {
//!     println!("{} {} {}", e.created_at_display(), e.status, e.filename);
//! }
//! ```

use crate::utils::get_filename_from_url_or_default;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Maximum number of entries kept in the history file.
pub const MAX_ENTRIES: usize = 1000;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

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
    /// Handed to an external application (e.g. the system torrent client);
    /// KGet did not download the data itself.
    #[serde(rename = "handed_off")]
    HandedOff,
}

impl std::fmt::Display for EntryStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `pad` honours width/alignment (`{:<12}`)
        f.pad(match self {
            EntryStatus::Completed => "completed",
            EntryStatus::Failed => "failed",
            EntryStatus::Cancelled => "cancelled",
            EntryStatus::HandedOff => "handed_off",
        })
    }
}

/// A single record in the download history.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HistoryEntry {
    /// Unique hex identifier (timestamp in nanoseconds + process counter).
    pub id: String,
    /// Source URL or magnet link (user name and password removed).
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
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let now = unix_now();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let id = format!(
            "{:012x}{:04x}",
            nanos & 0xffff_ffff_ffff,
            (seq ^ std::process::id() as u64) & 0xffff
        );
        let filename = get_filename_from_url_or_default(url, "download");
        Self {
            id,
            url: redact_url(url),
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

    /// Fill `filename`, `output_dir` and `bytes_total` from the saved file.
    pub fn set_saved_file(&mut self, path: &Path) {
        if let Some(name) = path.file_name() {
            self.filename = name.to_string_lossy().into_owned();
        }
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            self.output_dir = dir.to_string_lossy().into_owned();
        }
        if let Ok(meta) = std::fs::metadata(path)
            && meta.is_file()
        {
            self.bytes_total = Some(meta.len());
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

    /// Read, modify and write the history file under an exclusive lock.
    ///
    /// Safe across processes. A corrupt file is preserved as
    /// `history.json.corrupt` instead of being overwritten.
    pub fn update<F: FnOnce(&mut Vec<HistoryEntry>)>(f: F) -> Result<(), BoxError> {
        let path = history_path();
        let _lock = lock_history(&path)?;
        let mut entries = read_or_quarantine(&path);
        f(&mut entries);
        write_entries(&path, &mut entries)
    }

    /// Finalise `entry` and append it to the history file (cross-process safe).
    pub fn append(
        mut entry: HistoryEntry,
        status: EntryStatus,
        error: Option<String>,
    ) -> Result<(), BoxError> {
        finalise(&mut entry, status, error);
        Self::update(|entries| entries.push(entry))
    }

    /// Write the current in-memory history to disk (atomically, under the lock).
    ///
    /// Prefer [`DownloadHistory::append`] / [`DownloadHistory::update`] when
    /// other processes may write at the same time.
    pub fn save(&self) -> Result<(), BoxError> {
        let _lock = lock_history(&self.path)?;
        let mut entries = self.entries.clone();
        write_entries(&self.path, &mut entries)
    }

    /// Finalise an entry with a status and optional error message, then
    /// append it to the in-memory history.
    pub fn record(&mut self, mut entry: HistoryEntry, status: EntryStatus, error: Option<String>) {
        finalise(&mut entry, status, error);
        self.entries.push(entry);
    }

    /// All history entries in insertion order (oldest first).
    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    /// Up to `n` most-recent entries, newest first.
    pub fn recent(&self, n: usize) -> Vec<&HistoryEntry> {
        let mut v: Vec<&HistoryEntry> = self.entries.iter().collect();
        // Stable sort: entries created in the same second keep append order.
        v.reverse();
        v.sort_by_key(|e| std::cmp::Reverse(e.created_at));
        v.into_iter().take(n).collect()
    }

    /// Remove finished entries (`Completed`, `Cancelled` and `HandedOff`),
    /// keeping only failures.
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

fn finalise(entry: &mut HistoryEntry, status: EntryStatus, error: Option<String>) {
    entry.status = status;
    entry.finished_at = Some(unix_now());
    entry.error = error;
}

fn history_path() -> PathBuf {
    crate::config::config_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("history.json")
}

fn lock_history(path: &Path) -> Result<std::fs::File, BoxError> {
    if let Some(p) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(p)?;
    }
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path.with_extension("lock"))?;
    lock.lock_exclusive()?;
    // Unlocked when the file is dropped.
    Ok(lock)
}

fn read_or_quarantine(path: &Path) -> Vec<HistoryEntry> {
    match std::fs::read_to_string(path) {
        Ok(s) if !s.trim().is_empty() => match serde_json::from_str(&s) {
            Ok(v) => v,
            Err(_) => {
                let _ = std::fs::rename(path, path.with_extension("json.corrupt"));
                Vec::new()
            }
        },
        _ => Vec::new(),
    }
}

fn write_entries(path: &Path, entries: &mut Vec<HistoryEntry>) -> Result<(), BoxError> {
    if entries.len() > MAX_ENTRIES {
        entries.drain(..entries.len() - MAX_ENTRIES);
    }
    let json = serde_json::to_vec_pretty(entries)?;
    crate::config::write_atomic(path, &json, true)?;
    Ok(())
}

fn try_load(path: &Path) -> Option<Vec<HistoryEntry>> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Remove the user name and password from a URL before storing it.
///
/// ```rust
/// use kget::queue::redact_url;
///
/// assert_eq!(redact_url("sftp://me:secret@host/f"), "sftp://host/f");
/// assert_eq!(redact_url("https://example.com/a"), "https://example.com/a");
/// ```
pub fn redact_url(raw: &str) -> String {
    match url::Url::parse(raw) {
        Ok(mut u) if u.password().is_some() || !u.username().is_empty() => {
            let _ = u.set_password(None);
            let _ = u.set_username("");
            u.to_string()
        }
        _ => raw.to_string(),
    }
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
