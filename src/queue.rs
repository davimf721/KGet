//! Persistent download history.
//!
//! Every download — successful or not — is recorded in a SQLite database in
//! the OS config directory (or `KGET_CONFIG_DIR`) so the record survives app
//! restarts and can be searched and filtered ([`HistoryQuery`]).
//!
//! The database runs in WAL mode with a busy timeout and every write is an
//! immediate transaction, so several `kget` processes running at once (batch
//! mode, the macOS app) never lose entries. The schema is versioned with
//! `PRAGMA user_version` and migrated forward on open. A `history.json` left
//! by KGet ≤ 1.7 is imported automatically and kept as `history.json.bak`; a
//! database that cannot be read is moved aside to `history.db.corrupt`
//! instead of being overwritten. Credentials embedded in URLs are never
//! stored. The history keeps the most recent [`MAX_ENTRIES`] entries.
//!
//! **File locations**
//! - macOS:   `~/Library/Application Support/kget/history.db`
//! - Linux:   `~/.config/kget/history.db`
//! - Windows: `%APPDATA%\kget\history.db`
//!
//! # Example
//!
//! ```rust,no_run
//! use kget::queue::{DownloadHistory, EntryStatus, HistoryEntry, HistoryQuery};
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
//!
//! // Failed ISO downloads from example.com, newest first
//! let q = HistoryQuery::new()
//!     .status(EntryStatus::Failed)
//!     .domain("example.com")
//!     .file_type("iso")
//!     .limit(20);
//! for e in DownloadHistory::query(&q).unwrap() {
//!     println!("{} {}", e.filename, e.error.unwrap_or_default());
//! }
//! ```

use crate::utils::get_filename_from_url_or_default;
use rusqlite::types::Value;
use rusqlite::{Connection, ErrorCode, OptionalExtension, Row, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Maximum number of entries kept in the history database.
pub const MAX_ENTRIES: usize = 10_000;

/// Current database schema version (`PRAGMA user_version`).
pub const SCHEMA_VERSION: i32 = 1;

/// How long a writer waits for another process holding the database lock.
const BUSY_TIMEOUT: Duration = Duration::from_secs(15);

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

impl EntryStatus {
    /// Stable lowercase name, as stored in the database and JSON.
    pub fn as_str(&self) -> &'static str {
        match self {
            EntryStatus::Completed => "completed",
            EntryStatus::Failed => "failed",
            EntryStatus::Cancelled => "cancelled",
            EntryStatus::HandedOff => "handed_off",
        }
    }
}

impl std::fmt::Display for EntryStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `pad` honours width/alignment (`{:<12}`)
        f.pad(self.as_str())
    }
}

impl FromStr for EntryStatus {
    type Err = String;

    /// Parse `completed`/`done`, `failed`, `cancelled`/`canceled` or
    /// `handed_off` (case-insensitive).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "completed" | "complete" | "done" => Ok(EntryStatus::Completed),
            "failed" | "error" => Ok(EntryStatus::Failed),
            "cancelled" | "canceled" => Ok(EntryStatus::Cancelled),
            "handed_off" | "handedoff" => Ok(EntryStatus::HandedOff),
            other => Err(format!(
                "unknown status '{other}' (expected completed, failed, cancelled or handed_off)"
            )),
        }
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
    /// Protocol used (`http`, `ftp`, `sftp`, `webdav`, `torrent`, `video`…).
    #[serde(default)]
    pub protocol: Option<String>,
    /// Wall-clock duration of the download in milliseconds, if measured.
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// Average speed in bytes per second (completed downloads only).
    #[serde(default)]
    pub avg_speed: Option<u64>,
    /// Result of checksum verification: `Some(true)` verified,
    /// `Some(false)` mismatch, `None` not checked.
    #[serde(default)]
    pub checksum_ok: Option<bool>,
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
            protocol: Some(crate::protocol::detect(url).as_str().to_string()),
            duration_ms: None,
            avg_speed: None,
            checksum_ok: None,
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

    /// Record how long the download took: sets `duration_ms` and moves
    /// `created_at` back to when the download started.
    pub fn set_elapsed(&mut self, elapsed: Duration) {
        self.duration_ms = Some(elapsed.as_millis().min(u64::MAX as u128) as u64);
        self.created_at = unix_now().saturating_sub(elapsed.as_secs());
    }

    /// Lowercase host of the URL without a leading `www.` (`None` for magnet
    /// links and local files).
    pub fn domain(&self) -> Option<String> {
        url_domain(&self.url)
    }

    /// Lowercase extension of the filename, without the dot.
    pub fn extension(&self) -> Option<String> {
        Path::new(&self.filename)
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
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

/// Persistent download history backed by a SQLite database in the OS config dir.
///
/// `load()` takes an in-memory snapshot; the associated functions
/// ([`append`](Self::append), [`update`](Self::update),
/// [`query`](Self::query), …) work on the database directly and are safe to
/// call from several processes at once.
pub struct DownloadHistory {
    entries: Vec<HistoryEntry>,
    path: PathBuf,
}

impl DownloadHistory {
    /// Load history from disk.  Returns an empty history if the database
    /// does not exist or cannot be read.
    pub fn load() -> Self {
        let path = Self::default_path();
        let entries = HistoryStore::open(&path)
            .and_then(|s| s.all())
            .unwrap_or_default();
        Self { entries, path }
    }

    /// Path of the history database (`history.db` in the config dir).
    pub fn default_path() -> PathBuf {
        crate::config::config_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("history.db")
    }

    /// Read, modify and write the history in one exclusive transaction.
    ///
    /// Safe across processes. Entries removed from the vector are deleted,
    /// changed ones are updated in place and new ones are appended.
    pub fn update<F: FnOnce(&mut Vec<HistoryEntry>)>(f: F) -> Result<(), BoxError> {
        HistoryStore::open_default()?.update(f)
    }

    /// Finalise `entry` and append it to the history (cross-process safe).
    pub fn append(
        mut entry: HistoryEntry,
        status: EntryStatus,
        error: Option<String>,
    ) -> Result<(), BoxError> {
        finalise(&mut entry, status, error);
        HistoryStore::open_default()?.insert(&entry)
    }

    /// Replace the stored history with the current in-memory entries.
    ///
    /// Prefer [`DownloadHistory::append`] / [`DownloadHistory::update`] when
    /// other processes may write at the same time.
    pub fn save(&self) -> Result<(), BoxError> {
        HistoryStore::open(&self.path)?.replace_all(&self.entries)
    }

    /// Entries matching `query`, read straight from the database.
    pub fn query(query: &HistoryQuery) -> Result<Vec<HistoryEntry>, BoxError> {
        HistoryStore::open_default()?.query(query)
    }

    /// Number of entries matching `query` (ignores `limit`/`offset`).
    pub fn count(query: &HistoryQuery) -> Result<usize, BoxError> {
        HistoryStore::open_default()?.count(query)
    }

    /// Delete the entries matching `query`; returns how many were removed.
    pub fn delete(query: &HistoryQuery) -> Result<usize, BoxError> {
        HistoryStore::open_default()?.delete(query)
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

    /// Filesystem path of the history database (OS config dir).
    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

// ============================================================================
// Queries
// ============================================================================

/// Sort order for [`HistoryQuery`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HistorySort {
    /// Most recent first (default).
    #[default]
    Newest,
    /// Oldest first.
    Oldest,
    /// Largest files first.
    Largest,
    /// Smallest files first.
    Smallest,
    /// Filename, A–Z.
    Name,
    /// Highest average speed first.
    Fastest,
}

impl FromStr for HistorySort {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "newest" | "new" | "date" | "recent" => Ok(HistorySort::Newest),
            "oldest" | "old" => Ok(HistorySort::Oldest),
            "largest" | "size" | "big" => Ok(HistorySort::Largest),
            "smallest" | "small" => Ok(HistorySort::Smallest),
            "name" | "filename" => Ok(HistorySort::Name),
            "fastest" | "speed" => Ok(HistorySort::Fastest),
            other => Err(format!(
                "unknown sort '{other}' (expected newest, oldest, largest, smallest, name or fastest)"
            )),
        }
    }
}

/// Filters for [`DownloadHistory::query`] / [`HistoryStore::query`].
///
/// Every set field must match (AND); `statuses` and `extensions` match any
/// of their values (OR). The default query returns everything, newest first.
#[derive(Clone, Debug, Default)]
pub struct HistoryQuery {
    /// Allowed statuses (empty = any).
    pub statuses: Vec<EntryStatus>,
    /// Host name; also matches its subdomains (`example.com` matches
    /// `dl.example.com`). Case-insensitive.
    pub domain: Option<String>,
    /// Filename extensions without the dot (`iso`, `tar.gz`); empty = any.
    pub extensions: Vec<String>,
    /// Protocol name (`http`, `ftp`, `torrent`…).
    pub protocol: Option<String>,
    /// Only entries created at or after this Unix timestamp.
    pub since: Option<u64>,
    /// Only entries created before this Unix timestamp (exclusive).
    pub until: Option<u64>,
    /// Case-insensitive substring of the URL or filename.
    pub search: Option<String>,
    /// Result order.
    pub sort: HistorySort,
    /// Maximum number of results (`None` = all).
    pub limit: Option<usize>,
    /// Number of results to skip.
    pub offset: usize,
}

impl HistoryQuery {
    /// Query matching every entry, newest first.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an allowed status.
    pub fn status(mut self, status: EntryStatus) -> Self {
        if !self.statuses.contains(&status) {
            self.statuses.push(status);
        }
        self
    }

    /// Only entries from `domain` or its subdomains.
    pub fn domain(mut self, domain: &str) -> Self {
        self.domain = Some(domain.to_string());
        self
    }

    /// Add an allowed filename extension (`"iso"`, `".zip"`, `"tar.gz"`).
    pub fn extension(mut self, ext: &str) -> Self {
        let ext = ext.trim().trim_start_matches('.').to_lowercase();
        if !ext.is_empty() && !self.extensions.contains(&ext) {
            self.extensions.push(ext);
        }
        self
    }

    /// Add a file type: a category (`video`, `audio`, `image`, `archive`,
    /// `document`, `disk`, `package`) or comma-separated extensions.
    pub fn file_type(mut self, kind: &str) -> Self {
        for part in kind.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            match file_type_extensions(part) {
                Some(exts) => {
                    for e in exts {
                        self = self.extension(e);
                    }
                }
                None => self = self.extension(part),
            }
        }
        self
    }

    /// Only entries downloaded with `protocol`.
    pub fn protocol(mut self, protocol: &str) -> Self {
        self.protocol = Some(protocol.to_string());
        self
    }

    /// Only entries created at or after `unix_secs`.
    pub fn since(mut self, unix_secs: u64) -> Self {
        self.since = Some(unix_secs);
        self
    }

    /// Only entries created before `unix_secs`.
    pub fn until(mut self, unix_secs: u64) -> Self {
        self.until = Some(unix_secs);
        self
    }

    /// Case-insensitive text search on URL and filename.
    pub fn search(mut self, text: &str) -> Self {
        self.search = Some(text.to_string());
        self
    }

    /// Result order.
    pub fn sort(mut self, sort: HistorySort) -> Self {
        self.sort = sort;
        self
    }

    /// Maximum number of results.
    pub fn limit(mut self, n: usize) -> Self {
        self.limit = Some(n);
        self
    }

    /// Skip the first `n` results.
    pub fn offset(mut self, n: usize) -> Self {
        self.offset = n;
        self
    }

    /// `WHERE` clause (without the keyword; `1` when empty) and its parameters.
    fn where_clause(&self) -> (String, Vec<Value>) {
        let mut clauses: Vec<String> = Vec::new();
        let mut params: Vec<Value> = Vec::new();

        if !self.statuses.is_empty() {
            let marks = vec!["?"; self.statuses.len()].join(", ");
            clauses.push(format!("status IN ({marks})"));
            params.extend(
                self.statuses
                    .iter()
                    .map(|s| Value::Text(s.as_str().to_string())),
            );
        }
        if let Some(d) = self.domain.as_deref().map(normalise_domain)
            && !d.is_empty()
        {
            clauses.push(
                "(domain = ? OR (length(domain) > length(?) \
                 AND substr(domain, -length(?) - 1) = '.' || ?))"
                    .into(),
            );
            params.extend(std::iter::repeat_n(Value::Text(d), 4));
        }
        if !self.extensions.is_empty() {
            let parts: Vec<&str> = self
                .extensions
                .iter()
                .map(|_| "substr(lower(filename), -length(?)) = ?")
                .collect();
            clauses.push(format!("({})", parts.join(" OR ")));
            for e in &self.extensions {
                let suffix = format!(".{}", e.trim_start_matches('.').to_lowercase());
                params.push(Value::Text(suffix.clone()));
                params.push(Value::Text(suffix));
            }
        }
        if let Some(p) = self.protocol.as_deref().filter(|p| !p.trim().is_empty()) {
            clauses.push("protocol = ?".into());
            params.push(Value::Text(p.trim().to_lowercase()));
        }
        if let Some(t) = self.since {
            clauses.push("created_at >= ?".into());
            params.push(Value::Integer(to_i64(t)));
        }
        if let Some(t) = self.until {
            clauses.push("created_at < ?".into());
            params.push(Value::Integer(to_i64(t)));
        }
        if let Some(q) = self.search.as_deref().filter(|q| !q.trim().is_empty()) {
            clauses.push("(instr(lower(url), ?) > 0 OR instr(lower(filename), ?) > 0)".into());
            let q = q.trim().to_lowercase();
            params.push(Value::Text(q.clone()));
            params.push(Value::Text(q));
        }

        if clauses.is_empty() {
            ("1".into(), params)
        } else {
            (clauses.join(" AND "), params)
        }
    }

    fn order_clause(&self) -> &'static str {
        match self.sort {
            HistorySort::Newest => "created_at DESC, seq DESC",
            HistorySort::Oldest => "created_at ASC, seq ASC",
            HistorySort::Largest => "bytes_total IS NULL, bytes_total DESC, seq DESC",
            HistorySort::Smallest => "bytes_total IS NULL, bytes_total ASC, seq DESC",
            HistorySort::Name => "lower(filename) ASC, seq DESC",
            HistorySort::Fastest => "avg_speed IS NULL, avg_speed DESC, seq DESC",
        }
    }

    fn limit_clause(&self) -> String {
        match (self.limit, self.offset) {
            (None, 0) => String::new(),
            (limit, offset) => format!(
                " LIMIT {} OFFSET {}",
                limit.map(|l| l.min(i64::MAX as usize) as i64).unwrap_or(-1),
                offset.min(i64::MAX as usize)
            ),
        }
    }
}

/// Extensions belonging to a file type category, if `kind` names one.
pub fn file_type_extensions(kind: &str) -> Option<&'static [&'static str]> {
    Some(match kind.trim().to_ascii_lowercase().as_str() {
        "video" | "videos" => &[
            "mp4", "mkv", "webm", "avi", "mov", "m4v", "flv", "wmv", "mpg", "mpeg", "ts",
        ],
        "audio" | "music" => &[
            "mp3", "flac", "wav", "ogg", "opus", "m4a", "aac", "wma", "alac",
        ],
        "image" | "images" | "picture" => &[
            "jpg", "jpeg", "png", "gif", "webp", "svg", "bmp", "tiff", "heic", "avif",
        ],
        "archive" | "archives" | "compressed" => &[
            "zip", "tar", "gz", "tgz", "bz2", "xz", "zst", "7z", "rar", "lz4",
        ],
        "document" | "documents" | "doc" | "docs" => &[
            "pdf", "doc", "docx", "odt", "rtf", "txt", "md", "epub", "xls", "xlsx", "ods", "csv",
            "ppt", "pptx", "odp",
        ],
        "disk" | "iso" | "image-disk" => &["iso", "img", "dmg", "vhd", "vmdk", "qcow2"],
        "package" | "packages" | "app" | "installer" => &[
            "exe", "msi", "deb", "rpm", "apk", "pkg", "appimage", "flatpak", "snap",
        ],
        _ => return None,
    })
}

/// Parse a date bound for history filters (interpreted as UTC).
///
/// Accepts `YYYY-MM-DD`, `YYYY-MM-DD HH:MM[:SS]` (or with `T`), a Unix
/// timestamp, `today`, `yesterday`, or a relative age such as `30m`, `12h`,
/// `7d` or `2w` (that long ago). With `end_of_day`, a bare date means the
/// start of the *next* day, so `--until 2026-10-01` includes that whole day.
///
/// ```rust
/// use kget::queue::parse_date_bound;
///
/// assert_eq!(parse_date_bound("1970-01-02", false).unwrap(), 86_400);
/// assert_eq!(parse_date_bound("1970-01-02", true).unwrap(), 2 * 86_400);
/// assert_eq!(parse_date_bound("1970-01-01 01:30", false).unwrap(), 5_400);
/// ```
pub fn parse_date_bound(s: &str, end_of_day: bool) -> Result<u64, String> {
    use chrono::{NaiveDate, NaiveDateTime};
    let t = s.trim();
    let now = unix_now();
    let today = now - now % 86_400;
    let day_bound = |start: u64| if end_of_day { start + 86_400 } else { start };

    match t.to_ascii_lowercase().as_str() {
        "today" => return Ok(day_bound(today)),
        "yesterday" => return Ok(day_bound(today.saturating_sub(86_400))),
        "now" => return Ok(now),
        _ => {}
    }
    if let Some((num, unit)) = t
        .char_indices()
        .last()
        .map(|(i, c)| (&t[..i], c.to_ascii_lowercase()))
        && !num.is_empty()
        && let Ok(n) = num.parse::<u64>()
    {
        let secs = match unit {
            'm' => Some(60),
            'h' => Some(3_600),
            'd' => Some(86_400),
            'w' => Some(7 * 86_400),
            _ => None,
        };
        if let Some(secs) = secs {
            return Ok(now.saturating_sub(n.saturating_mul(secs)));
        }
    }
    if !t.is_empty() && t.chars().all(|c| c.is_ascii_digit()) {
        return t.parse::<u64>().map_err(|e| e.to_string());
    }
    if let Ok(d) = NaiveDate::parse_from_str(t, "%Y-%m-%d") {
        let start = d
            .and_hms_opt(0, 0, 0)
            .map(|dt| dt.and_utc().timestamp())
            .unwrap_or(0)
            .max(0) as u64;
        return Ok(day_bound(start));
    }
    for fmt in [
        "%Y-%m-%d %H:%M",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%dT%H:%M:%S",
    ] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(t.trim_end_matches('Z'), fmt) {
            return Ok(dt.and_utc().timestamp().max(0) as u64);
        }
    }
    Err(format!(
        "invalid date '{t}' (use YYYY-MM-DD, 'YYYY-MM-DD HH:MM', today, yesterday or 7d/12h)"
    ))
}

// ============================================================================
// SQLite store
// ============================================================================

/// Schema migrations; entry `n` upgrades `user_version` `n` → `n + 1`.
/// Only ever append: released migrations must never change.
const MIGRATIONS: &[&str] = &[
    // v1: initial schema (KGet 1.8)
    "CREATE TABLE IF NOT EXISTS history (
        seq             INTEGER PRIMARY KEY,
        id              TEXT NOT NULL UNIQUE,
        url             TEXT NOT NULL,
        filename        TEXT NOT NULL,
        output_dir      TEXT NOT NULL,
        status          TEXT NOT NULL,
        domain          TEXT NOT NULL DEFAULT '',
        protocol        TEXT,
        bytes_total     INTEGER,
        sha256          TEXT,
        expected_sha256 TEXT,
        checksum_ok     INTEGER,
        created_at      INTEGER NOT NULL,
        finished_at     INTEGER,
        duration_ms     INTEGER,
        avg_speed       INTEGER,
        error           TEXT
    );
    CREATE INDEX IF NOT EXISTS history_created ON history(created_at);
    CREATE INDEX IF NOT EXISTS history_status ON history(status, created_at);
    CREATE INDEX IF NOT EXISTS history_domain ON history(domain);",
];

const COLUMNS: &str = "id, url, filename, output_dir, status, bytes_total, sha256, \
     expected_sha256, created_at, finished_at, error, protocol, duration_ms, avg_speed, \
     checksum_ok";

/// Direct handle on a history database file.
///
/// [`DownloadHistory`] uses the default database in the config dir; open a
/// store explicitly to work with another file (tests, tools).
pub struct HistoryStore {
    conn: Connection,
    path: PathBuf,
}

impl HistoryStore {
    /// Open (creating and migrating if needed) the database at `path`.
    ///
    /// A `history.json` next to it is imported once and renamed to
    /// `history.json.bak`. A file that is not a valid database is moved to
    /// `<name>.corrupt` and a fresh one is created.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, BoxError> {
        let path = path.as_ref().to_path_buf();
        if let Some(p) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(p)?;
        }
        match Self::open_retrying(&path) {
            Err(e) if is_corruption(&e) => {
                quarantine(&path);
                Ok(Self::open_retrying(&path)?)
            }
            r => Ok(r?),
        }
    }

    /// SQLite reports `SQLITE_BUSY` without calling the busy handler while
    /// another connection creates the file or switches it to WAL, so retry
    /// opening for up to [`BUSY_TIMEOUT`].
    fn open_retrying(path: &Path) -> rusqlite::Result<Self> {
        let start = std::time::Instant::now();
        let mut delay = Duration::from_millis(5);
        loop {
            match Self::open_inner(path) {
                Err(e)
                    if matches!(
                        e.sqlite_error_code(),
                        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
                    ) && start.elapsed() < BUSY_TIMEOUT =>
                {
                    std::thread::sleep(delay);
                    delay = (delay * 2).min(Duration::from_millis(100));
                }
                r => return r,
            }
        }
    }

    /// Open the default database (`history.db` in the config dir).
    pub fn open_default() -> Result<Self, BoxError> {
        Self::open(DownloadHistory::default_path())
    }

    fn open_inner(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        // WAL is persistent: only switch once (switching needs an exclusive lock).
        let mode: String = conn.pragma_query_value(None, "journal_mode", |r| r.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            conn.pragma_update(None, "journal_mode", "WAL")?;
        }
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        restrict_permissions(path);
        let mut store = Self {
            conn,
            path: path.to_path_buf(),
        };
        store.migrate()?;
        store.import_legacy_json()?;
        Ok(store)
    }

    /// Bring the schema up to [`SCHEMA_VERSION`].
    fn migrate(&mut self) -> rusqlite::Result<()> {
        let version: i32 = self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version >= SCHEMA_VERSION {
            // Up to date, or created by a newer KGet: migrations only add,
            // so the columns used here still exist.
            return Ok(());
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Re-check under the write lock: another process may have migrated.
        let version: i32 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
        for sql in MIGRATIONS.iter().skip(version.max(0) as usize) {
            tx.execute_batch(sql)?;
        }
        if version < SCHEMA_VERSION {
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        tx.commit()
    }

    /// Import `history.json` from KGet ≤ 1.7 (also covers an old binary
    /// writing it again after the upgrade); duplicates are skipped by id.
    fn import_legacy_json(&mut self) -> rusqlite::Result<()> {
        let json = self.path.with_file_name("history.json");
        if !json.is_file() {
            return Ok(());
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Read under the write lock so concurrent processes import once.
        let content = match std::fs::read_to_string(&json) {
            Ok(c) => c,
            Err(_) => return Ok(()),
        };
        let entries: Vec<HistoryEntry> = if content.trim().is_empty() {
            Vec::new()
        } else {
            match serde_json::from_str(&content) {
                Ok(v) => v,
                Err(_) => {
                    let _ = std::fs::rename(&json, json.with_extension("json.corrupt"));
                    return Ok(());
                }
            }
        };
        for mut e in entries {
            // Older versions could have stored credentials; never keep them.
            e.url = redact_url(&e.url);
            insert_row(&tx, &e, true)?;
        }
        prune(&tx)?;
        tx.commit()?;
        let _ = std::fs::rename(&json, json.with_extension("json.bak"));
        let _ = std::fs::remove_file(json.with_extension("lock"));
        Ok(())
    }

    /// Path of the database file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append an entry (as given; see [`DownloadHistory::append`] to
    /// finalise it first). An entry with an existing id replaces it.
    pub fn insert(&mut self, entry: &HistoryEntry) -> Result<(), BoxError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        insert_row(&tx, entry, false)?;
        prune(&tx)?;
        tx.commit()?;
        Ok(())
    }

    /// Every entry in insertion order (oldest first).
    pub fn all(&self) -> Result<Vec<HistoryEntry>, BoxError> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {COLUMNS} FROM history ORDER BY seq"))?;
        let rows = stmt.query_map([], row_to_entry)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Entries matching `query`.
    pub fn query(&self, query: &HistoryQuery) -> Result<Vec<HistoryEntry>, BoxError> {
        let (filter, params) = query.where_clause();
        let sql = format!(
            "SELECT {COLUMNS} FROM history WHERE {filter} ORDER BY {}{}",
            query.order_clause(),
            query.limit_clause()
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(params), row_to_entry)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Number of entries matching `query` (ignores sort, `limit` and `offset`).
    pub fn count(&self, query: &HistoryQuery) -> Result<usize, BoxError> {
        let (filter, params) = query.where_clause();
        let n: i64 = self.conn.query_row(
            &format!("SELECT COUNT(*) FROM history WHERE {filter}"),
            rusqlite::params_from_iter(params),
            |r| r.get(0),
        )?;
        Ok(n.max(0) as usize)
    }

    /// Entry whose id is `id` or starts with it (newest wins).
    pub fn get(&self, id: &str) -> Result<Option<HistoryEntry>, BoxError> {
        let pattern = format!("{}%", id.replace(['%', '_', '\\'], ""));
        Ok(self
            .conn
            .query_row(
                &format!(
                    "SELECT {COLUMNS} FROM history WHERE id LIKE ?1 \
                     ORDER BY created_at DESC, seq DESC LIMIT 1"
                ),
                [pattern],
                row_to_entry,
            )
            .optional()?)
    }

    /// Delete the entries matching `query` (honouring sort/`limit`/`offset`);
    /// returns how many were removed.
    pub fn delete(&mut self, query: &HistoryQuery) -> Result<usize, BoxError> {
        let (filter, params) = query.where_clause();
        let sql = if query.limit.is_some() || query.offset > 0 {
            format!(
                "DELETE FROM history WHERE seq IN (SELECT seq FROM history WHERE {filter} \
                 ORDER BY {}{})",
                query.order_clause(),
                query.limit_clause()
            )
        } else {
            format!("DELETE FROM history WHERE {filter}")
        };
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let n = tx.execute(&sql, rusqlite::params_from_iter(params))?;
        tx.commit()?;
        Ok(n)
    }

    /// Read, modify and write every entry in one exclusive transaction.
    ///
    /// Entries removed from the vector are deleted, changed ones are updated
    /// in place (keeping their position) and new ones are appended.
    pub fn update<F: FnOnce(&mut Vec<HistoryEntry>)>(&mut self, f: F) -> Result<(), BoxError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut entries = {
            let mut stmt = tx.prepare(&format!("SELECT {COLUMNS} FROM history ORDER BY seq"))?;
            let rows = stmt.query_map([], row_to_entry)?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let before: HashSet<String> = entries.iter().map(|e| e.id.clone()).collect();
        f(&mut entries);
        let after: HashSet<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        {
            let mut del = tx.prepare("DELETE FROM history WHERE id = ?1")?;
            for id in before.iter().filter(|id| !after.contains(id.as_str())) {
                del.execute([id])?;
            }
        }
        for e in &entries {
            insert_row(&tx, e, false)?;
        }
        prune(&tx)?;
        tx.commit()?;
        Ok(())
    }

    /// Replace the whole history with `entries` (kept in this order).
    pub fn replace_all(&mut self, entries: &[HistoryEntry]) -> Result<(), BoxError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM history", [])?;
        for e in entries {
            insert_row(&tx, e, false)?;
        }
        prune(&tx)?;
        tx.commit()?;
        Ok(())
    }
}

// ============================================================================
// Internal helpers
// ============================================================================

fn finalise(entry: &mut HistoryEntry, status: EntryStatus, error: Option<String>) {
    entry.status = status;
    entry.finished_at = Some(unix_now());
    entry.error = error;
    if entry.avg_speed.is_none()
        && entry.status == EntryStatus::Completed
        && let (Some(bytes), Some(ms)) = (entry.bytes_total, entry.duration_ms)
        && ms > 0
    {
        entry.avg_speed = Some((bytes as u128 * 1000 / ms as u128).min(u64::MAX as u128) as u64);
    }
}

/// Insert `e`; on an id clash, update it in place (or keep the stored row
/// when `ignore_existing`).
fn insert_row(conn: &Connection, e: &HistoryEntry, ignore_existing: bool) -> rusqlite::Result<()> {
    let conflict = if ignore_existing {
        "DO NOTHING"
    } else {
        "DO UPDATE SET url = excluded.url, filename = excluded.filename, \
         output_dir = excluded.output_dir, status = excluded.status, domain = excluded.domain, \
         protocol = excluded.protocol, bytes_total = excluded.bytes_total, \
         sha256 = excluded.sha256, expected_sha256 = excluded.expected_sha256, \
         checksum_ok = excluded.checksum_ok, created_at = excluded.created_at, \
         finished_at = excluded.finished_at, duration_ms = excluded.duration_ms, \
         avg_speed = excluded.avg_speed, error = excluded.error"
    };
    conn.prepare_cached(&format!(
        "INSERT INTO history ({COLUMNS}, domain) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16) \
         ON CONFLICT(id) {conflict}"
    ))?
    .execute(params![
        e.id,
        redact_url(&e.url),
        e.filename,
        e.output_dir,
        e.status.as_str(),
        e.bytes_total.map(to_i64),
        e.sha256,
        e.expected_sha256,
        to_i64(e.created_at),
        e.finished_at.map(to_i64),
        e.error,
        e.protocol,
        e.duration_ms.map(to_i64),
        e.avg_speed.map(to_i64),
        e.checksum_ok,
        e.domain().unwrap_or_default(),
    ])?;
    Ok(())
}

fn row_to_entry(r: &Row<'_>) -> rusqlite::Result<HistoryEntry> {
    let u = |v: Option<i64>| v.map(|n| n.max(0) as u64);
    let status: String = r.get(4)?;
    Ok(HistoryEntry {
        id: r.get(0)?,
        url: r.get(1)?,
        filename: r.get(2)?,
        output_dir: r.get(3)?,
        status: status.parse().unwrap_or(EntryStatus::Failed),
        bytes_total: u(r.get(5)?),
        sha256: r.get(6)?,
        expected_sha256: r.get(7)?,
        created_at: r.get::<_, i64>(8)?.max(0) as u64,
        finished_at: u(r.get(9)?),
        error: r.get(10)?,
        protocol: r.get(11)?,
        duration_ms: u(r.get(12)?),
        avg_speed: u(r.get(13)?),
        checksum_ok: r.get(14)?,
    })
}

/// Keep only the newest [`MAX_ENTRIES`] rows.
fn prune(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM history WHERE seq <= \
         (SELECT seq FROM history ORDER BY seq DESC LIMIT 1 OFFSET ?1)",
        [MAX_ENTRIES as i64],
    )?;
    Ok(())
}

fn is_corruption(e: &rusqlite::Error) -> bool {
    matches!(
        e.sqlite_error_code(),
        Some(ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt)
    )
}

/// Move an unreadable database (and its WAL files) aside.
fn quarantine(path: &Path) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "history.db".into());
    let _ = std::fs::rename(path, path.with_file_name(format!("{name}.corrupt")));
    for suffix in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(path.with_file_name(format!("{name}{suffix}")));
    }
}

/// History may hold signed URLs: keep it private to the user.
fn restrict_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = path;
}

fn url_domain(raw: &str) -> Option<String> {
    url::Url::parse(raw)
        .ok()?
        .host_str()
        .map(|h| h.trim_start_matches("www.").to_lowercase())
        .filter(|h| !h.is_empty())
}

/// `https://www.Example.com/x` / `Example.COM.` → `example.com`.
fn normalise_domain(s: &str) -> String {
    let s = s.trim();
    let host = url_domain(s).unwrap_or_else(|| s.to_string());
    host.trim_end_matches('.')
        .trim_start_matches("www.")
        .to_lowercase()
}

fn to_i64(v: u64) -> i64 {
    v.min(i64::MAX as u64) as i64
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
