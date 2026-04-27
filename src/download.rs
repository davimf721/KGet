//! Simple HTTP/HTTPS download functionality.
//!
//! This module provides basic download capabilities with automatic retry,
//! progress tracking, and ISO integrity verification.
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
//!     quiet_mode: false,
//!     output_path: Some("./file.zip".to_string()),
//!     verify_iso: false,
//!     expected_sha256: None,
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

use crate::DownloadOptions;
use crate::config::ProxyConfig;
use crate::optimization::Optimizer;
use crate::progress::create_progress_bar;
use crate::utils::{self, print};
use humansize::{DECIMAL, format_size};
use mime::Mime;
use reqwest::blocking::Client;
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE};
use sha2::Digest;
use std::error::Error;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const MAX_RETRIES: u32 = 3;
const RETRY_DELAY: Duration = Duration::from_secs(2);

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
/// - Contains directory separators
/// - Is empty
pub fn validate_filename(filename: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    if filename.contains('/') || filename.contains('\\') {
        return Err("Filename cannot contain directory separators".into());
    }
    if filename.is_empty() {
        return Err("Filename cannot be empty".into());
    }
    Ok(())
}

/// Download a file from a URL with automatic retry and progress tracking.
///
/// This is the simple download function for basic use cases. For parallel
/// connections and resume support, use [`AdvancedDownloader`](crate::AdvancedDownloader).
///
/// # Arguments
///
/// * `target` - URL to download
/// * `proxy` - Proxy configuration (use `ProxyConfig::default()` for no proxy)
/// * `_optimizer` - Optimizer instance (reserved for future use)
/// * `options` - Download options (quiet mode, output path, ISO verification)
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
///         quiet_mode: false,
///         output_path: None, // Uses filename from URL
///         verify_iso: true,  // Verify SHA256 after download
///         expected_sha256: None,
///     },
///     None,
/// ).unwrap();
/// ```
///
/// # Errors
///
/// Returns an error if:
/// - Network connection fails after MAX_RETRIES attempts
/// - HTTP response indicates an error
/// - Insufficient disk space
/// - File cannot be created
pub fn download(
    target: &str,
    proxy: ProxyConfig,
    _optimizer: Optimizer,
    options: DownloadOptions,
    status_callback: Option<&(dyn Fn(String) + Send + Sync)>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let quiet_mode = options.quiet_mode;

    let mut client_builder = Client::builder()
        .timeout(Duration::from_secs(30))
        .no_gzip()
        .no_deflate();

    if proxy.enabled {
        if let Some(proxy_url) = &proxy.url {
            let proxy_client = match proxy.proxy_type {
                crate::config::ProxyType::Http => reqwest::Proxy::http(proxy_url),
                crate::config::ProxyType::Https => reqwest::Proxy::https(proxy_url),
                crate::config::ProxyType::Socks5 => reqwest::Proxy::all(proxy_url),
            };
            if let Ok(mut proxy_client) = proxy_client {
                if let (Some(username), Some(password)) = (&proxy.username, &proxy.password) {
                    proxy_client = proxy_client.basic_auth(username, password);
                }
                client_builder = client_builder.proxy(proxy_client);
            }
        }
    }

    let client = client_builder.build()?;

    let mut retries = 0;
    let response = loop {
        match client.get(target).send() {
            Ok(resp) => break resp,
            Err(e) => {
                retries += 1;
                if retries >= MAX_RETRIES {
                    return Err(format!("Failed after {} attempts: {}", MAX_RETRIES, e).into());
                }
                print(
                    &format!(
                        "Attempt {} failed, retrying in {} seconds...",
                        retries,
                        RETRY_DELAY.as_secs()
                    ),
                    quiet_mode,
                );
                std::thread::sleep(RETRY_DELAY);
            }
        }
    };

    print(
        &format!("HTTP request sent... {}", response.status()),
        quiet_mode,
    );

    if !response.status().is_success() {
        return Err(format!("HTTP error: {}", response.status()).into());
    }

    let content_length = response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|ct_len| ct_len.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());

    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|ct| ct.to_str().ok())
        .and_then(|s| s.parse::<Mime>().ok());

    if let Some(len) = content_length {
        print(
            &format!("Length: {} ({})", len, format_size(len, DECIMAL)),
            quiet_mode,
        );
    } else {
        print("Length: unknown", quiet_mode);
    }

    if let Some(ref ct) = content_type {
        print(&format!("Type: {}", ct), quiet_mode);
    }

    let is_iso = target.to_lowercase().ends_with(".iso")
        || content_type.as_ref().map_or(false, |ct| {
            ct.essence_str() == "application/x-iso9001"
                || ct.essence_str() == "application/x-cd-image"
        });

    if is_iso {
        print(
            "ISO file detected. Ensuring raw download to prevent corruption...",
            quiet_mode,
        );
    }

    let tentative_path: PathBuf;

    if let Some(output_arg_str) = options.output_path {
        let user_path = PathBuf::from(output_arg_str.clone());

        let is_target_dir =
            user_path.is_dir() || output_arg_str.ends_with(std::path::MAIN_SEPARATOR);

        if is_target_dir {
            let base_filename = utils::get_filename_from_url_or_default(target, "downloaded_file");
            validate_filename(&base_filename)?;
            tentative_path = user_path.join(base_filename);
        } else {
            if let Some(file_name_osstr) = user_path.file_name() {
                if let Some(file_name_str) = file_name_osstr.to_str() {
                    if file_name_str.is_empty() {
                        return Err(format!(
                            "Invalid output path, does not specify a file name: {}",
                            user_path.display()
                        )
                        .into());
                    }
                    validate_filename(file_name_str)?;
                } else {
                    return Err("Output filename contains invalid characters (non-UTF-8)".into());
                }
            } else {
                return Err(format!(
                    "Invalid output path, does not specify a file name: {}",
                    user_path.display()
                )
                .into());
            }
            tentative_path = user_path;
        }
    } else {
        let base_filename = utils::get_filename_from_url_or_default(target, "downloaded_file");
        validate_filename(&base_filename)?;
        tentative_path = PathBuf::from(base_filename);
    }

    let final_path: PathBuf = if tentative_path.is_absolute() {
        tentative_path
    } else {
        let current_dir = std::env::current_dir()
            .map_err(|e| format!("Failed to get current directory: {}", e))?;
        current_dir.join(tentative_path)
    };

    if let Some(parent_dir) = final_path.parent() {
        if !parent_dir.as_os_str().is_empty()
            && parent_dir != Path::new("/")
            && !parent_dir.exists()
        {
            std::fs::create_dir_all(parent_dir).map_err(|e| {
                format!("Failed to create directory {}: {}", parent_dir.display(), e)
            })?;
            if !quiet_mode {
                print(
                    &format!("Created directory: {}", parent_dir.display()),
                    quiet_mode,
                );
            }
        }
    }

    if !quiet_mode {
        print(&format!("Saving to: {}", final_path.display()), quiet_mode);
    }

    if let Some(len) = content_length {
        check_disk_space(&final_path, len)?;
    }

    let mut dest = File::create(&final_path)
        .map_err(|e| format!("Failed to create file {}: {}", final_path.display(), e))?;

    let response_content_length = response.content_length();
    let progress_bar_filename = final_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let progress = create_progress_bar(
        quiet_mode,
        progress_bar_filename,
        response_content_length,
        false,
    );

    let mut source = response.take(response_content_length.unwrap_or(u64::MAX));
    let mut buffered_reader = progress.wrap_read(&mut source);

    // Stream data instead of reading all into memory
    let mut buffer = [0u8; 8192];
    loop {
        let n = buffered_reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        dest.write_all(&buffer[..n])?;
    }

    progress.finish_with_message("Download completed\n");

    if is_iso && options.verify_iso {
        verify_file_sha256(
            &final_path,
            options.expected_sha256.as_deref(),
            status_callback,
        )?;
    } else if let Some(expected) = options.expected_sha256.as_deref() {
        verify_file_sha256(&final_path, Some(expected), status_callback)?;
    }

    Ok(())
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
    let msg = "Calculating SHA256 hash... (this may take a while for large ISOs)";
    if let Some(cb) = callback {
        cb(msg.to_string());
    }
    println!("{}", msg);

    let mut file = File::open(path)?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = [0; 8192];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    let hash = hex::encode(hasher.finalize());

    let msg_done = "Integrity check finished.";
    if let Some(cb) = callback {
        cb(msg_done.to_string());
    }
    println!("{}", msg_done);

    let msg_hash = format!("SHA256: {}", hash);
    if let Some(cb) = callback {
        cb(msg_hash.clone());
    }
    println!("SHA256: {}", hash);

    if let Some(expected_hash) = expected_hash {
        let expected_hash = expected_hash.trim().to_ascii_lowercase();
        if hash != expected_hash {
            return Err(
                format!("SHA256 mismatch: expected {}, got {}", expected_hash, hash).into(),
            );
        }

        let msg_match = "SHA256 matches expected hash.";
        if let Some(cb) = callback {
            cb(msg_match.to_string());
        }
        println!("{}", msg_match);
    } else {
        println!("You can compare this hash with the one provided by the source.");
    }

    Ok(hash)
}
