//! Fluent builder API — the primary interface for the KGet library.
//!
//! # Quick start
//!
//! ```rust,no_run
//! use kget::KgetError;
//!
//! // Single file
//! let result = kget::builder("https://example.com/file.zip")
//!     .output("./downloads/")
//!     .connections(8)
//!     .sha256("abc123...")
//!     .download()?;
//!
//! println!("Avg speed: {} B/s", result.avg_speed_bps);
//! # Ok::<(), KgetError>(())
//! ```
//!
//! # Batch
//!
//! ```rust,no_run
//! # use kget::KgetError;
//! let results = kget::batch(["https://a.com/f1.zip", "https://b.com/f2.iso"])
//!     .concurrency(4)
//!     .output_dir("./downloads/")
//!     .download_all();
//!
//! for r in &results {
//!     match &r.result {
//!         Ok(info) => println!("OK  {}", r.url),
//!         Err(e)   => eprintln!("ERR {}  — {}", r.url, e),
//!     }
//! }
//! ```

use crate::DownloadOptions;
use crate::advanced_download::AdvancedDownloader;
use crate::advanced_download::ResumePolicy;
use crate::checksum::{ChecksumAlgorithm, compute_checksum, parse_sidecar};
use crate::config::{Config, ProxyConfig, ProxyType};
use crate::download::download_to_path as http_download;
use crate::error::KgetError;
use crate::events::DownloadEvent;
use crate::http::ClientOptions;
use crate::optimization::Optimizer;
use crate::utils;
use std::collections::HashSet;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

// ════════════════════════════════════════════════════════════════════════════
// Retry configuration
// ════════════════════════════════════════════════════════════════════════════

/// Delay strategy between retry attempts.
#[derive(Debug, Clone)]
pub enum Backoff {
    /// Always wait the same fixed duration.
    Fixed(Duration),
    /// Double the delay each attempt, capped at `max_ms`.
    Exponential {
        /// Initial delay in milliseconds.
        base_ms: u64,
        /// Maximum delay cap in milliseconds.
        max_ms: u64,
    },
}

impl Backoff {
    fn delay(&self, attempt: u32) -> Duration {
        match self {
            Backoff::Fixed(d) => *d,
            Backoff::Exponential { base_ms, max_ms } => {
                let ms = base_ms.saturating_mul(1u64 << attempt.min(62));
                Duration::from_millis(ms.min(*max_ms))
            }
        }
    }
}

/// Controls how many times and how often a failed download is retried.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum total attempts (including the first).  Default: 3.
    pub max_attempts: u32,
    /// Delay strategy between retries.
    pub backoff: Backoff,
    /// HTTP status codes that should trigger a retry.
    /// Other HTTP errors (404, 403, …), checksum mismatches, I/O errors and
    /// cancellation abort immediately; network errors are always retried.
    pub retry_on_status: Vec<u16>,
}

impl Default for RetryConfig {
    fn default() -> Self {
        RetryConfig {
            max_attempts: 3,
            backoff: Backoff::Exponential {
                base_ms: 500,
                max_ms: 30_000,
            },
            retry_on_status: vec![408, 429, 500, 502, 503, 504],
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Download result
// ════════════════════════════════════════════════════════════════════════════

/// Digests that were computed (and optionally verified) after a download.
#[derive(Debug, Clone, Default)]
pub struct ComputedChecksums {
    pub sha256: Option<String>,
    pub sha512: Option<String>,
    pub sha1: Option<String>,
    pub md5: Option<String>,
    pub blake3: Option<String>,
}

/// Metrics and metadata returned after a successful download.
#[derive(Debug, Clone)]
pub struct DownloadResult {
    /// Absolute path of the saved file.  Empty for in-memory downloads.
    pub path: String,
    /// Total bytes written to disk.
    pub bytes_downloaded: u64,
    /// Average transfer speed in bytes per second.
    pub avg_speed_bps: u64,
    /// Wall-clock time from start to completion.
    pub duration: Duration,
    /// Number of parallel connections used.
    pub connections_used: usize,
    /// Checksums that were computed during or after the download.
    pub checksums: ComputedChecksums,
}

// ════════════════════════════════════════════════════════════════════════════
// DownloadBuilder internals
// ════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Default)]
struct ChecksumExpectations {
    sha256: Option<String>,
    sha512: Option<String>,
    sha1: Option<String>,
    md5: Option<String>,
    blake3: Option<String>,
}

impl ChecksumExpectations {
    fn any_set(&self) -> bool {
        self.sha256.is_some()
            || self.sha512.is_some()
            || self.sha1.is_some()
            || self.md5.is_some()
            || self.blake3.is_some()
    }
}

// ════════════════════════════════════════════════════════════════════════════
// DownloadBuilder
// ════════════════════════════════════════════════════════════════════════════

/// Fluent builder for a single download.
///
/// Create one via [`crate::builder()`] or [`DownloadBuilder::new()`].
pub struct DownloadBuilder {
    url: String,
    output: Option<String>,
    connections: usize,
    speed_limit: Option<u64>,
    proxy_url: Option<String>,
    proxy_user: Option<String>,
    proxy_pass: Option<String>,
    checksums: ChecksumExpectations,
    verify_from: Option<String>,
    headers: Vec<(String, String)>,
    retry: RetryConfig,
    range: Option<(u64, u64)>,
    quiet: bool,
    include: Vec<String>,
    exclude: Vec<String>,
    revision: Option<String>,
    hf_cache: bool,
    parallel_files: usize,
}

impl DownloadBuilder {
    /// Start building a download for `url`.
    pub fn new(url: impl Into<String>) -> Self {
        DownloadBuilder {
            url: url.into(),
            output: None,
            connections: 1,
            speed_limit: None,
            proxy_url: None,
            proxy_user: None,
            proxy_pass: None,
            checksums: ChecksumExpectations::default(),
            verify_from: None,
            headers: Vec::new(),
            retry: RetryConfig::default(),
            range: None,
            quiet: false,
            include: Vec::new(),
            exclude: Vec::new(),
            revision: None,
            hf_cache: false,
            parallel_files: 4,
        }
    }

    // ── Configuration ────────────────────────────────────────────────────────

    /// Set the output file path or directory.
    ///
    /// If a directory is given, the filename is inferred from the URL.
    pub fn output(mut self, path: impl Into<String>) -> Self {
        self.output = Some(path.into());
        self
    }

    /// Number of parallel HTTP connections.  Clamped to 1–32.
    pub fn connections(mut self, n: usize) -> Self {
        self.connections = n.clamp(1, 32);
        self
    }

    /// Global speed limit in bytes per second.
    pub fn speed_limit(mut self, bytes_per_sec: u64) -> Self {
        self.speed_limit = Some(bytes_per_sec);
        self
    }

    /// HTTP proxy URL (e.g. `"http://proxy:8080"` or `"socks5://host:1080"`).
    pub fn proxy(mut self, url: impl Into<String>) -> Self {
        self.proxy_url = Some(url.into());
        self
    }

    /// Proxy credentials (optional, required only for authenticated proxies).
    pub fn proxy_auth(mut self, user: impl Into<String>, pass: impl Into<String>) -> Self {
        self.proxy_user = Some(user.into());
        self.proxy_pass = Some(pass.into());
        self
    }

    /// Expect a SHA-256 digest.  The download fails if the file doesn't match.
    pub fn sha256(mut self, hash: impl Into<String>) -> Self {
        self.checksums.sha256 = Some(hash.into().to_lowercase());
        self
    }

    /// Expect a SHA-512 digest.
    pub fn sha512(mut self, hash: impl Into<String>) -> Self {
        self.checksums.sha512 = Some(hash.into().to_lowercase());
        self
    }

    /// Expect a SHA-1 digest.
    pub fn sha1(mut self, hash: impl Into<String>) -> Self {
        self.checksums.sha1 = Some(hash.into().to_lowercase());
        self
    }

    /// Expect an MD5 digest.
    pub fn md5(mut self, hash: impl Into<String>) -> Self {
        self.checksums.md5 = Some(hash.into().to_lowercase());
        self
    }

    /// Expect a BLAKE3 digest.
    pub fn blake3(mut self, hash: impl Into<String>) -> Self {
        self.checksums.blake3 = Some(hash.into().to_lowercase());
        self
    }

    /// Download a checksum sidecar file from `url` and use it to populate the
    /// expected digest automatically.
    ///
    /// Supports GNU (`<hash>  <file>`) and BSD (`SHA256 (<file>) = <hash>`) formats.
    pub fn verify_from(mut self, url: impl Into<String>) -> Self {
        self.verify_from = Some(url.into());
        self
    }

    /// Add a custom HTTP request header.  Can be called multiple times.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Override the default retry policy.
    pub fn retry(mut self, config: RetryConfig) -> Self {
        self.retry = config;
        self
    }

    /// Request only a byte range of the file: `[start, end]` (both inclusive).
    ///
    /// Sends `Range: bytes=start-end`.  The returned `DownloadResult` will have
    /// `bytes_downloaded` equal to `end - start + 1`.
    pub fn range(mut self, start: u64, end: u64) -> Self {
        self.range = Some((start, end));
        self
    }

    /// Suppress all progress output.
    pub fn quiet(mut self, q: bool) -> Self {
        self.quiet = q;
        self
    }

    /// `hf://` downloads: only fetch files matching this glob (repeatable).
    pub fn include(mut self, glob: impl Into<String>) -> Self {
        self.include.push(glob.into());
        self
    }

    /// `hf://` downloads: skip files matching this glob (repeatable).
    pub fn exclude(mut self, glob: impl Into<String>) -> Self {
        self.exclude.push(glob.into());
        self
    }

    /// `hf://` downloads: branch, tag or commit (overrides the URL's `@rev`).
    pub fn revision(mut self, rev: impl Into<String>) -> Self {
        self.revision = Some(rev.into());
        self
    }

    /// `hf://` downloads: write into the Hugging Face hub cache layout.
    pub fn hf_cache(mut self, enabled: bool) -> Self {
        self.hf_cache = enabled;
        self
    }

    /// `hf://` / `ollama://` downloads: files fetched at the same time.
    pub fn parallel_files(mut self, n: usize) -> Self {
        self.parallel_files = n.clamp(1, crate::fetch::MAX_PARALLEL_FILES);
        self
    }

    // ── Terminal methods ─────────────────────────────────────────────────────

    /// Execute the download synchronously and return metrics on success.
    ///
    /// `hf://` and `ollama://` URLs download every selected file; the result's
    /// `path` is then the directory (or the single file) that was written.
    pub fn download(mut self) -> Result<DownloadResult, KgetError> {
        if self.is_model() {
            return self.download_model(None);
        }
        // 1. Resolve sidecar before the main download so the hash is ready.
        if let Some(sidecar_url) = self.verify_from.take() {
            self.apply_sidecar(&sidecar_url)?;
        }

        let output_path = self.resolve_output();
        let proxy = self.make_proxy();
        let optimizer = self.make_optimizer();
        let start = Instant::now();

        // 2. Execute the download (with retry).
        let output_path = self.run_with_retry(&output_path, proxy, optimizer, None)?;

        let duration = start.elapsed();

        // 3. Verify checksums and collect digests.
        let checksums = self.verify_and_collect(Path::new(&output_path))?;

        // 4. Build result metrics.
        let bytes_downloaded = std::fs::metadata(&output_path)
            .map(|m| m.len())
            .unwrap_or(0);
        let avg_speed_bps = avg_speed(bytes_downloaded, duration);

        Ok(DownloadResult {
            path: output_path,
            bytes_downloaded,
            avg_speed_bps,
            duration,
            connections_used: self.connections,
            checksums,
        })
    }

    /// Download entirely into memory and return the raw bytes.
    ///
    /// Respects `.range()`, `.proxy()`, and `.header()` settings.
    /// Does **not** write to disk, so `.output()` is ignored.
    pub fn download_to_bytes(self) -> Result<Vec<u8>, KgetError> {
        let client = self.make_blocking_client()?;
        let mut req = client.get(&self.url);

        if let Some((s, e)) = self.range {
            req = req.header("Range", format!("bytes={s}-{e}"));
        }
        req = crate::http::apply_headers(req, &self.headers);

        let resp = req.send()?;
        if !resp.status().is_success() {
            return Err(KgetError::from_status(resp.status().as_u16(), &self.url));
        }
        Ok(resp.bytes()?.to_vec())
    }

    /// Download to memory and return an `impl Read`.
    ///
    /// Convenience wrapper over [`download_to_bytes`](Self::download_to_bytes).
    pub fn download_to_reader(self) -> Result<impl Read, KgetError> {
        self.download_to_bytes().map(Cursor::new)
    }

    /// Spawn the download in a background thread and return an event channel.
    ///
    /// The [`DownloadEvent`] receiver yields progress updates until the channel
    /// is closed (on completion or error).  Call `.join()` on the handle to
    /// retrieve the final [`DownloadResult`].
    pub fn spawn(
        mut self,
    ) -> (
        thread::JoinHandle<Result<DownloadResult, KgetError>>,
        mpsc::Receiver<DownloadEvent>,
    ) {
        let (tx, rx) = mpsc::channel::<DownloadEvent>();
        let handle = thread::spawn(move || {
            if self.is_model() {
                let result = self.download_model(Some(&tx));
                match &result {
                    Ok(r) => {
                        let _ = tx.send(DownloadEvent::Completed {
                            path: r.path.clone(),
                            sha256: None,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(DownloadEvent::Error(e.to_string()));
                    }
                }
                return result;
            }
            // Resolve sidecar
            if let Some(sidecar_url) = self.verify_from.take()
                && let Err(e) = self.apply_sidecar(&sidecar_url)
            {
                let _ = tx.send(DownloadEvent::Error(e.to_string()));
                return Err(e);
            }

            let output_path = self.resolve_output();
            let proxy = self.make_proxy();
            let optimizer = self.make_optimizer();
            let start = Instant::now();

            let result = self.run_with_retry(&output_path, proxy, optimizer, Some(&tx));

            match result {
                Ok(output_path) => {
                    let duration = start.elapsed();
                    let checksums = match self.verify_and_collect(Path::new(&output_path)) {
                        Ok(c) => c,
                        Err(e) => {
                            let _ = tx.send(DownloadEvent::Error(e.to_string()));
                            return Err(e);
                        }
                    };
                    let bytes_downloaded = std::fs::metadata(&output_path)
                        .map(|m| m.len())
                        .unwrap_or(0);
                    let _ = tx.send(DownloadEvent::Completed {
                        path: output_path.clone(),
                        sha256: checksums.sha256.clone(),
                    });
                    Ok(DownloadResult {
                        path: output_path,
                        bytes_downloaded,
                        avg_speed_bps: avg_speed(bytes_downloaded, duration),
                        duration,
                        connections_used: self.connections,
                        checksums,
                    })
                }
                Err(e) => {
                    let _ = tx.send(DownloadEvent::Error(e.to_string()));
                    Err(e)
                }
            }
        });
        (handle, rx)
    }

    /// Async version of [`download`](Self::download).
    ///
    /// Requires the `async` feature.  Runs the blocking download in a
    /// `tokio::task::spawn_blocking` thread so it doesn't block the executor.
    #[cfg(feature = "async")]
    pub async fn download_async(self) -> Result<DownloadResult, KgetError> {
        tokio::task::spawn_blocking(move || self.download())
            .await
            .map_err(|e| KgetError::Other(e.to_string()))?
    }

    // ── Private helpers ──────────────────────────────────────────────────────

    /// Whether the URL is a multi-file AI model source (`hf://`, `ollama://`).
    fn is_model(&self) -> bool {
        matches!(
            crate::protocol::detect(&self.url),
            crate::Protocol::HuggingFace | crate::Protocol::Ollama
        )
    }

    /// `hf://` / `ollama://` through the shared runner (no history entry).
    fn download_model(
        &self,
        events: Option<&mpsc::Sender<DownloadEvent>>,
    ) -> Result<DownloadResult, KgetError> {
        use crate::runner::{DownloadJob, Outcome, Reporter, RunContext, run_job};

        let mut job = DownloadJob::new(self.url.clone());
        job.output = self.output.clone();
        job.headers = self.headers.clone();
        job.sha256 = self.checksums.sha256.clone();
        job.retries = self.retry.max_attempts.max(1);
        job.include = self.include.clone();
        job.exclude = self.exclude.clone();
        job.revision = self.revision.clone();
        job.hf_cache = self.hf_cache;
        job.parallel_files = self.parallel_files;

        let mut config = Config {
            proxy: self.make_proxy(),
            ..Default::default()
        };
        config.optimization.speed_limit = self.speed_limit;
        config.optimization.max_connections = self.connections;
        let mut ctx = RunContext::new(config);
        ctx.record_history = false;

        let reporter = match events {
            Some(tx) => {
                let tx = std::sync::Mutex::new(tx.clone());
                Reporter::events(Arc::new(move |ev: &crate::events::JsonEvent| {
                    let msg = match ev {
                        crate::events::JsonEvent::Progress {
                            percent,
                            speed_bps,
                            eta_secs,
                            ..
                        } => DownloadEvent::Progress {
                            percent: percent.unwrap_or(0.0),
                            speed_bps: *speed_bps,
                            eta_secs: *eta_secs,
                        },
                        crate::events::JsonEvent::Status { message } => {
                            DownloadEvent::Status(message.clone())
                        }
                        _ => return,
                    };
                    let _ = crate::guard::lock(&tx).send(msg);
                }))
            }
            None if self.quiet => Reporter::quiet(),
            None => Reporter::terminal(),
        };

        let start = Instant::now();
        let outcome = run_job(&job, &ctx, &reporter).map_err(KgetError::from)?;
        let duration = start.elapsed();
        let path = match outcome {
            Outcome::Saved(Some(p)) => p,
            _ => std::path::PathBuf::new(),
        };
        let bytes_downloaded = if path.is_dir() {
            crate::fetch::dir_size(&path)
        } else {
            std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
        };
        Ok(DownloadResult {
            path: path.to_string_lossy().into_owned(),
            bytes_downloaded,
            avg_speed_bps: avg_speed(bytes_downloaded, duration),
            duration,
            connections_used: self.connections,
            checksums: ComputedChecksums::default(),
        })
    }

    /// Fetch the sidecar file and, if a matching hash is found, update
    /// `self.checksums` so the post-download verification uses it.
    fn apply_sidecar(&mut self, sidecar_url: &str) -> Result<(), KgetError> {
        let client = self.make_blocking_client()?;
        let resp = client.get(sidecar_url).send().map_err(KgetError::from)?;
        if !resp.status().is_success() {
            return Err(KgetError::SidecarError(format!(
                "HTTP {} fetching {sidecar_url}",
                resp.status()
            )));
        }
        let text = resp.text().map_err(KgetError::from)?;

        let filename = utils::get_filename_from_url_or_default(&self.url, "file");
        match parse_sidecar(&text, &filename) {
            Some((ChecksumAlgorithm::Sha256, h)) => self.checksums.sha256 = Some(h),
            Some((ChecksumAlgorithm::Sha512, h)) => self.checksums.sha512 = Some(h),
            Some((ChecksumAlgorithm::Sha1, h)) => self.checksums.sha1 = Some(h),
            Some((ChecksumAlgorithm::Md5, h)) => self.checksums.md5 = Some(h),
            Some((ChecksumAlgorithm::Blake3, h)) => self.checksums.blake3 = Some(h),
            None => {
                return Err(KgetError::SidecarError(format!(
                    "No entry for '{}' found in sidecar file",
                    filename
                )));
            }
        }
        Ok(())
    }

    /// Run the download, retrying on transient failures per `self.retry`.
    ///
    /// Later attempts continue the `.part` file written by earlier ones.
    /// Returns the final path of the saved file.
    fn run_with_retry(
        &self,
        output_path: &str,
        proxy: ProxyConfig,
        optimizer: Optimizer,
        events: Option<&mpsc::Sender<DownloadEvent>>,
    ) -> Result<String, KgetError> {
        let mut attempt = 0u32;
        loop {
            let result = self.run_once(
                output_path,
                proxy.clone(),
                optimizer.clone(),
                attempt,
                events,
            );
            match result {
                Ok(path) => return Ok(path),
                Err(e) => {
                    attempt += 1;
                    if attempt >= self.retry.max_attempts.max(1)
                        || !e.is_retryable(&self.retry.retry_on_status)
                    {
                        return Err(e);
                    }
                    let delay = self.retry.backoff.delay(attempt - 1);
                    if let Some(tx) = events {
                        let _ = tx.send(DownloadEvent::Status(format!(
                            "Attempt {}/{} failed: {e}. Retrying in {:?}…",
                            attempt, self.retry.max_attempts, delay
                        )));
                    }
                    if !self.quiet {
                        eprintln!(
                            "Attempt {}/{} failed: {e}. Retrying in {:?}…",
                            attempt, self.retry.max_attempts, delay
                        );
                    }
                    thread::sleep(delay);
                }
            }
        }
    }

    /// One attempt at the underlying download; forwards progress to `events`.
    fn run_once(
        &self,
        output_path: &str,
        proxy: ProxyConfig,
        optimizer: Optimizer,
        attempt: u32,
        events: Option<&mpsc::Sender<DownloadEvent>>,
    ) -> Result<String, KgetError> {
        // Range request: bypass the normal downloaders, use reqwest directly.
        if let Some((range_start, range_end)) = self.range {
            self.download_range(output_path, range_start, range_end)?;
            return Ok(output_path.to_string());
        }

        let progress_tx = events.cloned();
        let meter = Arc::new(std::sync::Mutex::new(crate::events::SpeedMeter::new()));
        let on_bytes = move |bytes: u64, total: Option<u64>| {
            if let Some(tx) = &progress_tx
                && let crate::events::JsonEvent::Progress {
                    percent,
                    speed_bps,
                    eta_secs,
                    ..
                } = crate::guard::lock(&meter).progress(bytes, total)
            {
                let _ = tx.send(DownloadEvent::Progress {
                    percent: percent.unwrap_or(0.0),
                    speed_bps,
                    eta_secs,
                });
            }
        };

        if self.connections > 1 {
            let mut dl = AdvancedDownloader::new(
                self.url.clone(),
                output_path.to_string(),
                self.quiet,
                proxy,
                optimizer,
            )
            .map_err(KgetError::from)?;
            dl.set_extra_headers(self.headers.clone());
            dl.set_resume_policy(ResumePolicy::AlwaysResume);
            if let Some(h) = &self.checksums.sha256 {
                dl.set_expected_sha256(h.clone());
            }
            if events.is_some() {
                dl.set_byte_progress_callback(on_bytes);
            }
            if let Some(tx) = events.cloned() {
                dl.set_status_callback(move |msg| {
                    let _ = tx.send(DownloadEvent::Status(msg));
                });
            }
            dl.download().map_err(KgetError::from)?;
            Ok(output_path.to_string())
        } else {
            let options = DownloadOptions {
                quiet_mode: self.quiet,
                output_path: Some(output_path.to_string()),
                expected_sha256: self.checksums.sha256.clone(),
                extra_headers: self.headers.clone(),
                resume: attempt > 0,
                progress_callback: events
                    .map(|_| Arc::new(on_bytes) as crate::ByteProgressCallback),
                ..Default::default()
            };
            let status_tx = events.cloned();
            let status_cb = move |msg: String| {
                if let Some(tx) = &status_tx
                    && !msg.starts_with("PROGRESS:")
                {
                    let _ = tx.send(DownloadEvent::Status(msg));
                }
            };
            let path = http_download(&self.url, proxy, optimizer, options, Some(&status_cb))
                .map_err(KgetError::from)?;
            Ok(path.to_string_lossy().into_owned())
        }
    }

    /// Raw range download via reqwest, written to `output_path`.
    fn download_range(&self, output_path: &str, start: u64, end: u64) -> Result<(), KgetError> {
        let client = self.make_blocking_client()?;
        let mut req = client
            .get(&self.url)
            .header("Range", format!("bytes={start}-{end}"));
        req = crate::http::apply_headers(req, &self.headers);

        let resp = req.send()?;
        if !resp.status().is_success() {
            return Err(KgetError::from_status(resp.status().as_u16(), &self.url));
        }
        let bytes = resp.bytes()?;

        // Create parent directory if needed
        if let Some(parent) = Path::new(output_path)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(output_path, &bytes)?;
        Ok(())
    }

    /// Compute all expected checksums and verify them.
    /// Returns `ComputedChecksums` populated with any digest that was requested.
    fn verify_and_collect(&self, path: &Path) -> Result<ComputedChecksums, KgetError> {
        if !self.checksums.any_set() {
            return Ok(ComputedChecksums::default());
        }

        let mut computed = ComputedChecksums::default();

        macro_rules! check {
            ($field:ident, $algo:expr) => {
                if let Some(expected) = &self.checksums.$field {
                    let got = compute_checksum(path, &$algo)?;
                    if got != *expected {
                        return Err(KgetError::ChecksumMismatch {
                            algorithm: $algo.name().to_string(),
                            expected: expected.clone(),
                            got,
                        });
                    }
                    computed.$field = Some(got);
                }
            };
        }

        check!(sha256, ChecksumAlgorithm::Sha256);
        check!(sha512, ChecksumAlgorithm::Sha512);
        check!(sha1, ChecksumAlgorithm::Sha1);
        check!(md5, ChecksumAlgorithm::Md5);
        check!(blake3, ChecksumAlgorithm::Blake3);

        Ok(computed)
    }

    fn resolve_output(&self) -> String {
        utils::resolve_output_path(self.output.clone(), &self.url, "download")
    }

    fn make_proxy(&self) -> ProxyConfig {
        match &self.proxy_url {
            None => ProxyConfig::default(),
            Some(url) => {
                let proxy_type = ProxyType::from_url(url);
                ProxyConfig {
                    enabled: true,
                    url: Some(url.clone()),
                    username: self.proxy_user.clone(),
                    password: self.proxy_pass.clone(),
                    proxy_type,
                }
            }
        }
    }

    fn make_optimizer(&self) -> Optimizer {
        let mut cfg = Config::default().optimization;
        cfg.speed_limit = self.speed_limit;
        cfg.max_connections = self.connections;
        Optimizer::from_config(cfg)
    }

    fn make_blocking_client(&self) -> Result<reqwest::blocking::Client, KgetError> {
        crate::http::build_client(&self.make_proxy(), &ClientOptions::default())
    }
}

// ════════════════════════════════════════════════════════════════════════════
// BatchBuilder
// ════════════════════════════════════════════════════════════════════════════

/// Result of one URL in a batch download.
pub struct BatchResult {
    /// The URL that was attempted.
    pub url: String,
    /// Success or failure for that URL.
    pub result: Result<DownloadResult, KgetError>,
}

/// Builder for downloading multiple URLs concurrently.
///
/// Create one via [`crate::batch()`].
pub struct BatchBuilder {
    urls: Vec<String>,
    concurrency: usize,
    output_dir: String,
    speed_limit: Option<u64>,
    proxy_url: Option<String>,
    proxy_user: Option<String>,
    proxy_pass: Option<String>,
    headers: Vec<(String, String)>,
    retry: RetryConfig,
    quiet: bool,
}

impl BatchBuilder {
    /// Create a batch builder from any iterator of URL strings.
    pub fn new(urls: impl IntoIterator<Item = impl Into<String>>) -> Self {
        BatchBuilder {
            urls: urls.into_iter().map(Into::into).collect(),
            concurrency: 4,
            output_dir: ".".to_string(),
            speed_limit: None,
            proxy_url: None,
            proxy_user: None,
            proxy_pass: None,
            headers: Vec::new(),
            retry: RetryConfig::default(),
            quiet: false,
        }
    }

    /// Maximum number of simultaneous downloads.  Default: 4.
    pub fn concurrency(mut self, n: usize) -> Self {
        self.concurrency = n.max(1);
        self
    }

    /// Directory where all files are saved.  Default: current directory.
    pub fn output_dir(mut self, dir: impl Into<String>) -> Self {
        self.output_dir = dir.into();
        self
    }

    /// Speed limit applied to each individual download (bytes/s).
    pub fn speed_limit(mut self, bps: u64) -> Self {
        self.speed_limit = Some(bps);
        self
    }

    /// HTTP proxy URL shared by all downloads.
    pub fn proxy(mut self, url: impl Into<String>) -> Self {
        self.proxy_url = Some(url.into());
        self
    }

    /// Proxy credentials.
    pub fn proxy_auth(mut self, user: impl Into<String>, pass: impl Into<String>) -> Self {
        self.proxy_user = Some(user.into());
        self.proxy_pass = Some(pass.into());
        self
    }

    /// Add a custom HTTP header sent with every download.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Retry policy for each individual download.
    pub fn retry(mut self, config: RetryConfig) -> Self {
        self.retry = config;
        self
    }

    /// Suppress per-download progress output.
    pub fn quiet(mut self, q: bool) -> Self {
        self.quiet = q;
        self
    }

    /// Run all downloads with bounded concurrency and return one result per URL.
    ///
    /// Uses a Rayon thread pool sized to `concurrency`.
    pub fn download_all(self) -> Vec<BatchResult> {
        use rayon::prelude::*;

        let pool = match rayon::ThreadPoolBuilder::new()
            .num_threads(self.concurrency)
            .build()
        {
            Ok(pool) => pool,
            // No threads available: every URL fails instead of panicking.
            Err(e) => {
                return self
                    .urls
                    .iter()
                    .map(|url| BatchResult {
                        url: url.clone(),
                        result: Err(KgetError::Other(format!(
                            "Cannot start the download threads: {e}"
                        ))),
                    })
                    .collect();
            }
        };

        let output_dir = Arc::new(self.output_dir.clone());
        let proxy_url = Arc::new(self.proxy_url.clone());
        let proxy_user = Arc::new(self.proxy_user.clone());
        let proxy_pass = Arc::new(self.proxy_pass.clone());
        let headers = Arc::new(self.headers.clone());
        let retry = Arc::new(self.retry.clone());
        let speed_limit = self.speed_limit;
        let quiet = self.quiet;

        let output_paths = reserve_batch_paths(&self.urls, &output_dir);

        pool.install(|| {
            self.urls
                .par_iter()
                .zip(output_paths.par_iter())
                .map(|(url, output_path)| {
                    let output_path = output_path.to_string_lossy().into_owned();
                    let mut b = DownloadBuilder::new(url)
                        .output(&output_path)
                        .quiet(quiet)
                        .retry((*retry).clone());

                    if let Some(lim) = speed_limit {
                        b = b.speed_limit(lim);
                    }
                    if let Some(pu) = proxy_url.as_ref() {
                        b = b.proxy(pu.clone());
                        if let (Some(user), Some(pass)) = (proxy_user.as_ref(), proxy_pass.as_ref())
                        {
                            b = b.proxy_auth(user.clone(), pass.clone());
                        }
                    }
                    for (k, v) in headers.iter() {
                        b = b.header(k.clone(), v.clone());
                    }

                    BatchResult {
                        url: url.clone(),
                        result: b.download(),
                    }
                })
                .collect()
        })
    }

    /// Async version of [`download_all`](Self::download_all).
    ///
    /// Requires the `async` feature.  Uses `tokio::sync::Semaphore` for
    /// concurrency control.
    #[cfg(feature = "async")]
    pub async fn download_all_async(self) -> Vec<BatchResult> {
        use std::sync::Arc as StdArc;
        use tokio::sync::Semaphore;
        use tokio::task::spawn_blocking;

        let semaphore = StdArc::new(Semaphore::new(self.concurrency));
        let mut join_handles = Vec::new();
        let output_paths = reserve_batch_paths(&self.urls, &self.output_dir);

        for (url, output_path) in self.urls.iter().cloned().zip(output_paths) {
            let sem = semaphore.clone();
            let od = self.output_dir.clone();
            let pu = self.proxy_url.clone();
            let puser = self.proxy_user.clone();
            let ppass = self.proxy_pass.clone();
            let hdrs = self.headers.clone();
            let retry = self.retry.clone();
            let sl = self.speed_limit;
            let quiet = self.quiet;

            // `acquire_owned` only fails once the semaphore is closed, and
            // this one never is.
            let permit = sem.acquire_owned().await.ok();
            let _ = od;
            let url_for_panic = url.clone();
            let h = spawn_blocking(move || {
                let _permit = permit;
                let output_path = output_path.to_string_lossy().into_owned();

                let mut b = DownloadBuilder::new(&url)
                    .output(&output_path)
                    .quiet(quiet)
                    .retry(retry);
                if let Some(lim) = sl {
                    b = b.speed_limit(lim);
                }
                if let Some(ref p) = pu {
                    b = b.proxy(p.clone());
                }
                if let (Some(u), Some(p)) = (puser, ppass) {
                    b = b.proxy_auth(u, p);
                }
                for (k, v) in hdrs {
                    b = b.header(k, v);
                }

                BatchResult {
                    url,
                    result: b.download(),
                }
            });
            join_handles.push((url_for_panic, h));
        }

        let mut results = Vec::new();
        for (url, h) in join_handles {
            match h.await {
                Ok(r) => results.push(r),
                // Keep results aligned with the input URLs.
                Err(e) => results.push(BatchResult {
                    url,
                    result: Err(KgetError::Other(format!("download task panicked: {e}"))),
                }),
            }
        }
        results
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Free-function helpers
// ════════════════════════════════════════════════════════════════════════════

/// Average speed in bytes/second, using fractional seconds.
fn avg_speed(bytes: u64, duration: Duration) -> u64 {
    let secs = duration.as_secs_f64().max(0.001);
    (bytes as f64 / secs) as u64
}

/// One unique destination per URL inside `dir` (`file (1).zip` on collisions).
fn reserve_batch_paths(urls: &[String], dir: &str) -> Vec<PathBuf> {
    let dir = PathBuf::from(utils::expand_tilde(dir));
    let mut taken = HashSet::new();
    urls.iter()
        .map(|url| {
            let name = utils::get_filename_from_url_or_default(url, "download");
            utils::unique_path(&dir, &name, &mut taken)
        })
        .collect()
}
