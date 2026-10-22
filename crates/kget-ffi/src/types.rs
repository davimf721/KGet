//! FFI records, enums and errors, and their mapping from/to the core types.
//!
//! Nothing here is shared with the `kget` crate on purpose: the core can
//! rename or extend its types freely, and only this mapping layer has to
//! follow. Adding a field to a record or a variant to an enum is a breaking
//! change for Swift/Kotlin callers, so keep these types small and stable.

use kget::checksum::ChecksumAlgorithm;
use kget::events::{JsonEvent, Phase};
use kget::jobs::{DownloadState, JobError, JobEvent, JobInfo, JobProgress};
use kget::queue::{EntryStatus, HistoryEntry, HistoryQuery, HistorySort};
use kget::runner::{self, DownloadJob};
use kget::ytdlp::VideoQuality;
use kget::{OverwritePolicy, Protocol};
use std::fmt;

// ============================================================================
// Errors
// ============================================================================

/// Error returned by [`KgetEngine`](crate::KgetEngine) methods.
#[derive(Debug, Clone, PartialEq, uniffi::Error)]
pub enum EngineError {
    /// No job with this id
    NotFound { id: u64 },
    /// The operation is not allowed in the job's current state
    InvalidState {
        id: u64,
        state: JobState,
        action: String,
    },
    /// The download request is malformed (empty URL, bad checksum…)
    InvalidRequest { message: String },
    /// The config directory or `config.json` cannot be used
    Config { message: String },
    /// The download history cannot be read
    History { message: String },
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::NotFound { id } => write!(f, "No job with id {id}"),
            EngineError::InvalidState { id, state, action } => {
                write!(f, "Cannot {action} job {id}: it is {}", state.as_core())
            }
            EngineError::InvalidRequest { message } => write!(f, "Invalid request: {message}"),
            EngineError::Config { message } => write!(f, "Configuration error: {message}"),
            EngineError::History { message } => write!(f, "History error: {message}"),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<JobError> for EngineError {
    fn from(e: JobError) -> Self {
        match e {
            JobError::NotFound(id) => EngineError::NotFound { id },
            JobError::InvalidState { id, state, action } => EngineError::InvalidState {
                id,
                state: state.into(),
                action: action.to_string(),
            },
        }
    }
}

// ============================================================================
// Enums
// ============================================================================

/// Lifecycle state of a job (mirrors `kget::DownloadState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum JobState {
    /// Waiting for a free slot
    Queued,
    /// Transferring data
    Running,
    /// Checking the checksum of the downloaded file
    Verifying,
    /// Paused; partial data is kept
    Paused,
    /// Cancelled; partial data is kept
    Cancelled,
    /// Finished successfully
    Completed,
    /// Finished with an error
    Failed,
}

impl JobState {
    fn as_core(self) -> DownloadState {
        match self {
            JobState::Queued => DownloadState::Queued,
            JobState::Running => DownloadState::Running,
            JobState::Verifying => DownloadState::Verifying,
            JobState::Paused => DownloadState::Paused,
            JobState::Cancelled => DownloadState::Cancelled,
            JobState::Completed => DownloadState::Completed,
            JobState::Failed => DownloadState::Failed,
        }
    }
}

impl From<DownloadState> for JobState {
    fn from(s: DownloadState) -> Self {
        match s {
            DownloadState::Queued => JobState::Queued,
            DownloadState::Running => JobState::Running,
            DownloadState::Verifying => JobState::Verifying,
            DownloadState::Paused => JobState::Paused,
            DownloadState::Cancelled => JobState::Cancelled,
            DownloadState::Completed => JobState::Completed,
            DownloadState::Failed => JobState::Failed,
        }
    }
}

/// `true` when the job is not queued or running (it can be resumed or
/// removed).
#[uniffi::export]
pub fn job_state_is_finished(state: JobState) -> bool {
    state.as_core().is_finished()
}

/// Download protocol (mirrors `kget::Protocol`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum DownloadProtocol {
    Http,
    Ftp,
    Sftp,
    WebDav,
    Torrent,
    Metalink,
    Video,
}

impl From<Protocol> for DownloadProtocol {
    fn from(p: Protocol) -> Self {
        match p {
            Protocol::Http => DownloadProtocol::Http,
            Protocol::Ftp => DownloadProtocol::Ftp,
            Protocol::Sftp => DownloadProtocol::Sftp,
            Protocol::WebDav => DownloadProtocol::WebDav,
            Protocol::Torrent => DownloadProtocol::Torrent,
            Protocol::Metalink => DownloadProtocol::Metalink,
            Protocol::Video => DownloadProtocol::Video,
        }
    }
}

impl From<DownloadProtocol> for Protocol {
    fn from(p: DownloadProtocol) -> Self {
        match p {
            DownloadProtocol::Http => Protocol::Http,
            DownloadProtocol::Ftp => Protocol::Ftp,
            DownloadProtocol::Sftp => Protocol::Sftp,
            DownloadProtocol::WebDav => Protocol::WebDav,
            DownloadProtocol::Torrent => Protocol::Torrent,
            DownloadProtocol::Metalink => Protocol::Metalink,
            DownloadProtocol::Video => Protocol::Video,
        }
    }
}

/// What to do when the destination file already exists.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, uniffi::Enum)]
pub enum OverwriteMode {
    /// Replace the existing file
    #[default]
    Overwrite,
    /// Keep the existing file and skip the download
    Skip,
    /// Save under a new name: `file (1).zip`…
    Rename,
}

impl From<OverwriteMode> for OverwritePolicy {
    fn from(m: OverwriteMode) -> Self {
        match m {
            OverwriteMode::Overwrite => OverwritePolicy::Overwrite,
            OverwriteMode::Skip => OverwritePolicy::Skip,
            OverwriteMode::Rename => OverwritePolicy::Rename,
        }
    }
}

/// Post-transfer phase announced by [`JobEventRecord::Phase`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum JobPhase {
    /// A checksum of the downloaded file is being computed
    Verifying,
    /// The downloaded archive is being unpacked
    Extracting,
}

impl From<Phase> for JobPhase {
    fn from(p: Phase) -> Self {
        match p {
            Phase::Verifying => JobPhase::Verifying,
            Phase::Extracting => JobPhase::Extracting,
        }
    }
}

// ============================================================================
// Requests and snapshots
// ============================================================================

/// One extra HTTP header.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct HttpHeader {
    pub name: String,
    pub value: String,
}

/// What to download. Only `url` is required; every other field has a
/// default in the generated bindings.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DownloadRequest {
    /// URL, magnet link or path of a `.torrent` / `.meta4` file
    pub url: String,
    /// Output file or directory. Native apps should always set it: the
    /// default is the process's current directory (`/` on iOS).
    #[uniffi(default = None)]
    pub output: Option<String>,
    /// Force a protocol instead of detecting it from the URL
    #[uniffi(default = None)]
    pub protocol: Option<DownloadProtocol>,
    /// Parallel, resumable HTTP download (turbo mode)
    #[uniffi(default = false)]
    pub advanced: bool,
    /// Expected SHA-256 (hex)
    #[uniffi(default = None)]
    pub sha256: Option<String>,
    /// Expected checksum as `<algorithm>:<hex>` (`sha256:…`, `md5:…`…)
    #[uniffi(default = None)]
    pub checksum: Option<String>,
    /// URL of a checksum file (`SHA256SUMS`, `file.iso.sha256`…)
    #[uniffi(default = None)]
    pub checksum_url: Option<String>,
    /// Extra HTTP headers
    #[uniffi(default = [])]
    pub headers: Vec<HttpHeader>,
    /// Continue an existing `<file>.part`
    #[uniffi(default = false)]
    pub resume: bool,
    /// Behaviour when the destination already exists (`None` = overwrite)
    #[uniffi(default = None)]
    pub overwrite: Option<OverwriteMode>,
    /// Maximum attempts per request (`None` = KGet default)
    #[uniffi(default = None)]
    pub retries: Option<u32>,
    /// Connect / per-read timeout in seconds (`None` = KGet default)
    #[uniffi(default = None)]
    pub timeout_secs: Option<u64>,
    /// Extract the archive after a successful download
    #[uniffi(default = false)]
    pub extract: bool,
    /// yt-dlp quality preset (`best`, `1080p`, `audio`…; `None` = best)
    #[uniffi(default = None)]
    pub video_quality: Option<String>,
}

impl DownloadRequest {
    /// Validate the request and build the core job.
    pub(crate) fn into_job(self) -> Result<DownloadJob, EngineError> {
        let invalid = |message: String| EngineError::InvalidRequest { message };
        let url = self.url.trim().to_string();
        if url.is_empty() {
            return Err(invalid("the URL is empty".into()));
        }
        let checksum: Option<(ChecksumAlgorithm, String)> = self
            .checksum
            .as_deref()
            .map(runner::parse_checksum_spec)
            .transpose()
            .map_err(|e| invalid(e.to_string()))?;
        let mut job = DownloadJob::new(url);
        job.output = self.output.filter(|o| !o.trim().is_empty());
        job.protocol = self.protocol.map(Into::into);
        job.advanced = self.advanced;
        job.sha256 = self.sha256.filter(|s| !s.trim().is_empty());
        job.checksum = checksum;
        job.checksum_url = self.checksum_url.filter(|s| !s.trim().is_empty());
        job.headers = self
            .headers
            .into_iter()
            .map(|h| (h.name, h.value))
            .collect();
        job.resume = self.resume;
        job.overwrite = self.overwrite.unwrap_or_default().into();
        if let Some(retries) = self.retries {
            job.retries = retries.max(1);
        }
        if let Some(timeout) = self.timeout_secs {
            job.timeout = timeout;
        }
        job.extract = self.extract;
        if let Some(q) = self.video_quality.filter(|q| !q.trim().is_empty()) {
            job.quality = VideoQuality::parse(q.trim());
        }
        Ok(job)
    }
}

/// Latest progress of a job.
#[derive(Debug, Clone, Default, PartialEq, uniffi::Record)]
pub struct JobProgressRecord {
    /// Bytes downloaded
    pub bytes: u64,
    /// Total size, when known
    pub total: Option<u64>,
    /// 0.0 – 100.0, when known
    pub percent: Option<f64>,
    /// Current speed in bytes per second
    pub speed_bps: u64,
    /// Estimated seconds remaining
    pub eta_secs: Option<u64>,
}

impl From<JobProgress> for JobProgressRecord {
    fn from(p: JobProgress) -> Self {
        Self {
            bytes: p.bytes,
            total: p.total,
            percent: p.percent,
            speed_bps: p.speed_bps,
            eta_secs: p.eta_secs,
        }
    }
}

/// Snapshot of a job, returned by `list()` and `get()`.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct JobSnapshot {
    pub id: u64,
    /// Requested URL
    pub url: String,
    /// Requested output (file or directory), if any
    pub output: Option<String>,
    /// Protocol used (forced or detected)
    pub protocol: DownloadProtocol,
    pub state: JobState,
    /// Unix timestamps (seconds)
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
    pub progress: JobProgressRecord,
    /// Where the data was saved (single-file downloads)
    pub path: Option<String>,
    /// Error message of a failed job
    pub error: Option<String>,
    /// The data was handed to an external application (system torrent client)
    pub handed_off: bool,
}

impl From<JobInfo> for JobSnapshot {
    fn from(info: JobInfo) -> Self {
        Self {
            id: info.id,
            url: info.job.url,
            output: info.job.output,
            protocol: info.protocol.into(),
            state: info.state.into(),
            created_at: info.created_at,
            started_at: info.started_at,
            finished_at: info.finished_at,
            progress: info.progress.into(),
            path: info.path,
            error: info.error,
            handed_off: info.handed_off,
        }
    }
}

// ============================================================================
// Events
// ============================================================================

/// One file inside a torrent.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TorrentFile {
    /// Path relative to the torrent root
    pub name: String,
    pub size: u64,
}

/// Progress of one file inside a torrent.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TorrentFileProgress {
    /// Index in the `Files` event
    pub index: u64,
    pub downloaded: u64,
    pub size: u64,
    /// 0.0 – 100.0
    pub percent: f64,
}

/// Event delivered to a [`KgetEventListener`](crate::KgetEventListener).
///
/// `StateChanged` drives the job lifecycle; the other variants are the
/// download events of the JSONL protocol (`docs/ARCHITECTURE.md`).
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum JobEventRecord {
    /// The job changed state; `error` is set for `Failed`
    StateChanged {
        id: u64,
        state: JobState,
        error: Option<String>,
    },
    /// The transfer started
    Started {
        id: u64,
        url: String,
        protocol: String,
        output: String,
    },
    /// Remote metadata became known
    Meta {
        id: u64,
        total: Option<u64>,
        filename: String,
        resumable: bool,
    },
    /// Periodic progress (about 5 per second)
    Progress {
        id: u64,
        progress: JobProgressRecord,
    },
    /// Human-readable status message (for logs only; never match on it)
    Status { id: u64, message: String },
    /// Files contained in a torrent
    Files { id: u64, files: Vec<TorrentFile> },
    /// Per-file progress of a torrent
    FileProgress {
        id: u64,
        files: Vec<TorrentFileProgress>,
    },
    /// A post-transfer phase started
    Phase {
        id: u64,
        phase: JobPhase,
        algorithm: Option<String>,
    },
    /// Result of a checksum verification
    Verified {
        id: u64,
        algorithm: String,
        hash: String,
        ok: bool,
    },
    /// The file was written to its final location
    Saved { id: u64, path: String, bytes: u64 },
    /// The transfer finished successfully
    Completed { id: u64 },
    /// The transfer failed
    Error {
        id: u64,
        code: String,
        message: String,
        retryable: bool,
    },
    /// A finished job was removed
    Removed { id: u64 },
}

impl JobEventRecord {
    /// The job this event belongs to.
    pub fn id(&self) -> u64 {
        match self {
            JobEventRecord::StateChanged { id, .. }
            | JobEventRecord::Started { id, .. }
            | JobEventRecord::Meta { id, .. }
            | JobEventRecord::Progress { id, .. }
            | JobEventRecord::Status { id, .. }
            | JobEventRecord::Files { id, .. }
            | JobEventRecord::FileProgress { id, .. }
            | JobEventRecord::Phase { id, .. }
            | JobEventRecord::Verified { id, .. }
            | JobEventRecord::Saved { id, .. }
            | JobEventRecord::Completed { id }
            | JobEventRecord::Error { id, .. }
            | JobEventRecord::Removed { id } => *id,
        }
    }
}

impl From<JobEvent> for JobEventRecord {
    fn from(event: JobEvent) -> Self {
        match event {
            JobEvent::State { id, state, error } => JobEventRecord::StateChanged {
                id,
                state: state.into(),
                error,
            },
            JobEvent::Removed { id } => JobEventRecord::Removed { id },
            JobEvent::Download { id, event } => download_event(id, event),
        }
    }
}

fn download_event(id: u64, event: JsonEvent) -> JobEventRecord {
    match event {
        JsonEvent::Started {
            url,
            protocol,
            output,
            ..
        } => JobEventRecord::Started {
            id,
            url,
            protocol,
            output,
        },
        JsonEvent::Meta {
            total,
            filename,
            resumable,
        } => JobEventRecord::Meta {
            id,
            total,
            filename,
            resumable,
        },
        JsonEvent::Progress {
            bytes,
            total,
            percent,
            speed_bps,
            eta_secs,
            ..
        } => JobEventRecord::Progress {
            id,
            progress: JobProgressRecord {
                bytes,
                total,
                percent,
                speed_bps,
                eta_secs,
            },
        },
        JsonEvent::Status { message } => JobEventRecord::Status { id, message },
        JsonEvent::Files { files } => JobEventRecord::Files {
            id,
            files: files
                .into_iter()
                .map(|f| TorrentFile {
                    name: f.name,
                    size: f.size,
                })
                .collect(),
        },
        JsonEvent::FileProgress { files } => JobEventRecord::FileProgress {
            id,
            files: files
                .into_iter()
                .map(|f| TorrentFileProgress {
                    index: f.idx as u64,
                    downloaded: f.downloaded,
                    size: f.size,
                    percent: f.pct,
                })
                .collect(),
        },
        JsonEvent::Phase { phase, algorithm } => JobEventRecord::Phase {
            id,
            phase: phase.into(),
            algorithm,
        },
        JsonEvent::Verified {
            algorithm,
            hash,
            ok,
        } => JobEventRecord::Verified {
            id,
            algorithm,
            hash,
            ok,
        },
        JsonEvent::Saved { path, bytes } => JobEventRecord::Saved { id, path, bytes },
        JsonEvent::Completed => JobEventRecord::Completed { id },
        JsonEvent::Error {
            code,
            message,
            retryable,
        } => JobEventRecord::Error {
            id,
            code,
            message,
            retryable,
        },
    }
}

// ============================================================================
// History
// ============================================================================

/// Final outcome of a download recorded in the history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum HistoryStatus {
    Completed,
    Failed,
    Cancelled,
    /// Handed to an external application (system torrent client)
    HandedOff,
}

impl From<HistoryStatus> for EntryStatus {
    fn from(s: HistoryStatus) -> Self {
        match s {
            HistoryStatus::Completed => EntryStatus::Completed,
            HistoryStatus::Failed => EntryStatus::Failed,
            HistoryStatus::Cancelled => EntryStatus::Cancelled,
            HistoryStatus::HandedOff => EntryStatus::HandedOff,
        }
    }
}

impl From<EntryStatus> for HistoryStatus {
    fn from(s: EntryStatus) -> Self {
        match s {
            EntryStatus::Completed => HistoryStatus::Completed,
            EntryStatus::Failed => HistoryStatus::Failed,
            EntryStatus::Cancelled => HistoryStatus::Cancelled,
            EntryStatus::HandedOff => HistoryStatus::HandedOff,
        }
    }
}

/// Order of history results.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, uniffi::Enum)]
pub enum HistoryOrder {
    /// Most recent first
    #[default]
    Newest,
    Oldest,
    /// Largest files first
    Largest,
    Smallest,
    /// Filename, A–Z
    Name,
    /// Highest average speed first
    Fastest,
}

impl From<HistoryOrder> for HistorySort {
    fn from(o: HistoryOrder) -> Self {
        match o {
            HistoryOrder::Newest => HistorySort::Newest,
            HistoryOrder::Oldest => HistorySort::Oldest,
            HistoryOrder::Largest => HistorySort::Largest,
            HistoryOrder::Smallest => HistorySort::Smallest,
            HistoryOrder::Name => HistorySort::Name,
            HistoryOrder::Fastest => HistorySort::Fastest,
        }
    }
}

/// Filters for `history()`. Every set field must match; an empty filter
/// returns everything, newest first.
#[derive(Debug, Clone, Default, PartialEq, uniffi::Record)]
pub struct HistoryFilter {
    /// Allowed statuses (empty = any)
    #[uniffi(default = [])]
    pub statuses: Vec<HistoryStatus>,
    /// Host name; also matches its subdomains
    #[uniffi(default = None)]
    pub domain: Option<String>,
    /// Category (`video`, `audio`, `image`, `archive`, `document`, `disk`,
    /// `package`) or comma-separated extensions
    #[uniffi(default = None)]
    pub file_type: Option<String>,
    /// Protocol name (`http`, `ftp`, `torrent`…)
    #[uniffi(default = None)]
    pub protocol: Option<String>,
    /// Only entries created at or after this Unix timestamp
    #[uniffi(default = None)]
    pub since: Option<u64>,
    /// Only entries created before this Unix timestamp
    #[uniffi(default = None)]
    pub until: Option<u64>,
    /// Case-insensitive substring of the URL or filename
    #[uniffi(default = None)]
    pub search: Option<String>,
    /// Result order (`None` = newest first)
    #[uniffi(default = None)]
    pub order: Option<HistoryOrder>,
    /// Maximum number of results (`None` = all)
    #[uniffi(default = None)]
    pub limit: Option<u32>,
    /// Number of results to skip
    #[uniffi(default = 0)]
    pub offset: u32,
}

impl From<HistoryFilter> for HistoryQuery {
    fn from(f: HistoryFilter) -> Self {
        let mut q = HistoryQuery::new()
            .sort(f.order.unwrap_or_default().into())
            .offset(f.offset as usize);
        for s in f.statuses {
            q = q.status(s.into());
        }
        if let Some(d) = f.domain.as_deref().filter(|d| !d.trim().is_empty()) {
            q = q.domain(d);
        }
        if let Some(t) = f.file_type.as_deref() {
            q = q.file_type(t);
        }
        if let Some(p) = f.protocol.as_deref().filter(|p| !p.trim().is_empty()) {
            q = q.protocol(p);
        }
        if let Some(s) = f.since {
            q = q.since(s);
        }
        if let Some(u) = f.until {
            q = q.until(u);
        }
        if let Some(s) = f.search.as_deref().filter(|s| !s.is_empty()) {
            q = q.search(s);
        }
        if let Some(l) = f.limit {
            q = q.limit(l as usize);
        }
        q
    }
}

/// One record of the download history.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct HistoryItem {
    pub id: String,
    /// Source URL (credentials removed)
    pub url: String,
    pub filename: String,
    pub output_dir: String,
    pub status: HistoryStatus,
    pub bytes_total: Option<u64>,
    pub sha256: Option<String>,
    pub expected_sha256: Option<String>,
    /// Unix timestamps (seconds)
    pub created_at: u64,
    pub finished_at: Option<u64>,
    pub error: Option<String>,
    pub protocol: Option<String>,
    pub duration_ms: Option<u64>,
    /// Average speed in bytes per second
    pub avg_speed: Option<u64>,
    /// `true` verified, `false` mismatch, `None` not checked
    pub checksum_ok: Option<bool>,
}

impl From<HistoryEntry> for HistoryItem {
    fn from(e: HistoryEntry) -> Self {
        Self {
            id: e.id,
            url: e.url,
            filename: e.filename,
            output_dir: e.output_dir,
            status: e.status.into(),
            bytes_total: e.bytes_total,
            sha256: e.sha256,
            expected_sha256: e.expected_sha256,
            created_at: e.created_at,
            finished_at: e.finished_at,
            error: e.error,
            protocol: e.protocol,
            duration_ms: e.duration_ms,
            avg_speed: e.avg_speed,
            checksum_ok: e.checksum_ok,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kget::events::{FileEntry, FileProgressEntry};

    #[test]
    fn states_round_trip() {
        for s in [
            DownloadState::Queued,
            DownloadState::Running,
            DownloadState::Verifying,
            DownloadState::Paused,
            DownloadState::Cancelled,
            DownloadState::Completed,
            DownloadState::Failed,
        ] {
            assert_eq!(JobState::from(s).as_core(), s);
            assert_eq!(job_state_is_finished(s.into()), s.is_finished());
        }
    }

    #[test]
    fn protocols_round_trip() {
        for p in [
            Protocol::Http,
            Protocol::Ftp,
            Protocol::Sftp,
            Protocol::WebDav,
            Protocol::Torrent,
            Protocol::Metalink,
            Protocol::Video,
        ] {
            assert_eq!(Protocol::from(DownloadProtocol::from(p)), p);
        }
    }

    fn request(url: &str) -> DownloadRequest {
        DownloadRequest {
            url: url.into(),
            output: None,
            protocol: None,
            advanced: false,
            sha256: None,
            checksum: None,
            checksum_url: None,
            headers: Vec::new(),
            resume: false,
            overwrite: None,
            retries: None,
            timeout_secs: None,
            extract: false,
            video_quality: None,
        }
    }

    #[test]
    fn request_defaults_match_core_defaults() {
        let job = request(" https://example.com/a.iso ").into_job().unwrap();
        let core = DownloadJob::new("https://example.com/a.iso");
        assert_eq!(job, core);
    }

    #[test]
    fn request_maps_every_field() {
        let mut r = request("https://example.com/a.iso");
        r.output = Some("/tmp/out".into());
        r.protocol = Some(DownloadProtocol::Http);
        r.advanced = true;
        r.sha256 = Some("ab".into());
        r.checksum = Some("md5:0123456789abcdef0123456789abcdef".into());
        r.checksum_url = Some("https://example.com/SHA256SUMS".into());
        r.headers = vec![HttpHeader {
            name: "X-Test".into(),
            value: "1".into(),
        }];
        r.resume = true;
        r.overwrite = Some(OverwriteMode::Rename);
        r.retries = Some(0);
        r.timeout_secs = Some(7);
        r.extract = true;
        r.video_quality = Some("720p".into());
        let job = r.into_job().unwrap();
        assert_eq!(job.output.as_deref(), Some("/tmp/out"));
        assert_eq!(job.protocol, Some(Protocol::Http));
        assert!(job.advanced && job.resume && job.extract);
        assert_eq!(job.sha256.as_deref(), Some("ab"));
        assert_eq!(
            job.checksum,
            Some((
                ChecksumAlgorithm::Md5,
                "0123456789abcdef0123456789abcdef".into()
            ))
        );
        assert_eq!(job.headers, vec![("X-Test".into(), "1".into())]);
        assert_eq!(job.overwrite, OverwritePolicy::Rename);
        assert_eq!(job.retries, 1, "at least one attempt");
        assert_eq!(job.timeout, 7);
        assert_eq!(job.quality, VideoQuality::P720);
    }

    #[test]
    fn invalid_requests_are_rejected() {
        assert!(matches!(
            request("  ").into_job(),
            Err(EngineError::InvalidRequest { .. })
        ));
        let mut r = request("https://example.com/a");
        r.checksum = Some("nope".into());
        assert!(matches!(
            r.into_job(),
            Err(EngineError::InvalidRequest { .. })
        ));
    }

    #[test]
    fn job_errors_map() {
        assert_eq!(
            EngineError::from(JobError::NotFound(3)),
            EngineError::NotFound { id: 3 }
        );
        let e = EngineError::from(JobError::InvalidState {
            id: 4,
            state: DownloadState::Running,
            action: "remove",
        });
        assert_eq!(e.to_string(), "Cannot remove job 4: it is running");
    }

    #[test]
    fn events_map() {
        let e: JobEventRecord = JobEvent::State {
            id: 1,
            state: DownloadState::Failed,
            error: Some("boom".into()),
        }
        .into();
        assert_eq!(
            e,
            JobEventRecord::StateChanged {
                id: 1,
                state: JobState::Failed,
                error: Some("boom".into())
            }
        );

        let progress: JobEventRecord = JobEvent::Download {
            id: 2,
            event: JsonEvent::Progress {
                bytes: 5,
                total: Some(10),
                percent: Some(50.0),
                speed_bps: 100,
                eta_secs: Some(1),
                progress: Some(0.5),
            },
        }
        .into();
        assert_eq!(progress.id(), 2);
        assert!(matches!(
            progress,
            JobEventRecord::Progress {
                progress: JobProgressRecord {
                    bytes: 5,
                    total: Some(10),
                    ..
                },
                ..
            }
        ));

        let files: JobEventRecord = JobEvent::Download {
            id: 3,
            event: JsonEvent::FileProgress {
                files: vec![FileProgressEntry {
                    idx: 1,
                    downloaded: 2,
                    size: 4,
                    pct: 50.0,
                }],
            },
        }
        .into();
        assert_eq!(
            files,
            JobEventRecord::FileProgress {
                id: 3,
                files: vec![TorrentFileProgress {
                    index: 1,
                    downloaded: 2,
                    size: 4,
                    percent: 50.0
                }]
            }
        );

        let list: JobEventRecord = JobEvent::Download {
            id: 3,
            event: JsonEvent::Files {
                files: vec![FileEntry {
                    name: "a".into(),
                    size: 1,
                }],
            },
        }
        .into();
        assert!(matches!(list, JobEventRecord::Files { ref files, .. } if files.len() == 1));

        let phase: JobEventRecord = JobEvent::Download {
            id: 4,
            event: JsonEvent::Phase {
                phase: Phase::Verifying,
                algorithm: Some("sha256".into()),
            },
        }
        .into();
        assert_eq!(
            phase,
            JobEventRecord::Phase {
                id: 4,
                phase: JobPhase::Verifying,
                algorithm: Some("sha256".into())
            }
        );
        assert_eq!(
            JobEventRecord::from(JobEvent::Removed { id: 9 }),
            JobEventRecord::Removed { id: 9 }
        );
    }

    #[test]
    fn history_filter_maps() {
        let q: HistoryQuery = HistoryFilter {
            statuses: vec![HistoryStatus::Failed, HistoryStatus::Failed],
            domain: Some("example.com".into()),
            file_type: Some("xyz,zip".into()),
            protocol: Some("http".into()),
            since: Some(10),
            until: Some(20),
            search: Some("ubuntu".into()),
            order: Some(HistoryOrder::Largest),
            limit: Some(5),
            offset: 2,
        }
        .into();
        assert_eq!(q.statuses, vec![EntryStatus::Failed]);
        assert_eq!(q.domain.as_deref(), Some("example.com"));
        assert_eq!(q.extensions, vec!["xyz".to_string(), "zip".to_string()]);
        assert_eq!(q.protocol.as_deref(), Some("http"));
        assert_eq!((q.since, q.until), (Some(10), Some(20)));
        assert_eq!(q.search.as_deref(), Some("ubuntu"));
        assert_eq!(q.sort, HistorySort::Largest);
        assert_eq!((q.limit, q.offset), (Some(5), 2));

        let all: HistoryQuery = HistoryFilter::default().into();
        assert!(all.statuses.is_empty() && all.limit.is_none());
        assert_eq!(all.sort, HistorySort::Newest);
    }
}
