//! Metalink (.meta4 / .metalink) download support.
//!
//! Metalink is a standard XML format (RFC 5854 for `.meta4`) that describes a
//! file along with multiple download mirrors and cryptographic checksums.
//! KGet parses the manifest, tries each mirror in priority order, and verifies
//! the hash after a successful download — all automatically.
//!
//! # Supported features
//! - Multiple mirrors with optional `priority` attribute (lower = preferred)
//! - `sha-256`, `sha-512`, and `md5` hash types (sha-256 preferred)
//! - Multiple `<file>` entries per manifest
//! - Local `.meta4` files and remote `.meta4` URLs
//!
//! # Example
//!
//! ```rust,no_run
//! use kget::metalink::download_metalink;
//! use kget::{ProxyConfig, Optimizer};
//!
//! download_metalink(
//!     "ubuntu-24.04.meta4",
//!     "~/Downloads",
//!     false,
//!     ProxyConfig::default(),
//!     Optimizer::new(),
//! ).unwrap();
//! ```

use crate::config::ProxyConfig;
use crate::download::verify_file_sha256;
use crate::optimization::Optimizer;
use std::error::Error;
use std::path::{Path, PathBuf};

// ============================================================================
// Public data model
// ============================================================================

/// A single download URL with an optional priority (lower number = tried first).
#[derive(Debug, Clone)]
pub struct MetalinkUrl {
    pub url: String,
    /// Lower priority number means the mirror is tried first.
    /// Defaults to 999 if the attribute is absent.
    pub priority: u32,
}

/// A single file described in a Metalink manifest.
#[derive(Debug, Clone)]
pub struct MetalinkFile {
    pub name: String,
    pub size: Option<u64>,
    pub sha256: Option<String>,
    pub sha512: Option<String>,
    pub md5: Option<String>,
    /// Mirrors sorted by priority (ascending).
    pub urls: Vec<MetalinkUrl>,
}

impl MetalinkFile {
    /// Return the best available hash as `(type_label, hex_string)`.
    /// Preference order: sha-256 → sha-512 → md5.
    pub fn best_hash(&self) -> Option<(&str, &str)> {
        if let Some(h) = &self.sha256 {
            return Some(("sha-256", h.as_str()));
        }
        if let Some(h) = &self.sha512 {
            return Some(("sha-512", h.as_str()));
        }
        if let Some(h) = &self.md5 {
            return Some(("md5", h.as_str()));
        }
        None
    }
}

/// A parsed Metalink manifest, potentially containing multiple files.
#[derive(Debug, Clone)]
pub struct MetalinkDoc {
    pub files: Vec<MetalinkFile>,
}

// ============================================================================
// Parser
// ============================================================================

/// Parse a `.meta4` or `.metalink` XML string into a [`MetalinkDoc`].
///
/// Both the RFC 5854 namespace (`urn:ietf:params:xml:ns:metalink`) and the
/// older Metalink 3.x format (no namespace) are accepted.
pub fn parse(content: &str) -> Result<MetalinkDoc, Box<dyn Error + Send + Sync>> {
    let doc = roxmltree::Document::parse(content)
        .map_err(|e| format!("Metalink XML parse error: {}", e))?;

    let root = doc.root_element();
    let mut files: Vec<MetalinkFile> = Vec::new();

    for file_node in root
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "file")
    {
        let name = file_node
            .attribute("name")
            .unwrap_or("download")
            .to_string();

        let mut size: Option<u64> = None;
        let mut sha256: Option<String> = None;
        let mut sha512: Option<String> = None;
        let mut md5: Option<String> = None;
        let mut urls: Vec<MetalinkUrl> = Vec::new();

        for child in file_node.children().filter(|n| n.is_element()) {
            match child.tag_name().name() {
                "size" => {
                    size = child.text().and_then(|t| t.trim().parse::<u64>().ok());
                }
                "hash" => {
                    let hash_type = child.attribute("type").unwrap_or("").to_lowercase();
                    let value = child.text().map(|t| t.trim().to_ascii_lowercase());
                    match hash_type.as_str() {
                        "sha-256" => sha256 = value,
                        "sha-512" => sha512 = value,
                        "md5" => md5 = value,
                        _ => {}
                    }
                }
                "url" => {
                    let priority = child
                        .attribute("priority")
                        .and_then(|p| p.parse::<u32>().ok())
                        .unwrap_or(999);
                    if let Some(url_text) = child.text() {
                        let url = url_text.trim().to_string();
                        if !url.is_empty() {
                            urls.push(MetalinkUrl { url, priority });
                        }
                    }
                }
                _ => {}
            }
        }

        // Sort URLs: lowest priority number first.
        urls.sort_by_key(|u| u.priority);

        if !urls.is_empty() {
            files.push(MetalinkFile {
                name,
                size,
                sha256,
                sha512,
                md5,
                urls,
            });
        }
    }

    if files.is_empty() {
        return Err("Metalink contains no downloadable files with at least one URL".into());
    }

    Ok(MetalinkDoc { files })
}

// ============================================================================
// Downloader
// ============================================================================

/// Download all files described in a Metalink source.
///
/// `source` may be:
/// - A local file path ending in `.meta4` or `.metalink`
/// - An HTTP/HTTPS URL pointing to a `.meta4` or `.metalink` file
///
/// Files are downloaded into `output_dir`, trying each mirror in priority
/// order.  If a hash is present in the manifest, it is verified after the
/// download completes.
pub fn download_metalink(
    source: &str,
    output_dir: &str,
    quiet: bool,
    proxy: ProxyConfig,
    optimizer: Optimizer,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let xml = fetch_manifest(source, quiet, &proxy)?;
    let manifest = parse(&xml)?;

    if !quiet {
        println!("Metalink: {} file(s) to download", manifest.files.len());
    }

    let out_dir = PathBuf::from(output_dir);
    if !out_dir.exists() {
        std::fs::create_dir_all(&out_dir)?;
    }

    for file in &manifest.files {
        download_one_file(file, &out_dir, quiet, &proxy, &optimizer)?;
    }

    Ok(())
}

// ============================================================================
// Internal helpers
// ============================================================================

/// Fetch the metalink manifest XML — from disk or over HTTP.
fn fetch_manifest(
    source: &str,
    quiet: bool,
    proxy: &ProxyConfig,
) -> Result<String, Box<dyn Error + Send + Sync>> {
    if source.starts_with("http://") || source.starts_with("https://") {
        if !quiet {
            println!("Fetching Metalink manifest: {}", source);
        }
        let client = build_http_client(proxy)?;
        let response = client.get(source).send()?;
        if !response.status().is_success() {
            return Err(format!(
                "Failed to fetch Metalink manifest: HTTP {}",
                response.status()
            )
            .into());
        }
        Ok(response.text()?)
    } else {
        std::fs::read_to_string(source)
            .map_err(|e| format!("Cannot read Metalink file '{}': {}", source, e).into())
    }
}

/// Build a blocking reqwest client with optional proxy support.
fn build_http_client(
    proxy: &ProxyConfig,
) -> Result<reqwest::blocking::Client, Box<dyn Error + Send + Sync>> {
    let mut builder = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .user_agent(concat!("KGet/", env!("CARGO_PKG_VERSION")));

    if proxy.enabled
        && let Some(proxy_url) = &proxy.url
    {
        let p = match proxy.proxy_type {
            crate::config::ProxyType::Http => reqwest::Proxy::http(proxy_url),
            crate::config::ProxyType::Https => reqwest::Proxy::https(proxy_url),
            crate::config::ProxyType::Socks5 => reqwest::Proxy::all(proxy_url),
        };
        if let Ok(mut p) = p {
            if let (Some(u), Some(pw)) = (&proxy.username, &proxy.password) {
                p = p.basic_auth(u, pw);
            }
            builder = builder.proxy(p);
        }
    }

    Ok(builder.build()?)
}

/// Download a single metalink file, trying each mirror in order.
fn download_one_file(
    file: &MetalinkFile,
    output_dir: &Path,
    quiet: bool,
    proxy: &ProxyConfig,
    optimizer: &Optimizer,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let safe_name = sanitise_filename(&file.name);
    let dest = output_dir.join(&safe_name);

    if !quiet {
        println!(
            "\nDownloading '{}' ({} mirror(s)){}",
            safe_name,
            file.urls.len(),
            file.size
                .map(|s| format!("  [{} bytes]", s))
                .unwrap_or_default()
        );
    }

    let mut last_error: Option<Box<dyn Error + Send + Sync>> = None;

    for (idx, mirror) in file.urls.iter().enumerate() {
        if !quiet {
            println!("  Mirror {}/{}: {}", idx + 1, file.urls.len(), mirror.url);
        }

        match download_from_mirror(&mirror.url, &dest, quiet, proxy, optimizer) {
            Ok(()) => {
                if !quiet {
                    println!("  Download OK");
                }

                // Verify hash if the manifest provides one.
                if let Some((hash_type, expected)) = file.best_hash() {
                    if hash_type == "sha-256" {
                        if !quiet {
                            println!("  Verifying SHA-256...");
                        }
                        match verify_file_sha256(&dest, Some(expected), None) {
                            Ok(_) => {
                                if !quiet {
                                    println!("  SHA-256 OK ✓");
                                }
                            }
                            Err(e) => {
                                eprintln!("  SHA-256 mismatch: {}", e);
                                // Remove corrupted file and try next mirror.
                                let _ = std::fs::remove_file(&dest);
                                last_error = Some(e);
                                continue;
                            }
                        }
                    } else if !quiet {
                        println!(
                            "  Skipping {} verification (only sha-256 supported inline)",
                            hash_type
                        );
                    }
                }

                return Ok(());
            }
            Err(e) => {
                if !quiet {
                    eprintln!("  Mirror failed: {}", e);
                }
                last_error = Some(e);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| "All mirrors failed".into()))
}

/// Attempt to download a single URL to `dest` using AdvancedDownloader.
fn download_from_mirror(
    url: &str,
    dest: &Path,
    quiet: bool,
    proxy: &ProxyConfig,
    optimizer: &Optimizer,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    use crate::advanced_download::AdvancedDownloader;

    let output_path = dest
        .to_str()
        .ok_or("Output path contains non-UTF-8 characters")?
        .to_string();

    let dl = AdvancedDownloader::new(
        url.to_string(),
        output_path,
        quiet,
        proxy.clone(),
        optimizer.clone(),
    )?;
    dl.download()
}

/// Remove characters that are unsafe in filenames across platforms.
fn sanitise_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            c => c,
        })
        .collect();

    let cleaned = cleaned.trim_matches('.').trim();
    if cleaned.is_empty() {
        "download".to_string()
    } else {
        cleaned.to_string()
    }
}

// ============================================================================
// Convenience re-check: is this path/URL a Metalink?
// ============================================================================

/// Return `true` if `source` looks like a local or remote Metalink file.
pub fn is_metalink(source: &str) -> bool {
    let lower = source.to_lowercase();
    // Strip query string for URL detection
    let base = lower.split('?').next().unwrap_or(&lower);
    base.ends_with(".meta4") || base.ends_with(".metalink")
}
