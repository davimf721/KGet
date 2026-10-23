//! Advanced parallel download functionality with resume support.
//!
//! The [`AdvancedDownloader`] provides high-performance downloads using:
//! - **Parallel connections**: Split files into chunks downloaded simultaneously
//! - **Work stealing**: Idle connections split the slowest remaining range
//! - **HTTP/2 multiplexing**: Fewer TCP connections when the server speaks h2
//! - **Multiple mirrors**: Different chunks from several URLs at once
//! - **Resume support**: Continue interrupted downloads from where they left off
//! - **Progress callbacks**: Real-time progress and status updates
//! - **Cancellation**: Graceful download cancellation via atomic tokens
//!
//! # How resume works
//!
//! While downloading, data goes to `<file>.part` and the progress of every
//! chunk is recorded in `<file>.kget` (JSON). The final file only appears once
//! every chunk is complete and the size (and SHA-256, if given) matched. On the
//! next run, the state is reused **only** if the remote resource is the same
//! (URL, size, `ETag`, `Last-Modified`), and only the missing byte ranges are
//! requested. A pre-existing final file is never used as a resume point.
//!
//! The state format is unchanged since 1.7.1 (`"version": 1`): a range split by
//! work stealing is stored as one more `{start, end, done}` chunk, so 1.7.1
//! states resume here and states written here still resume on 1.7.1.
//!
//! # Example
//!
//! ```rust,no_run
//! use kget::{AdvancedDownloader, ProxyConfig, Optimizer};
//!
//! let mut downloader = AdvancedDownloader::new(
//!     "https://releases.ubuntu.com/22.04/ubuntu-22.04-desktop-amd64.iso".to_string(),
//!     "ubuntu.iso".to_string(),
//!     false,
//!     ProxyConfig::default(),
//!     Optimizer::new(),
//! )?;
//!
//! // Set progress callback (0.0 to 1.0)
//! downloader.set_progress_callback(|progress| {
//!     println!("Progress: {:.1}%", progress * 100.0);
//! });
//!
//! // Set status callback for messages
//! downloader.set_status_callback(|msg| {
//!     println!("Status: {}", msg);
//! });
//!
//! // Start download
//! downloader.download()?;
//! # Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
//! ```
//!
//! # Parallel Downloads
//!
//! The number of connections comes from the [`Optimizer`] configuration
//! (`max_connections`, 1–32). Range requests are async tasks on a small,
//! process-wide tokio runtime, so the count is not limited by the number of
//! CPU cores and no thread is parked per connection. [`download`] stays
//! synchronous; from async code, call it inside `spawn_blocking`.
//!
//! When a connection runs out of work it takes the next unstarted chunk; when
//! none is left it halves the in-flight range that will finish last, so one
//! slow connection does not hold up the tail of the download.
//!
//! [`download`]: AdvancedDownloader::download

use crate::DownloadOptions;
use crate::config::ProxyConfig;
use crate::error::KgetError;
use crate::guard;
use crate::http::{self, ClientOptions, HttpVersion};
use crate::optimization::Optimizer;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs::File;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tokio::task::JoinSet;

#[cfg(target_family = "unix")]
use std::os::unix::fs::FileExt;
#[cfg(target_family = "windows")]
use std::os::windows::fs::FileExt;

type BoxError = Box<dyn Error + Send + Sync>;

/// Minimum chunk size for parallel downloads (4 MB)
const MIN_CHUNK_SIZE: u64 = 4 * 1024 * 1024;
/// Maximum chunk size for parallel downloads (64 MB)
const MAX_CHUNK_SIZE: u64 = 64 * 1024 * 1024;
/// Received bytes are buffered per connection and written in blocks this big
const WRITE_BUF: usize = 256 * 1024;
/// Version of the `.kget` state format
const STATE_VERSION: u32 = 1;
/// Work stealing never leaves either half of a split range below this size
const MIN_SPLIT: u64 = 512 * 1024;
// The owner of a split range may hold up to WRITE_BUF unwritten bytes (plus
// one more piece) past its recorded position; the split point must be beyond.
const _: () = assert!(MIN_SPLIT >= 2 * WRITE_BUF as u64);
/// A range must have been in flight this long before it can be split
const STEAL_GRACE: Duration = Duration::from_secs(1);
/// Only ranges expected to need at least this long are worth splitting
const STEAL_MIN_ETA: Duration = Duration::from_secs(2);
/// Range requests multiplexed over each HTTP/2 (or HTTP/3) connection
const H2_STREAMS_PER_CONN: usize = 4;
/// Interval of the progress / cancellation ticker
const TICK: Duration = Duration::from_millis(100);

/// Controls how [`AdvancedDownloader`] behaves around resume and prompts.
///
/// Non-interactive callers (library, `--jsonl`, automation) should use
/// [`AlwaysResume`](ResumePolicy::AlwaysResume) or
/// [`AlwaysRestart`](ResumePolicy::AlwaysRestart); [`Ask`](ResumePolicy::Ask)
/// only ever prompts when stdin is a terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResumePolicy {
    /// Resume a compatible `.part`/`.kget` state; for ISO files without an
    /// expected hash, ask on the terminal whether to compute the SHA-256.
    #[default]
    Ask,
    /// Resume a compatible state and never prompt (ISO hashes are computed).
    AlwaysResume,
    /// Always restart from scratch, discarding any partial download.
    AlwaysRestart,
}

use crate::http::io_runtime as runtime;

/// Token bucket for global download rate limiting across parallel connections.
///
/// Unlike per-connection throttling, this bounds aggregate throughput regardless
/// of how many connections are active.
struct TokenBucket {
    limit_bps: u64,
    tokens: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(limit_bps: u64) -> Self {
        Self {
            limit_bps,
            tokens: limit_bps as f64,
            last_refill: Instant::now(),
        }
    }

    /// Consume `n` bytes from the bucket and return how long to sleep before
    /// reading more.
    ///
    /// The caller must release the lock before sleeping.
    fn consume(&mut self, n: u64) -> Option<Duration> {
        let elapsed = self.last_refill.elapsed();
        let refill = elapsed.as_secs_f64() * self.limit_bps as f64;
        self.tokens = (self.tokens + refill).min(self.limit_bps as f64);
        self.last_refill = Instant::now();

        if self.tokens >= n as f64 {
            self.tokens -= n as f64;
            None
        } else {
            let deficit = n as f64 - self.tokens;
            self.tokens = 0.0;
            Some(Duration::from_secs_f64(deficit / self.limit_bps as f64))
        }
    }
}

// ============================================================================
// Resume state (<file>.kget)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChunkState {
    start: u64,
    /// Exclusive end offset
    end: u64,
    /// Bytes already written starting at `start`
    done: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ResumeState {
    version: u32,
    url: String,
    pub(crate) total_size: u64,
    etag: Option<String>,
    last_modified: Option<String>,
    chunks: Vec<ChunkState>,
}

impl ResumeState {
    fn load(path: &Path) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        Self::parse(&bytes)
    }

    pub(crate) fn parse(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }

    /// Atomic write: temporary file + rename, never a half-written JSON.
    fn save(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_vec(self).map_err(std::io::Error::other)?;
        crate::config::write_atomic(path, &json, false)
    }

    /// The state is reused only if the remote resource is the same.
    fn matches(&self, url: &str, remote: &RemoteInfo) -> bool {
        self.version == STATE_VERSION
            && self.url == url
            && Some(self.total_size) == remote.size
            && (self.etag.is_none() || self.etag == remote.etag)
            && (self.last_modified.is_none() || self.last_modified == remote.last_modified)
            && self.is_consistent()
    }

    /// The chunks must tile `0..total_size` in order and no chunk may claim
    /// more bytes than it holds. A corrupted `.kget` such as `"chunks": []`
    /// used to match, and the zero-filled `.part` was renamed as complete.
    pub(crate) fn is_consistent(&self) -> bool {
        let mut next = 0u64;
        for c in &self.chunks {
            if c.start != next || c.end <= c.start || c.done > c.end - c.start {
                return false;
            }
            next = c.end;
        }
        next == self.total_size
    }

    pub(crate) fn done_bytes(&self) -> u64 {
        self.chunks
            .iter()
            .fold(0u64, |acc, c| acc.saturating_add(c.done))
    }
}

#[derive(Debug, Clone, Default)]
struct RemoteInfo {
    size: Option<u64>,
    accepts_ranges: bool,
    etag: Option<String>,
    last_modified: Option<String>,
    /// HTTP version the server answered the probe with
    version: reqwest::Version,
}

impl RemoteInfo {
    /// Validator for `If-Range` (weak ETags are not allowed there).
    fn if_range(&self) -> Option<&str> {
        self.etag
            .as_deref()
            .filter(|e| !e.starts_with("W/"))
            .or(self.last_modified.as_deref())
    }

    /// HTTP/2 and HTTP/3 carry many requests over one connection.
    fn is_multiplexed(&self) -> bool {
        self.version >= reqwest::Version::HTTP_2
    }
}

fn version_name(v: reqwest::Version) -> &'static str {
    match v {
        reqwest::Version::HTTP_09 => "HTTP/0.9",
        reqwest::Version::HTTP_10 => "HTTP/1.0",
        reqwest::Version::HTTP_2 => "HTTP/2",
        reqwest::Version::HTTP_3 => "HTTP/3",
        _ => "HTTP/1.1",
    }
}

/// A client plus whether its requests go over HTTP/3.
///
/// reqwest sends a request over QUIC only when the request itself asks for
/// HTTP/3, even on an `http3_prior_knowledge` client.
#[derive(Clone)]
struct Conn {
    client: Client,
    h3: bool,
}

impl Conn {
    fn request(&self, method: reqwest::Method, url: &str) -> reqwest::RequestBuilder {
        let req = self.client.request(method, url);
        if self.h3 {
            req.version(reqwest::Version::HTTP_3)
        } else {
            req
        }
    }

    fn get(&self, url: &str) -> reqwest::RequestBuilder {
        self.request(reqwest::Method::GET, url)
    }

    fn head(&self, url: &str) -> reqwest::RequestBuilder {
        self.request(reqwest::Method::HEAD, url)
    }
}

/// Query size, range support and validators of `url`.
async fn probe(
    client: &Conn,
    url: &str,
    headers: &[(String, String)],
) -> Result<RemoteInfo, BoxError> {
    let header = |resp: &reqwest::Response, name: reqwest::header::HeaderName| {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };

    if let Ok(response) = http::apply_headers_async(client.head(url), headers)
        .send()
        .await
        && response.status().is_success()
    {
        let size = header(&response, reqwest::header::CONTENT_LENGTH)
            .and_then(|s| s.parse().ok())
            .filter(|l: &u64| *l > 0);
        let accepts_ranges = header(&response, reqwest::header::ACCEPT_RANGES)
            .is_some_and(|s| s.eq_ignore_ascii_case("bytes"));
        if size.is_some() && accepts_ranges {
            return Ok(RemoteInfo {
                size,
                accepts_ranges,
                etag: header(&response, reqwest::header::ETAG),
                last_modified: header(&response, reqwest::header::LAST_MODIFIED),
                version: response.version(),
            });
        }
    }

    // HEAD failed or was inconclusive: probe with a 1-byte range request.
    let response = http::apply_headers_async(client.get(url), headers)
        .header(reqwest::header::RANGE, "bytes=0-0")
        .send()
        .await?;
    let status = response.status();
    if status == reqwest::StatusCode::PARTIAL_CONTENT {
        let total = header(&response, reqwest::header::CONTENT_RANGE)
            .as_deref()
            .and_then(parse_content_range_total);
        return Ok(RemoteInfo {
            size: total,
            accepts_ranges: total.is_some(),
            etag: header(&response, reqwest::header::ETAG),
            last_modified: header(&response, reqwest::header::LAST_MODIFIED),
            version: response.version(),
        });
    }
    if status.is_success() {
        return Ok(RemoteInfo {
            size: response.content_length(),
            version: response.version(),
            ..Default::default()
        });
    }
    Err(Box::new(KgetError::from_status(status.as_u16(), url)))
}

/// A chunk request was answered with `200` (range ignored, or `If-Range`
/// says the file changed) or `416`: restart as a single stream.
#[derive(Debug)]
struct RangeNotHonored(u16);

impl std::fmt::Display for RangeNotHonored {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Server answered a range request with HTTP {} (no range support or the remote file changed)",
            self.0
        )
    }
}

impl Error for RangeNotHonored {}

/// High-performance downloader with parallel connections and resume support.
///
/// `AdvancedDownloader` is the recommended way to download large files. It provides:
///
/// - **Parallel chunk downloads**: Splits files into segments downloaded simultaneously
/// - **Work stealing**: Idle connections split the slowest in-flight range
/// - **Safe resume**: `<file>.part` + `<file>.kget` state, validated against the server
/// - **Server compatibility**: Falls back to single-stream if server doesn't support ranges
/// - **Mirrors**: Optional extra URLs ([`set_mirrors`](Self::set_mirrors)) serve other chunks
/// - **Integrity**: every chunk must be complete; size and SHA-256 are always checked
/// - **Progress tracking**: Real-time callbacks for UI integration
/// - **Cancellation support**: Stop downloads gracefully via atomic cancel token
///
/// # Example
///
/// ```rust,no_run
/// use kget::{AdvancedDownloader, ProxyConfig, Optimizer};
///
/// let downloader = AdvancedDownloader::new(
///     "https://example.com/large-file.zip".to_string(),
///     "large-file.zip".to_string(),
///     false,  // quiet_mode
///     ProxyConfig::default(),
///     Optimizer::new(),
/// )?;
///
/// downloader.download()?;
/// # Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
/// ```
///
/// # With Progress Tracking
///
/// ```rust,no_run
/// use kget::{AdvancedDownloader, ProxyConfig, Optimizer};
///
/// let mut dl = AdvancedDownloader::new(
///     "https://example.com/file.iso".to_string(),
///     "file.iso".to_string(),
///     true, // quiet mode (no stdout)
///     ProxyConfig::default(),
///     Optimizer::new(),
/// )?;
///
/// dl.set_progress_callback(|p| {
///     // p is 0.0 to 1.0
///     update_ui_progress(p);
/// });
///
/// dl.set_status_callback(|msg| println!("{}", msg));
///
/// dl.download()?;
///
/// fn update_ui_progress(p: f32) {
///     // Update your UI here
/// }
/// # Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
/// ```
#[derive(Clone)]
pub struct AdvancedDownloader {
    client: Client,
    url: String,
    mirrors: Vec<String>,
    output_path: String,
    quiet_mode: bool,
    proxy: ProxyConfig,
    optimizer: Optimizer,
    progress_callback: Option<Arc<dyn Fn(f32) + Send + Sync>>,
    byte_progress_callback: Option<crate::ByteProgressCallback>,
    status_callback: Option<Arc<dyn Fn(String) + Send + Sync>>,
    cancel_token: Arc<AtomicBool>,
    expected_sha256: Option<String>,
    extra_headers: Vec<(String, String)>,
    resume_policy: ResumePolicy,
    max_retries: u32,
    timeout_secs: u64,
    http_version: HttpVersion,
}

impl AdvancedDownloader {
    /// Create a new `AdvancedDownloader` instance.
    ///
    /// # Arguments
    ///
    /// * `url` - URL to download from
    /// * `output_path` - Local path for the downloaded file
    /// * `quiet_mode` - If true, suppress console output
    /// * `proxy_config` - Proxy settings (use `ProxyConfig::default()` for direct connection)
    /// * `optimizer` - Optimizer for connection settings
    ///
    /// # Errors
    ///
    /// Fails when the proxy is enabled but invalid.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use kget::{AdvancedDownloader, ProxyConfig, Optimizer};
    ///
    /// let dl = AdvancedDownloader::new(
    ///     "https://example.com/file.zip".to_string(),
    ///     "./downloads/file.zip".to_string(),
    ///     false,
    ///     ProxyConfig::default(),
    ///     Optimizer::new(),
    /// )?;
    /// # Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    /// ```
    pub fn new(
        url: String,
        output_path: String,
        quiet_mode: bool,
        proxy_config: ProxyConfig,
        optimizer: Optimizer,
    ) -> Result<Self, BoxError> {
        let client = http::build_async_client(&proxy_config, &ClientOptions::default())?;
        Ok(Self {
            client,
            url,
            mirrors: Vec::new(),
            output_path,
            quiet_mode,
            proxy: proxy_config,
            optimizer,
            progress_callback: None,
            byte_progress_callback: None,
            status_callback: None,
            cancel_token: Arc::new(AtomicBool::new(false)),
            expected_sha256: None,
            extra_headers: Vec::new(),
            resume_policy: ResumePolicy::default(),
            max_retries: http::DEFAULT_RETRIES,
            timeout_secs: http::DEFAULT_TIMEOUT_SECS,
            http_version: HttpVersion::Auto,
        })
    }

    /// Set a custom cancellation token for graceful download interruption.
    ///
    /// When the token is set to `true`, the download stops within about 100 ms
    /// (also while waiting for the server), the resume state is saved and an
    /// error is returned. Running the same download again continues where it
    /// stopped.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use kget::{AdvancedDownloader, ProxyConfig, Optimizer};
    /// use std::sync::Arc;
    /// use std::sync::atomic::AtomicBool;
    ///
    /// let cancel = Arc::new(AtomicBool::new(false));
    /// let mut dl = AdvancedDownloader::new(/* ... */
    /// #    "".to_string(), "".to_string(), false, ProxyConfig::default(), Optimizer::new()
    /// )?;
    /// dl.set_cancel_token(cancel.clone());
    ///
    /// // In another thread:
    /// // cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    /// # Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    /// ```
    pub fn set_cancel_token(&mut self, token: Arc<AtomicBool>) {
        self.cancel_token = token;
    }

    /// Check if the download has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancel_token.load(Ordering::Relaxed)
    }

    /// Set a callback for progress updates.
    ///
    /// The callback receives a value from 0.0 (start) to 1.0 (complete), about
    /// five times per second.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use kget::{AdvancedDownloader, ProxyConfig, Optimizer};
    /// # let mut dl = AdvancedDownloader::new("".to_string(), "".to_string(), false, ProxyConfig::default(), Optimizer::new())?;
    /// dl.set_progress_callback(|progress| {
    ///     println!("Downloaded: {:.1}%", progress * 100.0);
    /// });
    /// # Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    /// ```
    pub fn set_progress_callback(&mut self, callback: impl Fn(f32) + Send + Sync + 'static) {
        self.progress_callback = Some(Arc::new(callback));
    }

    /// Set a callback receiving `(bytes_downloaded, total_bytes)`.
    pub fn set_byte_progress_callback(
        &mut self,
        callback: impl Fn(u64, Option<u64>) + Send + Sync + 'static,
    ) {
        self.byte_progress_callback = Some(Arc::new(callback));
    }

    /// Set a callback for status messages.
    ///
    /// Receives human-readable status updates during the download.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use kget::{AdvancedDownloader, ProxyConfig, Optimizer};
    /// # let mut dl = AdvancedDownloader::new("".to_string(), "".to_string(), false, ProxyConfig::default(), Optimizer::new())?;
    /// dl.set_status_callback(|msg| {
    ///     println!("Download status: {}", msg);
    /// });
    /// # Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    /// ```
    pub fn set_status_callback(&mut self, callback: impl Fn(String) + Send + Sync + 'static) {
        self.status_callback = Some(Arc::new(callback));
    }

    /// Set an expected SHA-256 hash; the download fails if the file doesn't match.
    pub fn set_expected_sha256(&mut self, expected_sha256: impl Into<String>) {
        self.expected_sha256 = Some(expected_sha256.into());
    }

    /// Set extra HTTP headers sent with every request (e.g. `Referer` or `Cookie`).
    pub fn set_extra_headers(&mut self, headers: Vec<(String, String)>) {
        self.extra_headers = headers;
    }

    /// Set how the downloader handles resume state and interactive prompts.
    pub fn set_resume_policy(&mut self, policy: ResumePolicy) {
        self.resume_policy = policy;
    }

    /// Additional URLs serving the same file, in order of preference.
    ///
    /// Every mirror whose size matches the main URL and that supports range
    /// requests downloads its own share of the chunks. A mirror that fails
    /// (error status, ignored ranges, retries exhausted) is dropped and its
    /// chunks continue on the others; the download only fails when the last
    /// one fails. Mirrors serve different bytes of one file, so set an
    /// expected hash ([`set_expected_sha256`](Self::set_expected_sha256)) to
    /// catch a mirror with different content.
    pub fn set_mirrors(&mut self, mirrors: Vec<String>) {
        self.mirrors = mirrors.into_iter().filter(|m| *m != self.url).collect();
    }

    /// HTTP version policy (default [`HttpVersion::Auto`]: HTTP/2 when the
    /// server offers it). [`HttpVersion::Http3`] falls back to HTTP/2 or
    /// HTTP/1.1 when the QUIC connection fails.
    ///
    /// # Errors
    ///
    /// Fails when the proxy is enabled but invalid.
    pub fn set_http_version(&mut self, version: HttpVersion) -> Result<(), BoxError> {
        self.http_version = version;
        self.client = http::build_async_client(&self.proxy, &self.client_options())?;
        Ok(())
    }

    /// Maximum consecutive failed attempts per chunk without progress (default 3).
    pub fn set_retries(&mut self, retries: u32) {
        self.max_retries = retries.max(1);
    }

    /// Connect / per-read timeout in seconds (default 60, 0 = no read timeout).
    pub fn set_timeout_secs(&mut self, secs: u64) -> Result<(), BoxError> {
        self.timeout_secs = secs;
        self.client = http::build_async_client(&self.proxy, &self.client_options())?;
        Ok(())
    }

    fn client_options(&self) -> ClientOptions {
        ClientOptions::with_timeout_secs(self.timeout_secs).with_http_version(self.http_version)
    }

    fn send_status(&self, msg: &str) {
        if let Some(cb) = &self.status_callback {
            cb(msg.to_string());
        }
        if !self.quiet_mode {
            println!("{}", msg);
        }
    }

    fn log(&self, msg: &str) {
        if !self.quiet_mode {
            println!("{}", msg);
        }
    }

    fn part_path(&self) -> PathBuf {
        crate::download::part_path_for(Path::new(&self.output_path))
    }

    fn state_path(&self) -> PathBuf {
        let p = Path::new(&self.output_path);
        let mut name = p
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_else(|| "download".into());
        name.push(".kget");
        p.with_file_name(name)
    }

    /// Start the download.
    ///
    /// This method:
    /// 1. Queries the server for size, range support, `ETag` and `Last-Modified`
    /// 2. Reuses a compatible `.part`/`.kget` state, or starts fresh
    /// 3. Downloads the missing chunks in parallel, retrying each from its exact
    ///    offset and splitting slow ranges between idle connections
    /// 4. Falls back to a single stream if the server doesn't support ranges
    /// 5. Verifies size (and SHA-256), then renames `.part` to the final name
    ///
    /// It blocks the calling thread. Calling it from async code works (the
    /// download then runs on a helper thread), but prefer `spawn_blocking`.
    ///
    /// # Errors
    ///
    /// - Network failures after the retries are exhausted (state is kept for resume)
    /// - HTTP error status (a boxed [`KgetError::Http`] / [`KgetError::NotFound`])
    /// - Cancellation via the cancel token ([`KgetError::Cancelled`])
    /// - Size or checksum mismatch, disk I/O errors
    pub fn download(&self) -> Result<(), BoxError> {
        if tokio::runtime::Handle::try_current().is_ok() {
            // Inside a runtime, `block_on` would panic and the blocking parts
            // (single-stream fallback, prompts, hashing) would stall a worker.
            return std::thread::scope(|s| {
                s.spawn(|| self.download_blocking())
                    .join()
                    .unwrap_or_else(|p| std::panic::resume_unwind(p))
            });
        }
        self.download_blocking()
    }

    fn download_blocking(&self) -> Result<(), BoxError> {
        let is_iso = self.url.to_lowercase().ends_with(".iso");
        self.log(&format!("Starting advanced download for: {}", self.url));
        if is_iso {
            self.log("ISO mode active: data is written byte-for-byte.");
        }

        if let Some(parent) = Path::new(&self.output_path)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }

        self.log("Querying server for file size and range support...");
        let (client, remote) = self.probe_main()?;
        self.log(&format!(
            "Total file size: {}",
            remote
                .size
                .map(|s| format!("{s} bytes"))
                .unwrap_or_else(|| "unknown".into())
        ));
        self.log(&format!(
            "Server supports range requests: {}",
            remote.accepts_ranges
        ));

        let Some(total_size) = remote.size.filter(|_| remote.accepts_ranges) else {
            self.log("Range requests not supported, falling back to single-threaded download");
            return self.download_single_stream(is_iso);
        };

        let part = self.part_path();
        let state_path = self.state_path();
        let state = match ResumeState::load(&state_path) {
            Some(s)
                if self.resume_policy != ResumePolicy::AlwaysRestart
                    && part.exists()
                    && s.matches(&self.url, &remote) =>
            {
                self.send_status(&format!(
                    "Resuming previous download ({} of {} bytes already done)",
                    s.done_bytes(),
                    total_size
                ));
                s
            }
            _ => {
                if self.resume_policy != ResumePolicy::AlwaysRestart && state_path.exists() {
                    self.log(
                        "Previous partial download does not match the remote file; restarting",
                    );
                }
                crate::download::check_disk_space(&part, total_size)?;
                let f = File::create(&part)?;
                // Preallocation is safe: the state file says what is missing.
                f.set_len(total_size)?;
                // With mirrors, chunks come from servers with different
                // validators; the size and the expected hash identify the file.
                let single = self.mirrors.is_empty();
                ResumeState {
                    version: STATE_VERSION,
                    url: self.url.clone(),
                    total_size,
                    etag: remote.etag.clone().filter(|_| single),
                    last_modified: remote.last_modified.clone().filter(|_| single),
                    chunks: self
                        .calculate_chunks(total_size)
                        .into_iter()
                        .map(|(start, end)| ChunkState {
                            start,
                            end,
                            done: 0,
                        })
                        .collect(),
                }
            }
        };
        state.save(&state_path)?;

        let file = File::options().read(true).write(true).open(&part)?;
        if file.metadata()?.len() != total_size {
            file.set_len(total_size)?;
        }
        let file = Arc::new(file);

        let remaining = total_size - state.done_bytes();
        let workers = self
            .optimizer
            .max_connections()
            .min(remaining.div_ceil(MIN_SPLIT).max(1) as usize)
            .max(1);
        let sources = self.build_sources(client, remote, workers)?;

        let result = self.download_chunks(file.clone(), state, &state_path, sources, workers);
        if let Err(e) = result {
            if e.is::<RangeNotHonored>() {
                // Never splice two versions: drop the partial data and fetch
                // the whole (possibly new) file in one stream.
                drop(file);
                self.log(&format!("{e}; restarting with a single connection"));
                let _ = std::fs::remove_file(&part);
                let _ = std::fs::remove_file(&state_path);
                return self.download_single_stream(is_iso);
            }
            if matches!(e.downcast_ref::<KgetError>(), Some(KgetError::Cancelled)) {
                self.log("Download paused; run the same command again to resume.");
            }
            return Err(e);
        }
        file.sync_all()?;
        drop(file);

        self.finish(&part, total_size, is_iso)?;
        let _ = std::fs::remove_file(&state_path);
        Ok(())
    }

    /// Probe the main URL; an HTTP/3 failure falls back to HTTP/2 / HTTP/1.1.
    fn probe_main(&self) -> Result<(Conn, RemoteInfo), BoxError> {
        let rt = runtime()?;
        let h3 = self.http_version == HttpVersion::Http3
            && HttpVersion::http3_available()
            && !self.proxy.enabled;
        let conn = Conn {
            client: self.client.clone(),
            h3,
        };
        match rt.block_on(probe(&conn, &self.url, &self.extra_headers)) {
            Ok(info) => Ok((conn, info)),
            Err(e) if h3 => {
                self.send_status(&format!(
                    "HTTP/3 connection failed ({e}); falling back to HTTP/2 / HTTP/1.1"
                ));
                let client = http::build_async_client(
                    &self.proxy,
                    &self.client_options().with_http_version(HttpVersion::Auto),
                )?;
                let conn = Conn { client, h3: false };
                let info = rt.block_on(probe(&conn, &self.url, &self.extra_headers))?;
                Ok((conn, info))
            }
            Err(e) => Err(e),
        }
    }

    /// The main URL plus every usable mirror, each with its connections.
    fn build_sources(
        &self,
        client: Conn,
        main: RemoteInfo,
        workers: usize,
    ) -> Result<Vec<Source>, BoxError> {
        let mut found = vec![(self.url.clone(), main.clone(), client.clone())];
        if !self.mirrors.is_empty() {
            let probes = runtime()?.block_on(async {
                let mut set = JoinSet::new();
                for (i, url) in self.mirrors.iter().cloned().enumerate() {
                    let (client, headers) = (client.clone(), self.extra_headers.clone());
                    set.spawn(async move {
                        let r = probe(&client, &url, &headers).await;
                        (i, url, r)
                    });
                }
                let mut out = Vec::new();
                while let Some(Ok(r)) = set.join_next().await {
                    out.push(r);
                }
                out.sort_by_key(|(i, _, _)| *i);
                out
            });
            for (_, url, r) in probes {
                match r {
                    Ok(info) if info.size == main.size && info.accepts_ranges => {
                        found.push((url, info, client.clone()));
                    }
                    Ok(_) => self.send_status(&format!(
                        "Skipping mirror {url}: different size or no range support"
                    )),
                    Err(e) => self.send_status(&format!("Skipping mirror {url}: {e}")),
                }
            }
            self.send_status(&format!(
                "Downloading from {} source(s) in parallel",
                found.len()
            ));
        }

        let n = found.len();
        Ok(found
            .into_iter()
            .enumerate()
            .map(|(i, (url, remote, client))| {
                let share = (workers / n + usize::from(i < workers % n)).max(1);
                let mut clients = vec![client];
                if remote.is_multiplexed() {
                    // Several streams per connection, but more than one TCP
                    // connection so a single congestion window is not the limit.
                    // The extra clients skip negotiation, so each opens exactly
                    // one connection.
                    let conns = share.div_ceil(H2_STREAMS_PER_CONN);
                    let pref = if remote.version == reqwest::Version::HTTP_3 {
                        HttpVersion::Http3
                    } else {
                        HttpVersion::Http2PriorKnowledge
                    };
                    let opts = self.client_options().with_http_version(pref);
                    for _ in 1..conns {
                        match http::build_async_client(&self.proxy, &opts) {
                            Ok(client) => clients.push(Conn {
                                client,
                                h3: pref == HttpVersion::Http3,
                            }),
                            Err(_) => break,
                        }
                    }
                    self.send_status(&format!(
                        "Server negotiated {}: {share} stream(s) over {} connection(s)",
                        version_name(remote.version),
                        clients.len()
                    ));
                }
                Source {
                    url,
                    remote,
                    clients,
                    alive: AtomicBool::new(true),
                }
            })
            .collect())
    }

    /// Server without range support (or unknown size): one stream, still via `.part`.
    fn download_single_stream(&self, is_iso: bool) -> Result<(), BoxError> {
        let _ = std::fs::remove_file(self.state_path());
        let progress_cb = self.progress_callback.clone();
        let byte_cb = self.byte_progress_callback.clone();
        let options = DownloadOptions {
            quiet_mode: self.quiet_mode,
            output_path: Some(self.output_path.clone()),
            verify_iso: is_iso && self.should_verify_iso(),
            expected_sha256: self.expected_sha256.clone(),
            extra_headers: self.extra_headers.clone(),
            resume: false,
            retries: self.max_retries,
            timeout_secs: self.timeout_secs,
            cancel: Some(self.cancel_token.clone()),
            progress_callback: Some(Arc::new(move |bytes, total| {
                if let (Some(cb), Some(t)) = (&progress_cb, total) {
                    cb((bytes as f64 / t.max(1) as f64) as f32);
                }
                if let Some(cb) = &byte_cb {
                    cb(bytes, total);
                }
            })),
            ..Default::default()
        };
        let status_cb = self.status_callback.clone();
        let cb = move |msg: String| {
            if let Some(cb) = &status_cb
                && !msg.starts_with("PROGRESS:")
            {
                cb(msg);
            }
        };
        crate::download::download_to_path(
            &self.url,
            self.proxy.clone(),
            self.optimizer.clone(),
            options,
            Some(&cb),
        )?;
        self.send_status("Advanced download completed successfully!");
        Ok(())
    }

    /// Whether to compute the SHA-256 of an ISO when no expected hash was given.
    fn should_verify_iso(&self) -> bool {
        if self.expected_sha256.is_some() {
            return true;
        }
        match self.resume_policy {
            ResumePolicy::AlwaysResume => true,
            ResumePolicy::AlwaysRestart => false,
            ResumePolicy::Ask => {
                if self.status_callback.is_some() {
                    return true;
                }
                if self.quiet_mode || !std::io::stdin().is_terminal() {
                    return false;
                }
                println!("\nThis is an ISO file. Would you like to verify its integrity? (y/N)");
                let mut input = String::new();
                std::io::stdin().read_line(&mut input).is_ok()
                    && input.trim().eq_ignore_ascii_case("y")
            }
        }
    }

    /// Check the size, verify the hash, and move `.part` to the destination.
    fn finish(&self, part: &Path, expected: u64, is_iso: bool) -> Result<(), BoxError> {
        let len = std::fs::metadata(part)?.len();
        if len != expected {
            return Err(
                format!("File size mismatch: expected {expected} bytes, got {len} bytes").into(),
            );
        }
        self.send_status(&format!("File size verified: {len} bytes"));

        if self.expected_sha256.is_some() || (is_iso && self.should_verify_iso()) {
            // verify_file_sha256 announces the verifying phase itself.
            let status_cb = self.status_callback.clone();
            let cb = move |msg: String| {
                if let Some(cb) = &status_cb {
                    cb(msg);
                }
            };
            if let Err(e) = crate::download::verify_file_sha256(
                part,
                self.expected_sha256.as_deref(),
                Some(&cb),
            ) {
                let _ = std::fs::remove_file(part);
                let _ = std::fs::remove_file(self.state_path());
                return Err(e);
            }
            self.send_status("Integrity check passed - file is not corrupted");
        }

        std::fs::rename(part, &self.output_path)?;
        self.send_status("Advanced download completed successfully!");
        Ok(())
    }

    fn calculate_chunks(&self, total_size: u64) -> Vec<(u64, u64)> {
        let parallelism = self.optimizer.max_connections().max(1) as u64;
        // Keep workers fed without overwhelming servers.
        let target_chunks = parallelism.saturating_mul(2).max(2);
        let chunk_size = (total_size / target_chunks).clamp(MIN_CHUNK_SIZE, MAX_CHUNK_SIZE);

        let mut chunks = Vec::new();
        let mut start = 0;
        while start < total_size {
            let end = (start + chunk_size).min(total_size);
            chunks.push((start, end));
            start = end;
        }
        chunks
    }

    fn download_chunks(
        &self,
        file: Arc<File>,
        state: ResumeState,
        state_path: &Path,
        sources: Vec<Source>,
        workers: usize,
    ) -> Result<(), BoxError> {
        let total_size = state.total_size;
        let initial = state.done_bytes();
        let pending = state
            .chunks
            .iter()
            .filter(|c| c.start + c.done < c.end)
            .count();

        let bar = ProgressBar::new(total_size);
        bar.set_position(initial);
        if self.quiet_mode {
            bar.set_draw_target(indicatif::ProgressDrawTarget::hidden());
        } else {
            bar.set_style(
                ProgressStyle::with_template(
                    "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})",
                )
                .unwrap_or_else(|_| ProgressStyle::default_bar())
                .progress_chars("#>-"),
            );
        }
        self.log(&format!(
            "Downloading {} of {} chunks with {} connection(s)",
            pending,
            state.chunks.len(),
            workers
        ));

        let engine = Arc::new(Engine {
            dl: self.clone(),
            file,
            sched: Scheduler {
                slots: Mutex::new(
                    state
                        .chunks
                        .iter()
                        .map(|c| Slot::new(c.start, c.end, c.done))
                        .collect(),
                ),
                steals: AtomicU64::new(0),
            },
            sources,
            source_lock: Mutex::new(()),
            downloaded: AtomicU64::new(initial),
            throttle: self
                .optimizer
                .speed_limit
                .map(|limit| Mutex::new(TokenBucket::new(limit))),
            stop: watch::Sender::new(false),
            released: tokio::sync::Notify::new(),
            finished: AtomicBool::new(false),
            discard_state: AtomicBool::new(false),
            bar: bar.clone(),
            total_size,
            base_state: state,
            state_path: state_path.to_path_buf(),
        });

        let result = runtime()?.block_on(engine.clone().run(workers));

        // Always persist the final state so an interrupted run can resume.
        let final_state = engine.snapshot();
        if engine.discard_state.load(Ordering::Relaxed) {
            let _ = std::fs::remove_file(state_path);
        } else {
            let _ = final_state.save(state_path);
        }
        engine.report();
        let steals = engine.sched.steals.load(Ordering::Relaxed);
        if steals > 0 {
            self.log(&format!("Work stealing split {steals} slow range(s)"));
        }

        match result {
            Ok(()) => {
                if final_state.chunks.iter().any(|c| c.start + c.done < c.end) {
                    bar.abandon();
                    return Err("Download incomplete; run again to resume".into());
                }
                bar.finish_with_message("Download completed");
                Ok(())
            }
            Err(e) => {
                bar.abandon();
                Err(e)
            }
        }
    }
}

// ============================================================================
// Async chunk engine
// ============================================================================

/// One byte range of the file. `end` only ever shrinks (work stealing).
struct Slot {
    start: u64,
    end: AtomicU64,
    /// Bytes written to disk starting at `start`
    done: AtomicU64,
    /// A worker owns the range
    active: AtomicBool,
    /// When the current owner claimed the range, and `done` at that moment
    claim: Mutex<(Instant, u64)>,
}

impl Slot {
    fn new(start: u64, end: u64, done: u64) -> Arc<Self> {
        Arc::new(Self {
            start,
            end: AtomicU64::new(end),
            done: AtomicU64::new(done),
            active: AtomicBool::new(false),
            claim: Mutex::new((Instant::now(), done)),
        })
    }

    /// Offset of the first byte not yet written.
    fn pos(&self) -> u64 {
        self.start + self.done.load(Ordering::Acquire)
    }

    fn remaining(&self) -> u64 {
        self.end.load(Ordering::Acquire).saturating_sub(self.pos())
    }

    fn claim(&self) {
        self.active.store(true, Ordering::Release);
        *guard::lock(&self.claim) = (Instant::now(), self.done.load(Ordering::Acquire));
    }
}

enum Next {
    Work(Arc<Slot>),
    /// Nothing to take now; work in flight may become stealable
    Wait,
    Done,
}

struct Scheduler {
    slots: Mutex<Vec<Arc<Slot>>>,
    steals: AtomicU64,
}

impl Scheduler {
    /// The next range for an idle worker: an unclaimed one, else half of the
    /// in-flight range expected to finish last.
    fn next(&self) -> Next {
        let mut slots = guard::lock(&self.slots);
        if let Some(s) = slots
            .iter()
            .find(|s| !s.active.load(Ordering::Acquire) && s.remaining() > 0)
        {
            s.claim();
            return Next::Work(s.clone());
        }

        let now = Instant::now();
        let mut victim: Option<(f64, &Arc<Slot>)> = None;
        for s in slots.iter().filter(|s| s.active.load(Ordering::Acquire)) {
            let remaining = s.remaining();
            if remaining < 2 * MIN_SPLIT {
                continue;
            }
            let (at, done_then) = *guard::lock(&s.claim);
            let elapsed = now.duration_since(at);
            if elapsed < STEAL_GRACE {
                continue;
            }
            let progressed = s.done.load(Ordering::Acquire).saturating_sub(done_then);
            let eta = if progressed == 0 {
                f64::INFINITY
            } else {
                remaining as f64 * elapsed.as_secs_f64() / progressed as f64
            };
            if eta >= STEAL_MIN_ETA.as_secs_f64() && victim.is_none_or(|(best, _)| eta > best) {
                victim = Some((eta, s));
            }
        }

        if let Some((_, v)) = victim {
            // The owner has at most WRITE_BUF unwritten bytes past `pos`, and
            // `mid - pos >= MIN_SPLIT`, so the halves never overlap.
            let pos = v.pos();
            let end = v.end.load(Ordering::Acquire);
            let mid = pos + (end - pos) / 2;
            v.end.store(mid, Ordering::Release);
            let stolen = Slot::new(mid, end, 0);
            stolen.claim();
            slots.push(stolen.clone());
            self.steals.fetch_add(1, Ordering::Relaxed);
            return Next::Work(stolen);
        }

        if slots.iter().any(|s| s.active.load(Ordering::Acquire)) {
            Next::Wait
        } else {
            Next::Done
        }
    }
}

/// A URL serving the file (the main URL or a mirror).
struct Source {
    url: String,
    remote: RemoteInfo,
    /// One client per TCP connection for multiplexed servers, else one pool
    clients: Vec<Conn>,
    alive: AtomicBool,
}

enum ChunkError {
    /// Worth retrying on the same source
    Transient(String),
    /// The source is unusable (error status, range ignored); another mirror
    /// may still work
    Source { err: BoxError, discard_state: bool },
    /// Stop the whole download
    Fatal(BoxError),
}

struct Engine {
    dl: AdvancedDownloader,
    file: Arc<File>,
    sched: Scheduler,
    sources: Vec<Source>,
    source_lock: Mutex<()>,
    downloaded: AtomicU64,
    throttle: Option<Mutex<TokenBucket>>,
    /// Set on cancellation or when a worker failed: everyone stops
    stop: watch::Sender<bool>,
    /// A worker released its range (finished, or failed and stopped)
    released: tokio::sync::Notify,
    finished: AtomicBool,
    /// The remote file changed: the state must not be resumed
    discard_state: AtomicBool,
    bar: ProgressBar,
    total_size: u64,
    base_state: ResumeState,
    state_path: PathBuf,
}

impl Engine {
    async fn run(self: Arc<Self>, workers: usize) -> Result<(), BoxError> {
        let ticker = tokio::spawn(self.clone().ticker());
        let mut set = JoinSet::new();
        for id in 0..workers {
            set.spawn(self.clone().worker(id));
        }

        let mut first: Option<BoxError> = None;
        while let Some(joined) = set.join_next().await {
            let r = joined.unwrap_or_else(|e| Err(format!("download task failed: {e}").into()));
            if let Err(e) = r {
                self.stop.send_replace(true);
                // Report the root cause, not the cancellations it triggered.
                if first
                    .as_ref()
                    .is_none_or(|f| is_cancel(f) && !is_cancel(&e))
                {
                    first = Some(e);
                }
            }
        }
        self.finished.store(true, Ordering::Relaxed);
        let _ = ticker.await;
        first.map_or(Ok(()), Err)
    }

    /// Progress (UI/callbacks every 200 ms), cancellation polling, and the
    /// resume state persisted every second.
    async fn ticker(self: Arc<Self>) {
        let mut interval = tokio::time::interval(TICK);
        let start = Instant::now();
        let (mut last_report, mut last_print, mut last_save) = (start, start, start);
        while !self.finished.load(Ordering::Relaxed) {
            interval.tick().await;
            if self.dl.is_cancelled() {
                self.stop.send_replace(true);
            }
            if last_report.elapsed() >= Duration::from_millis(200) {
                self.report();
                last_report = Instant::now();
            }
            if !self.dl.quiet_mode && last_print.elapsed() >= Duration::from_millis(500) {
                let bytes = self.downloaded.load(Ordering::Relaxed);
                let total = self.total_size;
                let percent = (bytes as f64 / total.max(1) as f64 * 100.0).min(100.0);
                // Line-oriented format parsed by the macOS app
                self.bar
                    .suspend(|| println!("PROGRESS: {percent:.1}% ({bytes}/{total})"));
                last_print = Instant::now();
            }
            if last_save.elapsed() >= Duration::from_secs(1)
                && !self.discard_state.load(Ordering::Relaxed)
            {
                let (state, path) = (self.snapshot(), self.state_path.clone());
                let _ = tokio::task::spawn_blocking(move || state.save(&path)).await;
                last_save = Instant::now();
            }
        }
    }

    fn report(&self) {
        let bytes = self.downloaded.load(Ordering::Relaxed);
        self.bar.set_position(bytes);
        if let Some(cb) = &self.dl.progress_callback {
            cb((bytes as f64 / self.total_size.max(1) as f64) as f32);
        }
        if let Some(cb) = &self.dl.byte_progress_callback {
            cb(bytes, Some(self.total_size));
        }
    }

    fn snapshot(&self) -> ResumeState {
        let slots = guard::lock(&self.sched.slots);
        let mut chunks: Vec<ChunkState> = slots
            .iter()
            .map(|s| {
                let end = s.end.load(Ordering::Acquire);
                ChunkState {
                    start: s.start,
                    end,
                    done: s.done.load(Ordering::Acquire).min(end - s.start),
                }
            })
            .collect();
        chunks.sort_by_key(|c| c.start);
        ResumeState {
            chunks,
            ..self.base_state.clone()
        }
    }

    fn stopped(&self) -> bool {
        *self.stop.borrow() || self.dl.is_cancelled()
    }

    /// Sleep, waking early when the download stops.
    async fn sleep(&self, d: Duration, rx: &mut watch::Receiver<bool>) {
        tokio::select! {
            _ = tokio::time::sleep(d) => {}
            _ = rx.wait_for(|s| *s) => {}
        }
    }

    /// Live source for a worker: workers are spread over the live sources in
    /// order of preference.
    fn pick_source(&self, worker: usize) -> Option<usize> {
        let live: Vec<usize> = (0..self.sources.len())
            .filter(|&i| self.sources[i].alive.load(Ordering::Acquire))
            .collect();
        (!live.is_empty()).then(|| live[worker % live.len()])
    }

    /// Drop a failing source if another one is still alive.
    fn drop_source(&self, idx: usize, reason: &str) -> bool {
        let _guard = guard::lock(&self.source_lock);
        let live = self
            .sources
            .iter()
            .filter(|s| s.alive.load(Ordering::Acquire))
            .count();
        let src = &self.sources[idx];
        if !src.alive.load(Ordering::Acquire) {
            // Another worker already dropped it.
            return live > 0;
        }
        if live <= 1 {
            return false;
        }
        src.alive.store(false, Ordering::Release);
        self.dl
            .send_status(&format!("Mirror dropped: {} ({reason})", src.url));
        true
    }

    async fn worker(self: Arc<Self>, id: usize) -> Result<(), BoxError> {
        let mut rx = self.stop.subscribe();
        loop {
            if self.stopped() {
                return Err(Box::new(KgetError::Cancelled));
            }
            // Registered before asking, so a range released meanwhile still wakes us.
            let released = self.released.notified();
            tokio::pin!(released);
            released.as_mut().enable();
            match self.sched.next() {
                Next::Work(slot) => {
                    let r = self.fetch_slot(id, &slot, &mut rx).await;
                    slot.active.store(false, Ordering::Release);
                    self.released.notify_waiters();
                    r?;
                }
                Next::Wait => {
                    // Re-check periodically: in-flight ranges become stealable.
                    tokio::select! {
                        _ = released => {}
                        _ = self.sleep(Duration::from_millis(200), &mut rx) => {}
                    }
                }
                Next::Done => return Ok(()),
            }
        }
    }

    /// Download one range, resuming from the last written byte on every retry
    /// and moving to another mirror when its source fails.
    async fn fetch_slot(
        &self,
        id: usize,
        slot: &Slot,
        rx: &mut watch::Receiver<bool>,
    ) -> Result<(), BoxError> {
        let mut failures = 0u32;
        loop {
            if slot.remaining() == 0 {
                return Ok(());
            }
            if self.stopped() {
                return Err(Box::new(KgetError::Cancelled));
            }
            let Some(src) = self.pick_source(id) else {
                return Err("All mirrors failed".into());
            };
            let clients = &self.sources[src].clients;
            let client = &clients[(id / self.sources.len()) % clients.len()];

            let before = slot.pos();
            let r = self.attempt(src, client, slot, rx).await;
            if slot.pos() > before {
                // Progress was made: reset the failure counter.
                failures = 0;
            }
            match r {
                Ok(()) => {}
                Err(ChunkError::Fatal(e)) => return Err(e),
                Err(ChunkError::Source { err, discard_state }) => {
                    if self.drop_source(src, &err.to_string()) {
                        failures = 0;
                        continue;
                    }
                    if discard_state {
                        self.discard_state.store(true, Ordering::Relaxed);
                    }
                    return Err(err);
                }
                Err(ChunkError::Transient(msg)) => {
                    failures += 1;
                    if failures >= self.dl.max_retries {
                        let err = format!(
                            "Chunk {}-{} failed after {failures} attempts: {msg}",
                            slot.start,
                            slot.end.load(Ordering::Acquire)
                        );
                        if self.drop_source(src, &err) {
                            failures = 0;
                            continue;
                        }
                        return Err(err.into());
                    }
                    self.sleep(http::backoff_delay(failures), rx).await;
                }
            }
        }
    }

    /// One range request from the current position to the slot's end.
    async fn attempt(
        &self,
        src_idx: usize,
        client: &Conn,
        slot: &Slot,
        rx: &mut watch::Receiver<bool>,
    ) -> Result<(), ChunkError> {
        let src = &self.sources[src_idx];
        let start = slot.pos();
        let end = slot.end.load(Ordering::Acquire);
        if start >= end {
            return Ok(());
        }
        let mut req = http::apply_headers_async(client.get(&src.url), &self.dl.extra_headers)
            .header(
                reqwest::header::RANGE,
                format!("bytes={}-{}", start, end - 1),
            );
        // If-Range: if the file changed on the server it answers 200 instead of 206
        if let Some(v) = src.remote.if_range() {
            req = req.header(reqwest::header::IF_RANGE, v);
        }
        let sent = tokio::select! {
            biased;
            _ = rx.wait_for(|s| *s) => return Err(ChunkError::Fatal(Box::new(KgetError::Cancelled))),
            r = req.send() => r,
        };
        let mut resp = sent.map_err(|e| ChunkError::Transient(e.to_string()))?;
        let status = resp.status();
        if status == reqwest::StatusCode::OK || status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE
        {
            // Range ignored, `If-Range` says the file changed, or the range
            // no longer exists: another mirror may still serve ranges, else
            // the download restarts as a single stream (never splice versions).
            return Err(ChunkError::Source {
                err: Box::new(RangeNotHonored(status.as_u16())),
                discard_state: true,
            });
        }
        if http::is_transient_status(status.as_u16()) {
            if let Some(delay) = http::retry_after_header(resp.headers()) {
                self.sleep(delay, rx).await;
            }
            return Err(ChunkError::Transient(format!("HTTP {status}")));
        }
        if status != reqwest::StatusCode::PARTIAL_CONTENT {
            return Err(ChunkError::Source {
                err: Box::new(KgetError::from_status(status.as_u16(), &src.url)),
                discard_state: false,
            });
        }

        let mut buf: Vec<u8> = Vec::with_capacity(WRITE_BUF);
        // Offset of the next received byte (written bytes + buffered bytes)
        let mut pos = start;
        let result = 'read: loop {
            let end = slot.end.load(Ordering::Acquire);
            if pos >= end {
                break Ok(());
            }
            let chunk = tokio::select! {
                biased;
                _ = rx.wait_for(|s| *s) => break Err(ChunkError::Fatal(Box::new(KgetError::Cancelled))),
                c = resp.chunk() => c,
            };
            let data = match chunk {
                Ok(Some(d)) => d,
                Ok(None) => {
                    break Err(ChunkError::Transient(format!(
                        "connection closed early at byte {pos} (chunk ends at {end})"
                    )));
                }
                Err(e) => break Err(ChunkError::Transient(e.to_string())),
            };
            let mut rest = &data[..];
            let mut taken = 0u64;
            while !rest.is_empty() {
                // Re-read the end: work stealing may have shrunk the range.
                let room = slot.end.load(Ordering::Acquire).saturating_sub(pos);
                if room == 0 {
                    break;
                }
                // Saturate: on 32-bit targets `room` may not fit in a usize.
                let room = usize::try_from(room).unwrap_or(usize::MAX);
                let n = rest.len().min(WRITE_BUF - buf.len()).min(room);
                buf.extend_from_slice(&rest[..n]);
                rest = &rest[n..];
                pos += n as u64;
                taken += n as u64;
                if buf.len() == WRITE_BUF
                    && let Err(e) = self.flush(slot, &mut buf).await
                {
                    break 'read Err(ChunkError::Fatal(e));
                }
            }
            if let Some(bucket) = &self.throttle {
                let wait = guard::lock(bucket).consume(taken);
                if let Some(d) = wait {
                    self.sleep(d, rx).await;
                }
            }
        };
        // Bytes already received are valid: write them whatever happened.
        if let Err(e) = self.flush(slot, &mut buf).await {
            return Err(ChunkError::Fatal(e));
        }
        result
    }

    /// Write the buffered bytes at the slot's position and account for them.
    async fn flush(&self, slot: &Slot, buf: &mut Vec<u8>) -> Result<(), BoxError> {
        if buf.is_empty() {
            return Ok(());
        }
        let data = std::mem::replace(buf, Vec::with_capacity(WRITE_BUF));
        let n = data.len() as u64;
        let offset = slot.pos();
        // A positional write into the page cache is short; block_in_place lets
        // the runtime move other tasks off this worker meanwhile, without the
        // thread hop (and copy of ownership) of spawn_blocking.
        tokio::task::block_in_place(|| write_all_at(&self.file, &data, offset))
            .map_err(|e| Box::new(KgetError::Io(e)) as BoxError)?;
        slot.done.fetch_add(n, Ordering::Release);
        self.downloaded.fetch_add(n, Ordering::Relaxed);
        Ok(())
    }
}

fn is_cancel(e: &BoxError) -> bool {
    matches!(e.downcast_ref::<KgetError>(), Some(KgetError::Cancelled))
}

/// `write_at`/`seek_write` may write fewer bytes than requested; write everything.
fn write_all_at(file: &File, mut buf: &[u8], mut offset: u64) -> std::io::Result<()> {
    while !buf.is_empty() {
        #[cfg(target_family = "unix")]
        let n = file.write_at(buf, offset)?;
        #[cfg(target_family = "windows")]
        let n = file.seek_write(buf, offset)?;
        if n == 0 {
            return Err(std::io::ErrorKind::WriteZero.into());
        }
        buf = &buf[n..];
        offset += n as u64;
    }
    Ok(())
}

fn parse_content_range_total(value: &str) -> Option<u64> {
    let (_, total) = value.rsplit_once('/')?;
    if total == "*" {
        return None;
    }
    total.trim().parse::<u64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(chunks: &[(u64, u64, u64)], total: u64) -> ResumeState {
        ResumeState {
            version: STATE_VERSION,
            url: "u".into(),
            total_size: total,
            etag: None,
            last_modified: None,
            chunks: chunks
                .iter()
                .map(|&(start, end, done)| ChunkState { start, end, done })
                .collect(),
        }
    }

    /// Live turbo download over HTTP/3:
    /// `RUSTFLAGS="--cfg reqwest_unstable" cargo test --features http3 --lib -- --ignored live_turbo_http3`
    #[cfg(all(feature = "http3", reqwest_unstable))]
    #[test]
    #[ignore = "needs network access to an HTTP/3 server"]
    fn live_turbo_http3() {
        let dir = std::env::temp_dir().join(format!("kget-h3-{}", std::process::id()));
        let out = dir.join("h3.bin");
        let mut dl = AdvancedDownloader::new(
            "https://www.google.com/robots.txt".into(),
            out.to_string_lossy().into_owned(),
            true,
            ProxyConfig::default(),
            Optimizer::new(),
        )
        .unwrap();
        dl.set_http_version(HttpVersion::Http3).unwrap();
        let log: Arc<Mutex<Vec<String>>> = Arc::default();
        let l = log.clone();
        dl.set_status_callback(move |m| l.lock().unwrap().push(m));
        dl.download().unwrap();
        assert!(std::fs::metadata(&out).unwrap().len() > 0);
        let _ = std::fs::remove_dir_all(&dir);
        let log = log.lock().unwrap();
        assert!(!log.iter().any(|m| m.contains("falling back")), "{log:?}");
        assert!(
            log.iter().any(|m| m.contains("negotiated HTTP/3")),
            "{log:?}"
        );
    }

    #[test]
    fn state_must_cover_the_file_exactly() {
        assert!(state(&[(0, 5, 5), (5, 10, 0)], 10).is_consistent());
        assert!(state(&[(0, 3, 1), (3, 6, 2), (6, 10, 0)], 10).is_consistent());
        // Splits are appended to the scheduler out of order, but
        // `Engine::snapshot` sorts them before saving, so a file with
        // unordered chunks was not written by KGet: never trust it.
        assert!(!state(&[(0, 3, 1), (6, 10, 0), (3, 6, 2)], 10).is_consistent());
        assert!(!state(&[(0, 5, 5)], 10).is_consistent(), "gap at the end");
        assert!(!state(&[], 10).is_consistent(), "no chunks");
        assert!(
            !state(&[(0, 6, 0), (5, 10, 0)], 10).is_consistent(),
            "overlap"
        );
        assert!(
            !state(&[(0, 5, 6), (5, 10, 0)], 10).is_consistent(),
            "overfull"
        );
        let huge = state(&[(0, 5, 5), (5, u64::MAX, u64::MAX - 5)], u64::MAX);
        assert_eq!(huge.done_bytes(), u64::MAX, "saturating sum");
    }

    #[test]
    fn idle_worker_takes_unclaimed_range_first() {
        let sched = Scheduler {
            slots: Mutex::new(vec![Slot::new(0, 10, 10), Slot::new(10, 20, 3)]),
            steals: AtomicU64::new(0),
        };
        match sched.next() {
            Next::Work(s) => assert_eq!(s.pos(), 13),
            _ => panic!("expected work"),
        }
        // The only range left is in flight and small: nothing to steal.
        assert!(matches!(sched.next(), Next::Wait));
    }

    #[test]
    fn stalled_range_is_halved() {
        let big = 8 * MIN_SPLIT;
        let slot = Slot::new(0, big, 0);
        slot.claim();
        *slot.claim.lock().unwrap() = (Instant::now() - STEAL_GRACE * 2, 0);
        let sched = Scheduler {
            slots: Mutex::new(vec![slot.clone()]),
            steals: AtomicU64::new(0),
        };
        let Next::Work(stolen) = sched.next() else {
            panic!("expected a split");
        };
        assert_eq!(slot.end.load(Ordering::Relaxed), big / 2);
        assert_eq!(
            (stolen.start, stolen.end.load(Ordering::Relaxed)),
            (big / 2, big)
        );
        assert_eq!(sched.steals.load(Ordering::Relaxed), 1);
        slot.active.store(false, Ordering::Relaxed);
        stolen.done.store(big / 2, Ordering::Relaxed);
        stolen.active.store(false, Ordering::Relaxed);
        // The first half is unclaimed again and still incomplete.
        assert!(matches!(sched.next(), Next::Work(_)));
    }
}
