//! Simple HTTP/HTTPS download functionality.
//!
//! This module provides basic download capabilities with automatic retry,
//! progress tracking, and ISO integrity verification.
//!
//! Data is written to `<file>.part` and renamed to the final name only after
//! the whole body was received (and the hash, if any, matched). A body
//! shorter than `Content-Length` is retried with a `Range` request, and
//! `DownloadOptions::resume` continues an existing `.part` file.
//!
//! For advanced features like parallel connections and resume support,
//! see [`AdvancedDownloader`](crate::AdvancedDownloader).
//!
//! # Example
//!
//! ```rust,no_run
//! use kget::{download, DownloadOptions, ProxyConfig, Optimizer};
//!
//! let options = DownloadOptions {
//!     output_path: Some("./file.zip".to_string()),
//!     ..Default::default()
//! };
//!
//! download(
//!     "https://example.com/file.zip",
//!     ProxyConfig::default(),
//!     Optimizer::new(),
//!     options,
//!     None,
//! ).unwrap();
//! ```

use crate::config::ProxyConfig;
use crate::error::KgetError;
use crate::http::{self, ClientOptions};
use crate::optimization::Optimizer;
use crate::progress::{Throttle, create_progress_bar};
use crate::utils::{self, print};
use crate::{DownloadOptions, OverwritePolicy};
use humansize::{DECIMAL, format_size};
use mime::Mime;
use reqwest::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use sha2::Digest;
use std::error::Error;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

const READ_BUF: usize = 256 * 1024;

/// Check if there's enough disk space for the download.
///
/// # Arguments
///
/// * `path` - Target file path
/// * `required_size` - Required space in bytes
///
/// # Errors
///
/// Returns an error if available space is less than required.
pub fn check_disk_space(
    path: &Path,
    required_size: u64,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let available_space = fs2::available_space(dir)?;

    if available_space < required_size {
        return Err(format!(
            "Insufficient disk space. Required: {}, Available: {}",
            format_size(required_size, DECIMAL),
            format_size(available_space, DECIMAL)
        )
        .into());
    }
    Ok(())
}

/// Validate that a filename is safe and valid.
///
/// # Errors
///
/// Returns an error if the filename:
/// - Is empty
/// - Contains directory separators
/// - Contains null bytes
/// - Is exactly `.` or `..`
/// - Exceeds 255 bytes
/// - Matches a Windows reserved name (CON, NUL, COM1–9, LPT1–9, etc.)
pub fn validate_filename(filename: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    if filename.is_empty() {
        return Err("Filename cannot be empty".into());
    }
    if filename == "." || filename == ".." {
        return Err("Filename cannot be '.' or '..'".into());
    }
    if filename.contains('/') || filename.contains('\\') {
        return Err("Filename cannot contain directory separators".into());
    }
    if filename.contains('\0') {
        return Err("Filename cannot contain null bytes".into());
    }
    if filename.len() > 255 {
        return Err("Filename exceeds maximum length of 255 bytes".into());
    }
    // Windows reserved device names — forbidden even on Unix to stay cross-platform safe.
    let stem = filename.split('.').next().unwrap_or(filename);
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.iter().any(|r| stem.eq_ignore_ascii_case(r)) {
        return Err(format!("'{}' is a reserved filename on Windows", stem).into());
    }
    Ok(())
}

/// Download a file from a URL with automatic retry and progress tracking.
///
/// This is the simple download function for basic use cases. For parallel
/// connections, use [`AdvancedDownloader`](crate::AdvancedDownloader).
/// See [`download_to_path`] to also get the final path of the saved file.
///
/// # Arguments
///
/// * `target` - URL to download
/// * `proxy` - Proxy configuration (use `ProxyConfig::default()` for no proxy)
/// * `optimizer` - Optimizer instance (speed limit)
/// * `options` - Download options (quiet mode, output path, resume, retries…)
/// * `status_callback` - Optional callback for status messages
///
/// # Example
///
/// ```rust,no_run
/// use kget::{download, DownloadOptions, ProxyConfig, Optimizer};
///
/// download(
///     "https://releases.ubuntu.com/22.04/ubuntu-22.04-desktop-amd64.iso",
///     ProxyConfig::default(),
///     Optimizer::new(),
///     DownloadOptions {
///         verify_iso: true, // Verify SHA256 after download
///         ..Default::default()
///     },
///     None,
/// ).unwrap();
/// ```
///
/// # Errors
///
/// Returns an error if:
/// - Network connection fails after `options.retries` attempts
/// - HTTP response indicates an error (a boxed [`KgetError::Http`] / [`KgetError::NotFound`])
/// - The body is shorter than `Content-Length` after all retries
/// - Insufficient disk space, or the file cannot be created
/// - The SHA-256 does not match
pub fn download(
    target: &str,
    proxy: ProxyConfig,
    optimizer: Optimizer,
    options: DownloadOptions,
    status_callback: Option<&(dyn Fn(String) + Send + Sync)>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    download_to_path(target, proxy, optimizer, options, status_callback).map(|_| ())
}

/// Same as [`download`], returning the absolute path of the saved file
/// (which may come from `Content-Disposition`).
pub fn download_to_path(
    target: &str,
    proxy: ProxyConfig,
    optimizer: Optimizer,
    options: DownloadOptions,
    status_callback: Option<&(dyn Fn(String) + Send + Sync)>,
) -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
    let quiet_mode = options.quiet_mode;
    let max_attempts = options.retries.max(1);
    let client = http::build_client(
        &proxy,
        &ClientOptions::with_timeout_secs(options.timeout_secs),
    )?;
    let cancelled = || {
        options
            .cancel
            .as_ref()
            .is_some_and(|c| c.load(Ordering::Relaxed))
    };

    // Destination known before the request (explicit file or URL-derived name).
    let explicit_file = options
        .output_path
        .as_deref()
        .map(utils::expand_tilde)
        .filter(|o| !is_dir_like(o));
    let pre_path = resolve_final_path(target, options.output_path.as_deref(), None)?;
    if explicit_file.is_some() && pre_path.exists() && options.overwrite == OverwritePolicy::Skip {
        print(
            &format!("File already exists, skipping: {}", pre_path.display()),
            quiet_mode,
        );
        return Ok(pre_path);
    }

    let mut downloaded: u64 = if options.resume {
        std::fs::metadata(part_path_for(&pre_path))
            .map(|m| m.len())
            .unwrap_or(0)
    } else {
        0
    };
    if downloaded > 0 {
        print(
            &format!("Resuming {} from byte {}", pre_path.display(), downloaded),
            quiet_mode,
        );
    }

    let mut final_path: Option<PathBuf> = None;
    let mut attempt = 0u32;
    let mut progress_bar: Option<indicatif::ProgressBar> = None;
    let started_at = Instant::now();
    let session_start_bytes = downloaded;
    let mut status_throttle = Throttle::new(200);
    let mut byte_throttle = Throttle::new(200);
    let mut is_iso = target.to_lowercase().ends_with(".iso");
    let mut last_reported = u64::MAX;

    loop {
        if cancelled() {
            return Err(Box::new(KgetError::Cancelled));
        }
        let mut req = http::apply_headers(client.get(target), &options.extra_headers);
        if downloaded > 0 {
            req = req.header(reqwest::header::RANGE, format!("bytes={downloaded}-"));
        }

        let response = match req.send() {
            Ok(r) => r,
            Err(e) => {
                attempt += 1;
                if attempt >= max_attempts {
                    return Err(Box::new(KgetError::Network(format!(
                        "Failed after {max_attempts} attempts: {e}"
                    ))));
                }
                let delay = http::backoff_delay(attempt);
                print(
                    &format!(
                        "Attempt {attempt} failed ({e}), retrying in {:.1}s...",
                        delay.as_secs_f64()
                    ),
                    quiet_mode,
                );
                std::thread::sleep(delay);
                continue;
            }
        };

        let status = response.status();
        let append = match status.as_u16() {
            206 if downloaded > 0 => true,
            416 if downloaded > 0 => {
                // The .part file already holds the whole resource.
                let total = response
                    .headers()
                    .get(reqwest::header::CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok())
                    .and_then(parse_content_range_total);
                if total.is_none_or(|t| t == downloaded) {
                    if final_path.is_none() {
                        final_path = Some(pre_path.clone());
                    }
                    break;
                }
                // Stale .part larger than the remote file: start over.
                downloaded = 0;
                continue;
            }
            code if http::is_transient_status(code) => {
                attempt += 1;
                if attempt >= max_attempts {
                    return Err(Box::new(KgetError::from_status(code, target)));
                }
                let delay =
                    http::retry_after(&response).unwrap_or_else(|| http::backoff_delay(attempt));
                print(
                    &format!(
                        "Server error {status} on attempt {attempt}, retrying in {:.1}s...",
                        delay.as_secs_f64()
                    ),
                    quiet_mode,
                );
                std::thread::sleep(delay);
                continue;
            }
            _ if !status.is_success() => {
                return Err(Box::new(KgetError::from_status(status.as_u16(), target)));
            }
            _ => {
                // 200 (or 2xx without range): the server sends the whole file.
                downloaded = 0;
                false
            }
        };

        // Resolve the destination once, from the first successful response.
        let dest_path = match &final_path {
            Some(p) => p.clone(),
            None => {
                print(&format!("HTTP request sent... {status}"), quiet_mode);
                let content_type = response
                    .headers()
                    .get(CONTENT_TYPE)
                    .and_then(|ct| ct.to_str().ok())
                    .and_then(|s| s.parse::<Mime>().ok());
                let server_filename = response
                    .headers()
                    .get(CONTENT_DISPOSITION)
                    .and_then(|v| v.to_str().ok())
                    .and_then(parse_content_disposition_filename);
                match response.content_length() {
                    Some(len) if !append => print(
                        &format!("Length: {} ({})", len, format_size(len, DECIMAL)),
                        quiet_mode,
                    ),
                    Some(len) => print(
                        &format!(
                            "Length: {} ({}), resuming at {}",
                            len + downloaded,
                            format_size(len + downloaded, DECIMAL),
                            downloaded
                        ),
                        quiet_mode,
                    ),
                    None => print("Length: unknown", quiet_mode),
                }
                if let Some(ref ct) = content_type {
                    print(&format!("Type: {ct}"), quiet_mode);
                    is_iso |= ct.essence_str() == "application/x-iso9660-image"
                        || ct.essence_str() == "application/x-cd-image";
                }

                // When resuming, keep the name the .part file was saved under.
                let mut path = if append || explicit_file.is_some() {
                    pre_path.clone()
                } else {
                    resolve_final_path(
                        target,
                        options.output_path.as_deref(),
                        server_filename.as_deref(),
                    )?
                };
                if !append && path.exists() {
                    match options.overwrite {
                        OverwritePolicy::Skip => {
                            print(
                                &format!("File already exists, skipping: {}", path.display()),
                                quiet_mode,
                            );
                            return Ok(path);
                        }
                        OverwritePolicy::Rename => {
                            let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
                            let name = path
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_else(|| "download".into());
                            path = utils::unique_path(&dir, &name, &mut Default::default());
                        }
                        OverwritePolicy::Overwrite => {}
                    }
                }
                ensure_parent_dir(&path, quiet_mode)?;
                print(&format!("Saving to: {}", path.display()), quiet_mode);
                if let Some(len) = response.content_length() {
                    check_disk_space(&path, len)?;
                }
                final_path = Some(path.clone());
                path
            }
        };
        let part_path = part_path_for(&dest_path);
        let expected_total = response.content_length().map(|l| l + downloaded);

        let bar = progress_bar.get_or_insert_with(|| {
            create_progress_bar(
                quiet_mode,
                dest_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                expected_total,
                false,
            )
        });
        if let Some(t) = expected_total {
            bar.set_length(t);
        }
        bar.set_position(downloaded);

        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(&part_path)
            .map_err(|e| format!("Failed to create file {}: {}", part_path.display(), e))?;
        let mut dest = BufWriter::with_capacity(READ_BUF, file);
        let mut source = response;
        let mut buffer = vec![0u8; READ_BUF];

        let copy_result: Result<(), std::io::Error> = (|| {
            loop {
                if cancelled() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "cancelled",
                    ));
                }
                let n = source.read(&mut buffer)?;
                if n == 0 {
                    return Ok(());
                }
                dest.write_all(&buffer[..n])?;
                downloaded += n as u64;
                bar.set_position(downloaded);

                let pct = expected_total
                    .map(|t| downloaded as f64 / t.max(1) as f64 * 100.0)
                    .unwrap_or(0.0);
                if let (Some(cb), Some(total)) = (status_callback, expected_total)
                    && status_throttle.ready(pct)
                {
                    cb(format!("PROGRESS: {pct:.1}% ({downloaded}/{total})"));
                }
                if let Some(cb) = &options.progress_callback {
                    let fire = if expected_total.is_some() {
                        byte_throttle.ready(pct)
                    } else {
                        byte_throttle.tick()
                    };
                    if fire {
                        cb(downloaded, expected_total);
                        last_reported = downloaded;
                    }
                }

                throttle_download(
                    downloaded - session_start_bytes,
                    started_at,
                    optimizer.speed_limit,
                );
            }
        })();
        let flushed = dest.flush();

        match copy_result.and(flushed) {
            Ok(()) => {
                if let Some(total) = expected_total
                    && downloaded < total
                {
                    attempt += 1;
                    if attempt >= max_attempts {
                        return Err(format!(
                            "Incomplete download: received {downloaded} of {total} bytes; run again with -c to resume"
                        )
                        .into());
                    }
                    print(
                        &format!(
                            "Connection closed early at {downloaded}/{total} bytes, resuming..."
                        ),
                        quiet_mode,
                    );
                    std::thread::sleep(http::backoff_delay(attempt));
                    continue;
                }
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted && cancelled() => {
                return Err(Box::new(KgetError::Cancelled));
            }
            Err(e) => {
                attempt += 1;
                if attempt >= max_attempts {
                    return Err(format!(
                        "Download interrupted at {downloaded} bytes: {e}; run again with -c to resume"
                    )
                    .into());
                }
                print(
                    &format!("Read error ({e}), resuming from byte {downloaded}..."),
                    quiet_mode,
                );
                std::thread::sleep(http::backoff_delay(attempt));
            }
        }
    }

    let final_path = final_path.unwrap_or(pre_path);
    let part_path = part_path_for(&final_path);
    if let Some(bar) = &progress_bar {
        bar.finish_with_message("Download completed\n");
    }
    if let Some(cb) = &options.progress_callback
        && last_reported != downloaded
    {
        cb(downloaded, Some(downloaded));
    }
    if let Some(cb) = status_callback {
        cb(format!("PROGRESS: 100.0% ({downloaded}/{downloaded})"));
    }

    // Verify before exposing the file under its final name.
    if (options.expected_sha256.is_some() || (is_iso && options.verify_iso))
        && let Err(e) = verify_file_sha256(
            &part_path,
            options.expected_sha256.as_deref(),
            status_callback,
        )
    {
        let _ = std::fs::remove_file(&part_path);
        return Err(e);
    }

    std::fs::rename(&part_path, &final_path).map_err(|e| {
        format!(
            "Failed to move {} to {}: {}",
            part_path.display(),
            final_path.display(),
            e
        )
    })?;
    Ok(final_path)
}

/// `<file>.part`, the temporary name used while a download is in progress.
pub fn part_path_for(final_path: &Path) -> PathBuf {
    let mut name = final_path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_else(|| "download".into());
    name.push(".part");
    final_path.with_file_name(name)
}

fn is_dir_like(output: &str) -> bool {
    let p = Path::new(output);
    p.is_dir() || output.ends_with('/') || output.ends_with(std::path::MAIN_SEPARATOR)
}

/// Resolve the absolute destination from the user's `-O` value, the server's
/// suggested name and the URL.
fn resolve_final_path(
    target: &str,
    output: Option<&str>,
    server_filename: Option<&str>,
) -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
    let from_remote = || {
        server_filename
            .map(utils::sanitize_filename)
            .unwrap_or_else(|| utils::get_filename_from_url_or_default(target, "downloaded_file"))
    };

    let tentative = match output.map(utils::expand_tilde) {
        Some(out) if is_dir_like(&out) => PathBuf::from(out).join(from_remote()),
        Some(out) => {
            let user_path = PathBuf::from(&out);
            let name = user_path
                .file_name()
                .ok_or_else(|| {
                    format!(
                        "Invalid output path, does not specify a file name: {}",
                        user_path.display()
                    )
                })?
                .to_str()
                .ok_or("Output filename contains invalid characters (non-UTF-8)")?;
            validate_filename(name)?;
            user_path
        }
        None => PathBuf::from(from_remote()),
    };

    if tentative.is_absolute() {
        Ok(tentative)
    } else {
        let cwd = std::env::current_dir()
            .map_err(|e| format!("Failed to get current directory: {}", e))?;
        Ok(cwd.join(tentative))
    }
}

fn ensure_parent_dir(path: &Path, quiet_mode: bool) -> Result<(), Box<dyn Error + Send + Sync>> {
    if let Some(parent_dir) = path.parent()
        && !parent_dir.as_os_str().is_empty()
        && !parent_dir.exists()
    {
        std::fs::create_dir_all(parent_dir)
            .map_err(|e| format!("Failed to create directory {}: {}", parent_dir.display(), e))?;
        print(
            &format!("Created directory: {}", parent_dir.display()),
            quiet_mode,
        );
    }
    Ok(())
}

fn parse_content_range_total(value: &str) -> Option<u64> {
    let (_, total) = value.rsplit_once('/')?;
    total.trim().parse::<u64>().ok()
}

/// Parse the `filename` or `filename*` from a `Content-Disposition` header value.
///
/// Prefers the RFC 5987 `filename*` form (percent-encoded, with charset) over
/// the plain `filename` form.  Returns `None` if neither is present or valid.
pub fn parse_content_disposition_filename(header_value: &str) -> Option<String> {
    let mut plain: Option<String> = None;

    for part in header_value.split(';') {
        let part = part.trim();

        // filename*= (RFC 5987) takes priority
        if let Some(val) = part.strip_prefix("filename*=") {
            let val = val.trim().trim_matches('"');
            // Format: charset'language'encoded-value
            let mut parts = val.splitn(3, '\'');
            let _charset = parts.next();
            let _language = parts.next();
            if let Some(encoded) = parts.next()
                && let Ok(decoded) = urlencoding::decode(encoded)
            {
                let name = decoded.into_owned();
                if !name.is_empty() {
                    return Some(name);
                }
            }
        }

        // Plain filename= (fallback)
        if plain.is_none()
            && let Some(val) = part.strip_prefix("filename=")
        {
            let name = val.trim().trim_matches('"').to_string();
            if !name.is_empty() {
                plain = Some(name);
            }
        }
    }

    plain
}

fn throttle_download(downloaded: u64, started_at: Instant, speed_limit: Option<u64>) {
    let Some(limit) = speed_limit else { return };
    if limit == 0 {
        return;
    }

    let expected_elapsed = Duration::from_secs_f64(downloaded as f64 / limit as f64);
    let actual_elapsed = started_at.elapsed();
    if expected_elapsed > actual_elapsed {
        std::thread::sleep(expected_elapsed - actual_elapsed);
    }
}

/// Verify the integrity of an ISO file by calculating its SHA-256 hash.
///
/// After download, this function calculates the SHA-256 checksum of the file
/// and displays it for manual comparison with the source.
///
/// # Arguments
///
/// * `path` - Path to the ISO file
/// * `callback` - Optional callback for status messages
///
/// # Example
///
/// ```rust,no_run
/// use kget::verify_iso_integrity;
/// use std::path::Path;
///
/// verify_iso_integrity(
///     Path::new("ubuntu-22.04-desktop-amd64.iso"),
///     Some(&|msg| println!("Status: {}", msg)),
/// ).unwrap();
/// ```
///
/// # Output
///
/// Prints the SHA256 hash to stdout and sends it via callback if provided.
pub fn verify_iso_integrity(
    path: &Path,
    callback: Option<&(dyn Fn(String) + Send + Sync)>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    verify_file_sha256(path, None, callback).map(|_| ())
}

/// Calculate a file SHA-256 hash and optionally compare it with an expected value.
pub fn verify_file_sha256(
    path: &Path,
    expected_hash: Option<&str>,
    callback: Option<&(dyn Fn(String) + Send + Sync)>,
) -> Result<String, Box<dyn Error + Send + Sync>> {
    let send = |msg: &str| {
        if let Some(cb) = callback {
            cb(msg.to_string());
        }
    };

    send("Calculating SHA256 hash... (this may take a while for large ISOs)");

    let mut file = File::open(path)?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    let hash = hex::encode(hasher.finalize());

    send("Integrity check finished.");
    send(&format!("SHA256: {}", hash));

    if let Some(expected_hash) = expected_hash {
        let expected_hash = expected_hash.trim().to_ascii_lowercase();
        if hash != expected_hash {
            return Err(Box::new(KgetError::ChecksumMismatch {
                algorithm: "sha256".into(),
                expected: expected_hash,
                got: hash,
            }));
        }
        send("SHA256 matches expected hash.");
    }

    Ok(hash)
}
