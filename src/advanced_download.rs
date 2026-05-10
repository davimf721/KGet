//! Advanced parallel download functionality with resume support.
//!
//! The [`AdvancedDownloader`] provides high-performance downloads using:
//! - **Parallel connections**: Split files into chunks downloaded simultaneously
//! - **Resume support**: Continue interrupted downloads from where they left off
//! - **Progress callbacks**: Real-time progress and status updates
//! - **Cancellation**: Graceful download cancellation via atomic tokens
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
//! );
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
//! downloader.download().unwrap();
//! ```
//!
//! # Parallel Downloads
//!
//! The downloader automatically determines the optimal number of connections
//! based on the [`Optimizer`] configuration. For large files,
//! this can provide significant speed improvements.

use crate::config::ProxyConfig;
use crate::optimization::Optimizer;
use hex;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use reqwest::blocking::Client;
use sha2::{Digest, Sha256};
use std::error::Error;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(target_family = "unix")]
use std::os::unix::fs::FileExt;
#[cfg(target_family = "windows")]
use std::os::windows::fs::FileExt;

/// Minimum chunk size for parallel downloads (4 MB)
const MIN_CHUNK_SIZE: u64 = 4 * 1024 * 1024;
/// Maximum retry attempts per chunk
const MAX_RETRIES: usize = 3;

/// High-performance downloader with parallel connections and resume support.
///
/// `AdvancedDownloader` is the recommended way to download large files. It provides:
///
/// - **Parallel chunk downloads**: Splits files into segments downloaded simultaneously
/// - **Automatic resume**: Detects existing partial files and resumes from last position
/// - **Server compatibility**: Falls back to single-stream if server doesn't support ranges
/// - **ISO optimization**: Disables compression for binary files to prevent corruption
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
/// );
///
/// downloader.download().expect("Download failed");
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
/// );
///
/// dl.set_progress_callback(|p| {
///     // p is 0.0 to 1.0
///     update_ui_progress(p);
/// });
///
/// dl.set_status_callback(|msg| println!("{}", msg));
///
/// dl.download().unwrap();
///
/// fn update_ui_progress(p: f32) {
///     // Update your UI here
/// }
/// ```
pub struct AdvancedDownloader {
    client: Client,
    url: String,
    output_path: String,
    quiet_mode: bool,
    #[allow(dead_code)]
    proxy: ProxyConfig,
    optimizer: Optimizer,
    progress_callback: Option<Arc<dyn Fn(f32) + Send + Sync>>,
    status_callback: Option<Arc<dyn Fn(String) + Send + Sync>>,
    cancel_token: Arc<AtomicBool>,
    expected_sha256: Option<String>,
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
    /// );
    /// ```
    pub fn new(
        url: String,
        output_path: String,
        quiet_mode: bool,
        proxy_config: ProxyConfig,
        optimizer: Optimizer,
    ) -> Self {
        let _is_iso = url.to_lowercase().ends_with(".iso");

        let mut client_builder = Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .connect_timeout(std::time::Duration::from_secs(20))
            .user_agent("KGet/1.0")
            .no_gzip()
            .no_deflate();

        if proxy_config.enabled {
            if let Some(proxy_url) = &proxy_config.url {
                let proxy = match proxy_config.proxy_type {
                    crate::config::ProxyType::Http => reqwest::Proxy::http(proxy_url),
                    crate::config::ProxyType::Https => reqwest::Proxy::https(proxy_url),
                    crate::config::ProxyType::Socks5 => reqwest::Proxy::all(proxy_url),
                };

                if let Ok(mut proxy) = proxy {
                    if let (Some(username), Some(password)) =
                        (&proxy_config.username, &proxy_config.password)
                    {
                        proxy = proxy.basic_auth(username, password);
                    }
                    client_builder = client_builder.proxy(proxy);
                }
            }
        }

        let client = client_builder
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            url,
            output_path,
            quiet_mode,
            proxy: proxy_config,
            optimizer,
            progress_callback: None,
            status_callback: None,
            cancel_token: Arc::new(AtomicBool::new(false)),
            expected_sha256: None,
        }
    }

    /// Set a custom cancellation token for graceful download interruption.
    ///
    /// When the token is set to `true`, the download will stop at the next
    /// checkpoint and return an error.
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
    /// );
    /// dl.set_cancel_token(cancel.clone());
    ///
    /// // In another thread:
    /// // cancel.store(true, std::sync::atomic::Ordering::Relaxed);
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
    /// The callback receives a value from 0.0 (start) to 1.0 (complete).
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use kget::{AdvancedDownloader, ProxyConfig, Optimizer};
    /// # let mut dl = AdvancedDownloader::new("".to_string(), "".to_string(), false, ProxyConfig::default(), Optimizer::new());
    /// dl.set_progress_callback(|progress| {
    ///     println!("Downloaded: {:.1}%", progress * 100.0);
    /// });
    /// ```
    pub fn set_progress_callback(&mut self, callback: impl Fn(f32) + Send + Sync + 'static) {
        self.progress_callback = Some(Arc::new(callback));
    }

    /// Set a callback for status messages.
    ///
    /// Receives human-readable status updates during the download.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use kget::{AdvancedDownloader, ProxyConfig, Optimizer};
    /// # let mut dl = AdvancedDownloader::new("".to_string(), "".to_string(), false, ProxyConfig::default(), Optimizer::new());
    /// dl.set_status_callback(|msg| {
    ///     println!("Download status: {}", msg);
    /// });
    /// ```
    pub fn set_status_callback(&mut self, callback: impl Fn(String) + Send + Sync + 'static) {
        self.status_callback = Some(Arc::new(callback));
    }

    /// Set an expected SHA-256 hash for automatic verification after download.
    pub fn set_expected_sha256(&mut self, expected_sha256: impl Into<String>) {
        self.expected_sha256 = Some(expected_sha256.into());
    }

    fn send_status(&self, msg: &str) {
        if let Some(cb) = &self.status_callback {
            cb(msg.to_string());
        }
        if !self.quiet_mode {
            println!("{}", msg);
        }
    }

    /// Start the download.
    ///
    /// This method:
    /// 1. Checks for existing partial file (resume support)
    /// 2. Queries server for file size and range support
    /// 3. Downloads using parallel connections if supported
    /// 4. Falls back to single-stream if server doesn't support ranges
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on successful download, or an error if the download fails.
    ///
    /// # Errors
    ///
    /// - Network connection failures
    /// - Existing file larger than remote (corrupted state)
    /// - Cancellation via cancel token
    /// - Disk I/O errors
    pub fn download(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let is_iso = self.url.to_lowercase().ends_with(".iso");
        if !self.quiet_mode {
            println!("Starting advanced download for: {}", self.url);
            if is_iso {
                println!(
                    "Warning: ISO mode active. Disabling optimizations that could corrupt binary data."
                );
            }
        }

        // Verify if the output path is valid
        let existing_size = if Path::new(&self.output_path).exists() {
            let size = std::fs::metadata(&self.output_path)?.len();
            if !self.quiet_mode {
                println!("Existing file found with size: {} bytes", size);
            }
            Some(size)
        } else {
            if !self.quiet_mode {
                println!("Output file does not exist, starting fresh download");
            }
            None
        };

        // Get the total file size and range support
        if !self.quiet_mode {
            println!("Querying server for file size and range support...");
        }
        let (total_size, supports_range) = self.get_file_size_and_range()?;
        if !self.quiet_mode {
            println!("Total file size: {} bytes", total_size);
            println!("Server supports range requests: {}", supports_range);
        }

        if let Some(size) = existing_size {
            if size > total_size {
                return Err("Existing file is larger than remote; aborting".into());
            }
            if !self.quiet_mode {
                println!("Resuming download from byte: {}", size);
            }
        }

        // Create a progress bar if not quiet or if we have a callback
        let progress = if !self.quiet_mode || self.progress_callback.is_some() {
            let bar = ProgressBar::new(total_size);
            if let Some(size) = existing_size {
                bar.set_position(size);
            }
            if self.quiet_mode {
                bar.set_draw_target(indicatif::ProgressDrawTarget::hidden());
            } else {
                bar.set_style(ProgressStyle::with_template(
                    "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})"
                ).unwrap().progress_chars("#>-"));
            }
            Some(Arc::new(Mutex::new(bar)))
        } else {
            None
        };

        // Create or open the output file and preallocate
        if !self.quiet_mode {
            println!("Preparing output file: {}", self.output_path);
        }
        let file = if existing_size.is_some() {
            File::options()
                .read(true)
                .write(true)
                .open(&self.output_path)?
        } else {
            File::create(&self.output_path)?
        };
        file.set_len(total_size)?;
        if !self.quiet_mode {
            println!("File preallocated to {} bytes", total_size);
        }

        // If range not supported, do a single download
        if !supports_range {
            if !self.quiet_mode {
                println!("Range requests not supported, falling back to single-threaded download");
            }
            self.download_whole(&file, existing_size.unwrap_or(0), progress.clone())?;
            if let Some(ref bar) = progress {
                bar.lock()
                    .expect("Progress bar mutex was poisoned")
                    .finish_with_message("Download completed");
            }
            if !self.quiet_mode {
                println!("Single-threaded download completed");
            }
            return Ok(());
        }

        // Calculate chunks for parallel download
        if !self.quiet_mode {
            println!("Calculating download chunks...");
        }
        let chunks = self.calculate_chunks(total_size, existing_size)?;
        if !self.quiet_mode {
            println!("Download will be split into {} chunks", chunks.len());
        }

        // Download parallel chunks
        if !self.quiet_mode {
            println!("Starting parallel chunk downloads...");
        }
        self.download_chunks_parallel(
            chunks,
            &file,
            progress.clone(),
            total_size,
            existing_size.unwrap_or(0),
        )?;

        if let Some(ref bar) = progress {
            bar.lock()
                .expect("Progress bar mutex was poisoned")
                .finish_with_message("Download completed");
        }

        // Verify download integrity
        if !self.quiet_mode || self.status_callback.is_some() {
            if is_iso || self.expected_sha256.is_some() {
                let should_verify = if self.status_callback.is_some() {
                    true
                } else if self.expected_sha256.is_some() {
                    true
                } else {
                    println!(
                        "\nThis is an ISO file. Would you like to verify its integrity? (y/N)"
                    );
                    let mut input = String::new();
                    std::io::stdin().read_line(&mut input).is_ok()
                        && input.trim().to_lowercase() == "y"
                };

                if should_verify {
                    self.verify_integrity(total_size)?;
                }
            } else {
                let metadata = std::fs::metadata(&self.output_path)?;
                if metadata.len() != total_size {
                    return Err(format!(
                        "File size mismatch: expected {} bytes, got {} bytes",
                        total_size,
                        metadata.len()
                    )
                    .into());
                }
            }
            self.send_status("Advanced download completed successfully!");
        }

        Ok(())
    }

    fn get_file_size_and_range(&self) -> Result<(u64, bool), Box<dyn Error + Send + Sync>> {
        let head_response = self.client.head(&self.url).send();
        let Ok(response) = head_response else {
            return self.get_file_size_with_range_probe();
        };

        if !response.status().is_success() {
            return self.get_file_size_with_range_probe();
        }

        let content_length = response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());

        let accepts_range = response
            .headers()
            .get(reqwest::header::ACCEPT_RANGES)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.eq_ignore_ascii_case("bytes"))
            .unwrap_or(false);

        if let Some(content_length) = content_length {
            Ok((content_length, accepts_range))
        } else {
            self.get_file_size_with_range_probe()
        }
    }

    fn get_file_size_with_range_probe(&self) -> Result<(u64, bool), Box<dyn Error + Send + Sync>> {
        let response = self
            .client
            .get(&self.url)
            .header(reqwest::header::RANGE, "bytes=0-0")
            .send()?;

        if response.status() == reqwest::StatusCode::PARTIAL_CONTENT {
            if let Some(total) = response
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(parse_content_range_total)
            {
                return Ok((total, true));
            }
        }

        if response.status().is_success() {
            if let Some(total) = response
                .headers()
                .get(reqwest::header::CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
            {
                return Ok((total, false));
            }
        }

        Err("Could not determine file size".into())
    }

    fn calculate_chunks(
        &self,
        total_size: u64,
        existing_size: Option<u64>,
    ) -> Result<Vec<(u64, u64)>, Box<dyn Error + Send + Sync>> {
        let mut chunks = Vec::new();
        let start_from = existing_size.unwrap_or(0);

        let configured_parallelism = self.optimizer.max_connections() as u64;
        let runtime_parallelism = rayon::current_num_threads() as u64;
        let parallelism = configured_parallelism.min(runtime_parallelism).max(1);
        let target_chunks = parallelism.saturating_mul(2).max(2); // Keep workers fed without overwhelming servers.
        let chunk_size = ((total_size / target_chunks).max(MIN_CHUNK_SIZE)).min(64 * 1024 * 1024);

        let mut start = start_from;
        while start < total_size {
            let end = (start + chunk_size).min(total_size);
            chunks.push((start, end));
            start = end;
        }

        Ok(chunks)
    }

    fn download_whole(
        &self,
        file: &File,
        offset: u64,
        progress: Option<Arc<Mutex<ProgressBar>>>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let response = self.client.get(&self.url).send()?;
        if offset > 0 {
            // Resume not possible without range; warn
            return Err("Server does not support range; cannot resume partial file".into());
        }

        let mut reader = BufReader::new(response);
        let mut f = file.try_clone()?;
        f.seek(SeekFrom::Start(0))?;

        struct ProgressWriter<'a, W> {
            inner: W,
            progress: Option<Arc<Mutex<ProgressBar>>>,
            callback: Option<&'a Arc<dyn Fn(f32) + Send + Sync>>,
        }

        impl<'a, W: Write> Write for ProgressWriter<'a, W> {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                let n = self.inner.write(buf)?;
                if let Some(ref bar) = self.progress {
                    let guard = bar.lock().expect("Progress bar mutex was poisoned");
                    guard.inc(n as u64);
                    if let Some(cb) = self.callback {
                        let pos = guard.position();
                        let len = guard.length().unwrap_or(1);
                        drop(guard);
                        (cb)(pos as f32 / len as f32);
                    }
                }
                Ok(n)
            }

            fn flush(&mut self) -> std::io::Result<()> {
                self.inner.flush()
            }
        }

        let mut writer = ProgressWriter {
            inner: f,
            progress,
            callback: self.progress_callback.as_ref(),
        };
        std::io::copy(&mut reader, &mut writer)?;

        Ok(())
    }

    fn download_chunks_parallel(
        &self,
        chunks: Vec<(u64, u64)>,
        file: &File,
        progress: Option<Arc<Mutex<ProgressBar>>>,
        total_size: u64,
        initial_downloaded: u64,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let file = Arc::new(file);
        let client = Arc::new(self.client.clone());
        let url = Arc::new(self.url.clone());
        let _optimizer = Arc::new(self.optimizer.clone());
        let progress_callback = self.progress_callback.clone();
        let cancel_token = self.cancel_token.clone();

        // Shared progress counter for pipe-friendly output
        let downloaded_bytes = Arc::new(AtomicU64::new(initial_downloaded));
        let last_print_time = Arc::new(Mutex::new(Instant::now()));
        let started_at = Arc::new(Instant::now());
        let speed_limit = self.optimizer.speed_limit;
        let quiet_mode = self.quiet_mode;

        chunks.par_iter().try_for_each(|&(start, end)| {
            // Check for cancellation before starting chunk
            if cancel_token.load(Ordering::Relaxed) {
                return Err("Download cancelled".into());
            }

            let range = format!("bytes={}-{}", start, end - 1);
            let range_header = reqwest::header::HeaderValue::from_str(&range)
                .map_err(|e| format!("Invalid range header {}: {}", range, e))?;

            for retry in 0..=MAX_RETRIES {
                // Check for cancellation on each retry
                if cancel_token.load(Ordering::Relaxed) {
                    return Err("Download cancelled".into());
                }

                let request = client.get(url.as_str());
                let request = request.header(reqwest::header::RANGE, range_header.clone());

                match request.send() {
                    Ok(mut response) => {
                        let status = response.status();
                        if status == reqwest::StatusCode::PARTIAL_CONTENT {
                            // Use FileExt to write at specific offset without seeking shared cursor
                            // This prevents race conditions when multiple threads write to the same file

                            let mut current_pos = start;
                            let mut buffer = [0u8; 16384];

                            while current_pos < end {
                                // Check for cancellation periodically during download
                                if cancel_token.load(Ordering::Relaxed) {
                                    return Err("Download cancelled".into());
                                }

                                let limit = (end - current_pos).min(buffer.len() as u64);
                                let n = response.read(&mut buffer[..limit as usize])?;
                                if n == 0 {
                                    break;
                                }

                                #[cfg(target_family = "unix")]
                                file.write_at(&buffer[..n], current_pos)?;

                                #[cfg(target_family = "windows")]
                                file.seek_write(&buffer[..n], current_pos)?;

                                current_pos += n as u64;

                                // Update shared progress counter
                                let new_downloaded = downloaded_bytes
                                    .fetch_add(n as u64, Ordering::Relaxed)
                                    + n as u64;

                                // Print progress periodically (every 200ms) for pipe-friendly output
                                {
                                    let mut last_time = last_print_time.lock().expect("Timer mutex was poisoned");
                                    if !quiet_mode && last_time.elapsed() >= Duration::from_millis(200) {
                                let percent = (new_downloaded as f64 / total_size.max(1) as f64
                                            * 100.0)
                                            .min(100.0);
                                        // PROGRESS: format that Swift can parse
                                        println!(
                                            "PROGRESS: {:.1}% ({}/{})",
                                            percent, new_downloaded, total_size
                                        );
                                        *last_time = Instant::now();
                                    }
                                }

                                throttle_parallel_download(
                                    new_downloaded.saturating_sub(initial_downloaded),
                                    *started_at,
                                    speed_limit,
                                );

                                if let Some(ref bar) = progress {
                                    let guard = bar.lock().expect("Progress bar mutex was poisoned");
                                    guard.inc(n as u64);
                                    if let Some(ref cb) = progress_callback {
                                        let pos = guard.position();
                                        let len = guard.length().unwrap_or(1);
                                        drop(guard);
                                        (cb)(pos as f32 / len as f32);
                                    }
                                }
                            }

                            return Ok::<(), Box<dyn Error + Send + Sync>>(());
                        } else if status == reqwest::StatusCode::OK {
                            return Err(format!(
                                "Server ignored range request for chunk {}-{}; refusing to write mismatched data",
                                start, end
                            )
                            .into());
                        } else if status.as_u16() == 416 {
                            if retry == MAX_RETRIES {
                                return Err(format!(
                                    "Failed to download chunk {}-{}: HTTP {}",
                                    start, end, status
                                )
                                .into());
                            }
                            std::thread::sleep(Duration::from_millis(250 * (retry as u64 + 1)));
                        }
                    }
                    Err(e) => {
                        if retry == MAX_RETRIES {
                            return Err(format!(
                                "Failed to download chunk {}-{}: {}",
                                start, end, e
                            )
                            .into());
                        }
                        std::thread::sleep(Duration::from_millis(250 * (retry as u64 + 1)));
                    }
                }
            }
            Err(format!("Failed to download chunk {}-{} after retries", start, end).into())
        })?;

        Ok(())
    }

    fn verify_integrity(&self, expected_size: u64) -> Result<(), Box<dyn Error + Send + Sync>> {
        let metadata = std::fs::metadata(&self.output_path)?;
        let actual_size = metadata.len();

        if actual_size != expected_size {
            return Err(format!(
                "File size mismatch: expected {} bytes, got {} bytes",
                expected_size, actual_size
            )
            .into());
        }

        self.send_status(&format!("File size verified: {} bytes", actual_size));

        // Calculate SHA256 hash for corruption check
        self.send_status("Calculating SHA256 hash...");

        let mut file = File::open(&self.output_path)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0; 8192];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
        let hash = hasher.finalize();
        let hash_hex = hex::encode(hash);

        self.send_status(&format!("SHA256 hash: {}", hash_hex));
        if let Some(expected_sha256) = &self.expected_sha256 {
            let expected_sha256 = expected_sha256.trim().to_ascii_lowercase();
            if hash_hex != expected_sha256 {
                return Err(format!(
                    "SHA256 mismatch: expected {}, got {}",
                    expected_sha256, hash_hex
                )
                .into());
            }
            self.send_status("SHA256 matches expected hash.");
        }
        self.send_status("Integrity check passed - file is not corrupted");

        Ok(())
    }
}

fn parse_content_range_total(value: &str) -> Option<u64> {
    let (_, total) = value.rsplit_once('/')?;
    if total == "*" {
        return None;
    }
    total.parse::<u64>().ok()
}

fn throttle_parallel_download(downloaded_this_run: u64, started_at: Instant, speed_limit: Option<u64>) {
    let Some(limit) = speed_limit else { return };
    if limit == 0 {
        return;
    }

    let expected_elapsed = Duration::from_secs_f64(downloaded_this_run as f64 / limit as f64);
    let actual_elapsed = started_at.elapsed();
    if expected_elapsed > actual_elapsed {
        std::thread::sleep(expected_elapsed - actual_elapsed);
    }
}
