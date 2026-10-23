//! Wire protocol of the daemon: newline-delimited JSON-RPC 2.0.
//!
//! Each request and response is one JSON object on one line (UTF-8, at most
//! [`MAX_REQUEST_BYTES`](super::MAX_REQUEST_BYTES)). Batches (arrays) are
//! accepted. The protocol is versioned with [`PROTOCOL_VERSION`] (returned by
//! `version`); new methods and fields may be added within a version, removals
//! or incompatible changes bump it.
//!
//! | Method | Params | Result |
//! |--------|--------|--------|
//! | `version` | – | `{protocol, kget, pid, started_at, transport, download_dir, allow_outside_download_dir, methods}` |
//! | `auth` | `{token}` | `{ok: true}` — first request on the TCP transport |
//! | `submit` | [`SubmitParams`] | job snapshot |
//! | `list` | `{states?: [state], limit?}` | `{jobs: [job]}` |
//! | `get` | `{id}` | job snapshot |
//! | `wait` | `{id, timeout_secs?}` | `{finished, job}` |
//! | `cancel` / `pause` / `resume` / `remove` | `{id}` | job snapshot |
//! | `subscribe` | `{ids?: [id], progress?: bool}` | `{subscribed: true}`, then `job.event` notifications |
//! | `unsubscribe` | – | `{subscribed: false}` |
//! | `history.query` | [`HistoryParams`] | `{entries: [entry], total}` |
//! | `config.get` | – | `{config, download_dir, allow_outside_download_dir, max_concurrent}` |
//! | `shutdown` | – | `{ok: true}`; running jobs are saved and resumed on the next start |
//!
//! A job snapshot is a [`JobInfo`](crate::jobs::JobInfo) with credentials
//! removed from URLs and header values replaced by `"<redacted>"`:
//!
//! ```json
//! {"id":1,"state":"running","protocol":"http","created_at":1760000000,
//!  "started_at":1760000001,"finished_at":null,"path":null,"error":null,
//!  "handed_off":false,"progress":{"bytes":1024,"total":4096,"percent":25.0,
//!  "speed_bps":2048,"eta_secs":1},"job":{"url":"https://example.com/a.iso",…}}
//! ```
//!
//! Events are notifications on the subscribed connection; `params` is a
//! [`JobEvent`](crate::jobs::JobEvent):
//!
//! ```json
//! {"jsonrpc":"2.0","method":"job.event","params":{"type":"state","id":1,"state":"completed","error":null}}
//! {"jsonrpc":"2.0","method":"job.event","params":{"type":"download","id":1,"event":{"event":"progress","bytes":1024,…}}}
//! {"jsonrpc":"2.0","method":"job.event","params":{"type":"removed","id":1}}
//! ```

use crate::OverwritePolicy;
use crate::jobs::{DownloadState, JobError, JobId, JobInfo};
use crate::protocol::Protocol;
use crate::runner::DownloadJob;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use std::fmt;
use std::path::Path;

/// Version of the daemon protocol.
pub const PROTOCOL_VERSION: u32 = 1;

/// Method name of event notifications sent after `subscribe`.
pub const EVENT_METHOD: &str = "job.event";

/// Every method the daemon serves.
pub const METHODS: &[&str] = &[
    "version",
    "auth",
    "submit",
    "list",
    "get",
    "wait",
    "cancel",
    "pause",
    "resume",
    "remove",
    "subscribe",
    "unsubscribe",
    "history.query",
    "config.get",
    "shutdown",
];

/// JSON-RPC error codes. `-32700…-32600` are standard, `-320xx` are KGet's.
pub mod codes {
    /// The line is not valid JSON
    pub const PARSE_ERROR: i64 = -32700;
    /// Not a JSON-RPC 2.0 request
    pub const INVALID_REQUEST: i64 = -32600;
    /// Unknown method
    pub const METHOD_NOT_FOUND: i64 = -32601;
    /// Missing or malformed parameters
    pub const INVALID_PARAMS: i64 = -32602;
    /// Unexpected server error
    pub const INTERNAL_ERROR: i64 = -32603;
    /// No job with this id
    pub const JOB_NOT_FOUND: i64 = -32001;
    /// The operation is not allowed in the job's state
    pub const INVALID_STATE: i64 = -32002;
    /// The output path is outside the allowed directory or malformed
    pub const PATH_NOT_ALLOWED: i64 = -32003;
    /// Missing or wrong token (TCP transport)
    pub const UNAUTHORIZED: i64 = -32004;
    /// The request line exceeds the size limit
    pub const REQUEST_TOO_LARGE: i64 = -32005;
    /// The daemon is shutting down
    pub const SHUTTING_DOWN: i64 = -32006;
    /// Client and daemon speak incompatible protocol versions
    pub const VERSION_MISMATCH: i64 = -32007;
}

/// JSON-RPC error object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcError {
    /// Error code (see [`codes`])
    pub code: i64,
    /// Human-readable message (never contains secrets)
    pub message: String,
    /// Extra structured data
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    /// Error with `code` and `message`.
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// [`codes::INVALID_PARAMS`]
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(codes::INVALID_PARAMS, message)
    }

    /// [`codes::INTERNAL_ERROR`]
    pub fn internal(message: impl fmt::Display) -> Self {
        Self::new(codes::INTERNAL_ERROR, message.to_string())
    }

    /// [`codes::METHOD_NOT_FOUND`]
    pub fn method_not_found(method: &str) -> Self {
        Self::new(
            codes::METHOD_NOT_FOUND,
            format!("Unknown method '{method}'"),
        )
    }
}

impl fmt::Display for RpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (code {})", self.message, self.code)
    }
}

impl std::error::Error for RpcError {}

impl From<JobError> for RpcError {
    fn from(e: JobError) -> Self {
        let code = match e {
            JobError::NotFound(_) => codes::JOB_NOT_FOUND,
            JobError::InvalidState { .. } => codes::INVALID_STATE,
        };
        Self::new(code, e.to_string())
    }
}

impl From<super::PathError> for RpcError {
    fn from(e: super::PathError) -> Self {
        Self::new(codes::PATH_NOT_ALLOWED, e.to_string())
    }
}

/// Successful response.
pub fn success(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

/// Error response.
pub fn failure(id: &Value, error: &RpcError) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": error})
}

/// Notification (no id, no response).
pub fn notification(method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "method": method, "params": params})
}

/// Request with a numeric id.
pub fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

/// Deserialize `params` (`null` = defaults) into `T`.
pub fn parse_params<T: for<'de> Deserialize<'de> + Default>(params: Value) -> Result<T, RpcError> {
    if params.is_null() {
        return Ok(T::default());
    }
    serde_json::from_value(params).map_err(|e| RpcError::invalid_params(e.to_string()))
}

// ============================================================================
// Parameters
// ============================================================================

/// Parameters of `submit`.
///
/// Only `url` is required. `dir` is relative to the daemon's download
/// directory (or absolute inside it); `filename` is a single file name.
///
/// ```json
/// {"url":"https://example.com/a.iso","dir":"isos","filename":"a.iso",
///  "checksum":"sha256:9f86d0…","headers":{"Referer":"https://example.com/"}}
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SubmitParams {
    /// URL, magnet link, or absolute path of a local `.torrent` / `.meta4`
    pub url: String,
    /// Output directory (default: the download directory)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
    /// Output file name (default: from the URL)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    /// Force a protocol (`http`, `ftp`, `sftp`, `webdav`, `torrent`, `metalink`, `video`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<Protocol>,
    /// Parallel, resumable HTTP download (turbo mode)
    #[serde(alias = "turbo")]
    pub advanced: bool,
    /// Expected SHA-256 (hex)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// Expected checksum as `<algorithm>:<hex>` (sha256, sha512, sha1, md5, blake3)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    /// URL of a checksum file (`SHA256SUMS`, `file.iso.sha256`…)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum_url: Option<String>,
    /// Extra HTTP headers: `{"Name": "value"}` or `[["Name", "value"], …]`
    #[serde(
        deserialize_with = "header_list",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub headers: Vec<(String, String)>,
    /// yt-dlp quality (`best`, `1080p`, `720p`, `480p`, `360p`, `audio`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
    /// Continue an existing partial file
    pub resume: bool,
    /// When the file exists: `rename` (default: `file (1).zip`), `overwrite` or `skip`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overwrite: Option<OverwritePolicy>,
    /// Maximum attempts per request
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retries: Option<u32>,
    /// Connect / read timeout in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
    /// Extract the archive after downloading
    pub extract: bool,
    /// `hf://`: only files matching one of these globs
    #[serde(
        deserialize_with = "string_or_list",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub include: Vec<String>,
    /// `hf://`: skip files matching any of these globs
    #[serde(
        deserialize_with = "string_or_list",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub exclude: Vec<String>,
    /// `hf://`: branch, tag or commit
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// `hf://`: write into the Hugging Face hub cache (outside the download
    /// directory, so only allowed with `allow_outside_download_dir`)
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hf_cache: bool,
    /// Tags recorded in the history (merged with the matching smart rule's)
    #[serde(
        deserialize_with = "string_or_list",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub tags: Vec<String>,
    /// Apply the smart rules: `true` / `false`, absent = `rules_enabled` in
    /// the daemon's configuration. Rule folders are created inside `dir`
    /// (default: the download directory). Ignored when `filename` is given,
    /// and when `dir` is given without `organize: true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organize: Option<bool>,
    /// The URL was already downloaded and the file is still there: `skip`,
    /// `download` (default) or `link`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duplicates: Option<crate::dedupe::DuplicatePolicy>,
    /// Replace the file by a hard link when the history already has a file
    /// with the same SHA-256
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub dedupe: bool,
    /// Torrent: files to download by number, as `kget --list-files` shows
    /// them (`"1,3-5"`, 1-based)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub select: Option<String>,
    /// Torrent: files to download by path pattern (`"*.mkv"`)
    #[serde(
        deserialize_with = "string_or_list",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub select_glob: Vec<String>,
    /// Torrent: keep seeding until this share ratio
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed_ratio: Option<f64>,
    /// Torrent: keep seeding for this many seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed_time_secs: Option<u64>,
}

impl SubmitParams {
    /// Parameters for `url` with default options.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            ..Self::default()
        }
    }

    /// Parameters for a job built by the command line. A relative output is
    /// taken relative to `cwd` (the client's directory, not the daemon's);
    /// an output that is an existing directory or ends with a separator is
    /// the directory, anything else the file.
    pub fn from_job(job: &DownloadJob, cwd: &Path) -> Self {
        let (dir, filename) = match job.output.as_deref().filter(|o| !o.is_empty()) {
            None => (None, None),
            Some(out) => {
                let expanded = crate::utils::expand_tilde(out);
                let is_dir_hint = expanded.ends_with('/') || expanded.ends_with('\\');
                let path = cwd.join(&expanded);
                let single_file = !matches!(
                    job.protocol(),
                    Protocol::Torrent
                        | Protocol::Metalink
                        | Protocol::HuggingFace
                        | Protocol::Ollama
                );
                if is_dir_hint || path.is_dir() || !single_file {
                    (Some(path.to_string_lossy().into_owned()), None)
                } else {
                    (
                        path.parent().map(|p| p.to_string_lossy().into_owned()),
                        path.file_name().map(|n| n.to_string_lossy().into_owned()),
                    )
                }
            }
        };
        Self {
            url: job.url.clone(),
            dir,
            filename,
            protocol: job.protocol,
            advanced: job.advanced,
            sha256: job.sha256.clone(),
            checksum: job
                .checksum
                .as_ref()
                .map(|(alg, hex)| format!("{}:{hex}", alg.name())),
            checksum_url: job.checksum_url.clone(),
            headers: job.headers.clone(),
            quality: Some(job.quality.as_str().to_string()),
            resume: job.resume,
            overwrite: Some(job.overwrite),
            retries: Some(job.retries),
            timeout: Some(job.timeout),
            extract: job.extract,
            include: job.include.clone(),
            exclude: job.exclude.clone(),
            revision: job.revision.clone(),
            hf_cache: job.hf_cache,
            tags: job.tags.clone(),
            organize: job.organize,
            duplicates: job.duplicates,
            dedupe: job.dedupe,
            select: (!job.torrent.select.indices.is_empty()).then(|| {
                job.torrent
                    .select
                    .indices
                    .iter()
                    .map(|i| (i + 1).to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            }),
            select_glob: job.torrent.select.globs.clone(),
            seed_ratio: job.torrent.seed_ratio,
            seed_time_secs: job.torrent.seed_time_secs,
        }
    }
}

fn header_list<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<(String, String)>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Headers {
        Map(std::collections::BTreeMap<String, String>),
        List(Vec<(String, String)>),
    }
    Ok(match Option::<Headers>::deserialize(d)? {
        None => Vec::new(),
        Some(Headers::Map(m)) => m.into_iter().collect(),
        Some(Headers::List(l)) => l,
    })
}

/// `{id}` (`get`, `cancel`, `pause`, `resume`, `remove`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IdParams {
    /// Job id
    pub id: JobId,
}

/// Parameters of `wait`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WaitParams {
    /// Job id
    pub id: JobId,
    /// Give up after this many seconds (default 60, at most 3600)
    pub timeout_secs: Option<u64>,
}

/// Parameters of `list`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ListParams {
    /// Only jobs in these states (empty = all)
    pub states: Vec<DownloadState>,
    /// Newest `limit` jobs only
    pub limit: Option<usize>,
}

/// Parameters of `subscribe`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SubscribeParams {
    /// Only events of these jobs (empty = all)
    pub ids: Vec<JobId>,
    /// Include `progress` / `file_progress` download events (default `true`)
    pub progress: bool,
}

impl Default for SubscribeParams {
    fn default() -> Self {
        Self {
            ids: Vec::new(),
            progress: true,
        }
    }
}

/// Parameters of `auth`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthParams {
    /// Contents of the token in `daemon.tcp`
    pub token: String,
}

/// Parameters of `history.query`; same filters as `kget --history`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryParams {
    /// Text in the URL or file name
    pub search: Option<String>,
    /// Statuses (`completed`, `failed`, `cancelled`, `handed_off`)
    #[serde(deserialize_with = "string_or_list")]
    pub status: Vec<String>,
    /// Domain (also matches subdomains)
    pub domain: Option<String>,
    /// Tags (every one must be present)
    #[serde(deserialize_with = "string_or_list")]
    pub tags: Vec<String>,
    /// File type (`video`, `archive`, …) or extensions (`iso,zip`)
    pub file_type: Option<String>,
    /// Protocol (`http`, `ftp`, …)
    pub protocol: Option<String>,
    /// Lower date bound (`YYYY-MM-DD`, `today`, `7d`, …; UTC)
    pub since: Option<String>,
    /// Upper date bound
    pub until: Option<String>,
    /// `newest` (default), `oldest`, `largest`, `smallest`, `name`, `fastest`
    pub sort: Option<String>,
    /// Maximum entries (default 50, at most 500)
    pub limit: Option<usize>,
    /// Entries to skip
    pub offset: usize,
}

impl HistoryParams {
    /// The equivalent `kget --history` filter.
    pub fn filter(&self) -> crate::cli::HistoryFilter {
        crate::cli::HistoryFilter {
            status: self.status.clone(),
            domain: self.domain.clone(),
            file_type: self.file_type.clone(),
            protocol: self.protocol.clone(),
            since: self.since.clone(),
            until: self.until.clone(),
            search: self.search.clone(),
            tags: self.tags.clone(),
            sort: self.sort.clone().unwrap_or_default(),
            limit: self.limit.unwrap_or(50).clamp(1, 500),
            offset: self.offset,
        }
    }
}

fn string_or_list<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    Ok(match Option::<OneOrMany>::deserialize(d)? {
        None => Vec::new(),
        Some(OneOrMany::One(s)) => vec![s],
        Some(OneOrMany::Many(v)) => v,
    })
}

// ============================================================================
// Redaction
// ============================================================================

/// A job snapshot safe to hand to clients: credentials removed from URLs and
/// header values replaced by `"<redacted>"`.
pub fn redact_info(info: &JobInfo) -> Value {
    let mut info = info.clone();
    info.job.url = crate::queue::redact_url(&info.job.url);
    info.job.checksum_url = info
        .job
        .checksum_url
        .as_deref()
        .map(crate::queue::redact_url);
    for (_, value) in info.job.headers.iter_mut() {
        *value = "<redacted>".into();
    }
    serde_json::to_value(&info).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submit_params_accept_header_map_and_list() {
        let a: SubmitParams =
            serde_json::from_value(json!({"url": "u", "headers": {"A": "1"}})).unwrap();
        let b: SubmitParams =
            serde_json::from_value(json!({"url": "u", "headers": [["A", "1"]]})).unwrap();
        assert_eq!(a.headers, b.headers);
        assert!(serde_json::from_value::<SubmitParams>(json!({"url": "u", "bogus": 1})).is_err());
        let t: SubmitParams = serde_json::from_value(json!({"url": "u", "turbo": true})).unwrap();
        assert!(t.advanced);
    }

    #[test]
    fn from_job_splits_output() {
        let cwd = tempfile::tempdir().unwrap();
        let mut job = DownloadJob::new("https://example.com/a.iso");
        job.output = Some("sub/b.iso".into());
        let p = SubmitParams::from_job(&job, cwd.path());
        assert_eq!(p.filename.as_deref(), Some("b.iso"));
        assert_eq!(
            p.dir.as_deref(),
            Some(cwd.path().join("sub").to_str().unwrap())
        );
        job.output = Some("out/".into());
        let p = SubmitParams::from_job(&job, cwd.path());
        assert_eq!(p.filename, None);
    }

    #[test]
    fn redaction_hides_secrets() {
        let mut job = DownloadJob::new("sftp://me:secret@host/f");
        job.headers = vec![("Authorization".into(), "Bearer t0ken".into())];
        let info = JobInfo {
            id: 1,
            protocol: job.protocol(),
            job,
            state: DownloadState::Queued,
            created_at: 0,
            started_at: None,
            finished_at: None,
            progress: Default::default(),
            path: None,
            error: None,
            handed_off: false,
        };
        let text = redact_info(&info).to_string();
        assert!(!text.contains("secret"), "{text}");
        assert!(!text.contains("t0ken"), "{text}");
        assert!(text.contains("Authorization"));
    }
}
