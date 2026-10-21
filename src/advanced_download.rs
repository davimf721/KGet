//! Advanced parallel download functionality with resume support.
//!
//! The [`AdvancedDownloader`] provides high-performance downloads using:
//! - **Parallel connections**: Split files into chunks downloaded simultaneously
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
//! (`max_connections`, 1–32). Each connection runs on its own thread, so the
//! count is not limited by the number of CPU cores.

use crate::DownloadOptions;
use crate::config::ProxyConfig;
use crate::error::KgetError;
use crate::http::{self, ClientOptions};
use crate::optimization::Optimizer;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs::File;
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(target_family = "unix")]
use std::os::unix::fs::FileExt;
#[cfg(target_family = "windows")]
use std::os::windows::fs::FileExt;

type BoxError = Box<dyn Error + Send + Sync>;

/// Minimum chunk size for parallel downloads (4 MB)
const MIN_CHUNK_SIZE: u64 = 4 * 1024 * 1024;
/// Maximum chunk size for parallel downloads (64 MB)
const MAX_CHUNK_SIZE: u64 = 64 * 1024 * 1024;
/// Read buffer per connection
const READ_BUF: usize = 64 * 1024;
/// Version of the `.kget` state format
const STATE_VERSION: u32 = 1;

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

/// Token bucket for global download rate limiting across parallel threads.
///
/// Unlike per-thread throttling, this bounds aggregate throughput regardless of
/// how many threads are active.
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

    /// Consume `n` bytes from the bucket and return how long to sleep before writing.
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
struct ResumeState {
    version: u32,
    url: String,
    total_size: u64,
    etag: Option<String>,
    last_modified: Option<String>,
    chunks: Vec<ChunkState>,
}

impl ResumeState {
    fn load(path: &Path) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        serde_json::from_slice(&bytes).ok()
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
            && self.chunks.iter().all(|c| c.start + c.done <= c.end)
    }

    fn done_bytes(&self) -> u64 {
        self.chunks.iter().map(|c| c.done).sum()
    }
}

#[derive(Debug, Clone, Default)]
struct RemoteInfo {
    size: Option<u64>,
    accepts_ranges: bool,
    etag: Option<String>,
    last_modified: Option<String>,
}

impl RemoteInfo {
    /// Validator for `If-Range` (weak ETags are not allowed there).
    fn if_range(&self) -> Option<&str> {
        self.etag
            .as_deref()
            .filter(|e| !e.starts_with("W/"))
            .or(self.last_modified.as_deref())
    }
}

enum ChunkError {
    Transient(String),
    Fatal(BoxError),
}

/// High-performance downloader with parallel connections and resume support.
///
/// `AdvancedDownloader` is the recommended way to download large files. It provides:
///
/// - **Parallel chunk downloads**: Splits files into segments downloaded simultaneously
/// - **Safe resume**: `<file>.part` + `<file>.kget` state, validated against the server
/// - **Server compatibility**: Falls back to single-stream if server doesn't support ranges
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
pub struct AdvancedDownloader {
    client: Client,
    url: String,
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
        let client = http::build_client(&proxy_config, &ClientOptions::default())?;
        Ok(Self {
            client,
            url,
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
        })
    }

    /// Set a custom cancellation token for graceful download interruption.
    ///
    /// When the token is set to `true`, the download stops at the next
    /// checkpoint, the resume state is saved and an error is returned. Running
    /// the same download again continues where it stopped.
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

    /// Maximum consecutive failed attempts per chunk without progress (default 3).
    pub fn set_retries(&mut self, retries: u32) {
        self.max_retries = retries.max(1);
    }

    /// Connect / per-read timeout in seconds (default 60, 0 = no read timeout).
    pub fn set_timeout_secs(&mut self, secs: u64) -> Result<(), BoxError> {
        self.timeout_secs = secs;
        self.client = http::build_client(&self.proxy, &ClientOptions::with_timeout_secs(secs))?;
        Ok(())
    }

    fn apply_headers(
        &self,
        req: reqwest::blocking::RequestBuilder,
    ) -> reqwest::blocking::RequestBuilder {
        http::apply_headers(req, &self.extra_headers)
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
    /// 3. Downloads the missing chunks in parallel, retrying each from its exact offset
    /// 4. Falls back to a single stream if the server doesn't support ranges
    /// 5. Verifies size (and SHA-256), then renames `.part` to the final name
    ///
    /// # Errors
    ///
    /// - Network failures after the retries are exhausted (state is kept for resume)
    /// - HTTP error status (a boxed [`KgetError::Http`] / [`KgetError::NotFound`])
    /// - Cancellation via the cancel token ([`KgetError::Cancelled`])
    /// - Size or checksum mismatch, disk I/O errors
    pub fn download(&self) -> Result<(), BoxError> {
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
        let remote = self.probe_remote()?;
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
                ResumeState {
                    version: STATE_VERSION,
                    url: self.url.clone(),
                    total_size,
                    etag: remote.etag.clone(),
                    last_modified: remote.last_modified.clone(),
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

        let result = self.download_chunks_parallel(&file, state, &state_path, &remote);
        match result {
            Ok(()) => {}
            Err(e) => {
                if matches!(e.downcast_ref::<KgetError>(), Some(KgetError::Cancelled)) {
                    self.log("Download paused; run the same command again to resume.");
                }
                return Err(e);
            }
        }
        file.sync_all()?;
        drop(file);

        self.finish(&part, total_size, is_iso)?;
        let _ = std::fs::remove_file(&state_path);
        Ok(())
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

    fn probe_remote(&self) -> Result<RemoteInfo, BoxError> {
        let header = |resp: &reqwest::blocking::Response, name: reqwest::header::HeaderName| {
            resp.headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };

        if let Ok(response) = self.apply_headers(self.client.head(&self.url)).send()
            && response.status().is_success()
        {
            let size = response.content_length().filter(|l| *l > 0).or_else(|| {
                header(&response, reqwest::header::CONTENT_LENGTH).and_then(|s| s.parse().ok())
            });
            let accepts_ranges = header(&response, reqwest::header::ACCEPT_RANGES)
                .is_some_and(|s| s.eq_ignore_ascii_case("bytes"));
            if size.is_some() && accepts_ranges {
                return Ok(RemoteInfo {
                    size,
                    accepts_ranges,
                    etag: header(&response, reqwest::header::ETAG),
                    last_modified: header(&response, reqwest::header::LAST_MODIFIED),
                });
            }
        }

        // HEAD failed or was inconclusive: probe with a 1-byte range request.
        let response = self
            .apply_headers(self.client.get(&self.url))
            .header(reqwest::header::RANGE, "bytes=0-0")
            .send()?;
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
            });
        }
        if status.is_success() {
            return Ok(RemoteInfo {
                size: response.content_length(),
                ..Default::default()
            });
        }
        Err(Box::new(KgetError::from_status(status.as_u16(), &self.url)))
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

    fn download_chunks_parallel(
        &self,
        file: &File,
        state: ResumeState,
        state_path: &Path,
        remote: &RemoteInfo,
    ) -> Result<(), BoxError> {
        let total_size = state.total_size;
        let chunks: Vec<(u64, u64)> = state.chunks.iter().map(|c| (c.start, c.end)).collect();
        let done: Vec<AtomicU64> = state
            .chunks
            .iter()
            .map(|c| AtomicU64::new(c.done))
            .collect();
        let pending: Vec<usize> = state
            .chunks
            .iter()
            .enumerate()
            .filter(|(_, c)| c.start + c.done < c.end)
            .map(|(i, _)| i)
            .collect();
        let initial: u64 = state.done_bytes();
        let downloaded = AtomicU64::new(initial);
        let finished = AtomicBool::new(false);

        let snapshot = || ResumeState {
            chunks: state
                .chunks
                .iter()
                .zip(&done)
                .map(|(c, d)| ChunkState {
                    start: c.start,
                    end: c.end,
                    done: d.load(Ordering::Relaxed),
                })
                .collect(),
            ..state.clone()
        };

        let bar = ProgressBar::new(total_size);
        bar.set_position(initial);
        if self.quiet_mode {
            bar.set_draw_target(indicatif::ProgressDrawTarget::hidden());
        } else {
            bar.set_style(
                ProgressStyle::with_template(
                    "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})",
                )
                .unwrap()
                .progress_chars("#>-"),
            );
        }

        let throttle_bucket = self
            .optimizer
            .speed_limit
            .map(|limit| Mutex::new(TokenBucket::new(limit)));

        let threads = self.optimizer.max_connections().min(pending.len()).max(1);
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|i| format!("kget-chunk-{i}"))
            .build()?;
        self.log(&format!(
            "Downloading {} of {} chunks with {} connection(s)",
            pending.len(),
            chunks.len(),
            threads
        ));

        let report = |bytes: u64| {
            bar.set_position(bytes);
            if let Some(cb) = &self.progress_callback {
                cb((bytes as f64 / total_size.max(1) as f64) as f32);
            }
            if let Some(cb) = &self.byte_progress_callback {
                cb(bytes, Some(total_size));
            }
        };

        let result = std::thread::scope(|scope| {
            // One ticker thread updates progress (UI/callbacks every 200 ms) and
            // persists the resume state every second.
            scope.spawn(|| {
                let mut last_save = Instant::now();
                let mut last_print = Instant::now();
                while !finished.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(200));
                    let bytes = downloaded.load(Ordering::Relaxed);
                    report(bytes);
                    if !self.quiet_mode && last_print.elapsed() >= Duration::from_millis(500) {
                        let percent = (bytes as f64 / total_size.max(1) as f64 * 100.0).min(100.0);
                        // Line-oriented format parsed by the macOS app
                        bar.suspend(|| println!("PROGRESS: {percent:.1}% ({bytes}/{total_size})"));
                        last_print = Instant::now();
                    }
                    if last_save.elapsed() >= Duration::from_secs(1) {
                        let _ = snapshot().save(state_path);
                        last_save = Instant::now();
                    }
                }
            });

            let r = pool.install(|| {
                pending.par_iter().try_for_each(|&idx| {
                    self.fetch_chunk(
                        file,
                        chunks[idx],
                        &done[idx],
                        &downloaded,
                        remote,
                        throttle_bucket.as_ref(),
                    )
                })
            });
            finished.store(true, Ordering::Relaxed);
            r
        });

        // Always persist the final state so an interrupted run can resume.
        let final_state = snapshot();
        let _ = final_state.save(state_path);
        let bytes = downloaded.load(Ordering::Relaxed);
        report(bytes);

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

    /// Download one chunk, resuming from the last written byte on every retry.
    fn fetch_chunk(
        &self,
        file: &File,
        (chunk_start, end): (u64, u64),
        done: &AtomicU64,
        downloaded: &AtomicU64,
        remote: &RemoteInfo,
        throttle: Option<&Mutex<TokenBucket>>,
    ) -> Result<(), BoxError> {
        let mut pos = chunk_start + done.load(Ordering::Relaxed);
        let mut failures = 0u32;

        while pos < end {
            if self.is_cancelled() {
                return Err(Box::new(KgetError::Cancelled));
            }

            let attempt: Result<(), ChunkError> = (|| {
                let mut req = self
                    .apply_headers(self.client.get(&self.url))
                    .header(reqwest::header::RANGE, format!("bytes={}-{}", pos, end - 1));
                // If-Range: if the file changed on the server it answers 200 instead of 206
                if let Some(v) = remote.if_range() {
                    req = req.header(reqwest::header::IF_RANGE, v);
                }
                let mut resp = req
                    .send()
                    .map_err(|e| ChunkError::Transient(e.to_string()))?;
                let status = resp.status();
                if status == reqwest::StatusCode::OK {
                    let _ = std::fs::remove_file(self.state_path());
                    return Err(ChunkError::Fatal(
                        "Server ignored the range request or the remote file changed; restart the download"
                            .into(),
                    ));
                }
                if http::is_transient_status(status.as_u16()) {
                    if let Some(delay) = http::retry_after(&resp) {
                        std::thread::sleep(delay);
                    }
                    return Err(ChunkError::Transient(format!("HTTP {status}")));
                }
                if status != reqwest::StatusCode::PARTIAL_CONTENT {
                    return Err(ChunkError::Fatal(Box::new(KgetError::from_status(
                        status.as_u16(),
                        &self.url,
                    ))));
                }

                let mut buf = vec![0u8; READ_BUF];
                while pos < end {
                    if self.is_cancelled() {
                        return Err(ChunkError::Fatal(Box::new(KgetError::Cancelled)));
                    }
                    let want = ((end - pos) as usize).min(buf.len());
                    let n = resp
                        .read(&mut buf[..want])
                        .map_err(|e| ChunkError::Transient(e.to_string()))?;
                    if n == 0 {
                        return Err(ChunkError::Transient(format!(
                            "connection closed early at byte {pos} (chunk ends at {end})"
                        )));
                    }
                    write_all_at(file, &buf[..n], pos)
                        .map_err(|e| ChunkError::Fatal(Box::new(KgetError::Io(e))))?;
                    pos += n as u64;
                    done.fetch_add(n as u64, Ordering::Relaxed);
                    downloaded.fetch_add(n as u64, Ordering::Relaxed);
                    // Progress was made: reset the failure counter.
                    failures = 0;

                    if let Some(bucket) = throttle {
                        let sleep_dur = bucket
                            .lock()
                            .expect("throttle bucket poisoned")
                            .consume(n as u64);
                        if let Some(dur) = sleep_dur {
                            std::thread::sleep(dur);
                        }
                    }
                }
                Ok(())
            })();

            match attempt {
                Ok(()) => {}
                Err(ChunkError::Fatal(e)) => return Err(e),
                Err(ChunkError::Transient(msg)) => {
                    failures += 1;
                    if failures >= self.max_retries {
                        return Err(format!(
                            "Chunk {chunk_start}-{end} failed after {failures} attempts: {msg}"
                        )
                        .into());
                    }
                    std::thread::sleep(http::backoff_delay(failures));
                }
            }
        }
        Ok(())
    }
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
