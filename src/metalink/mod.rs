//! Metalink (.meta4 / .metalink) download support.
//!
//! Metalink is a standard XML format (RFC 5854 for `.meta4`) that describes a
//! file along with multiple download mirrors and cryptographic checksums.
//! KGet parses the manifest, tries each mirror in priority order, and verifies
//! the hash after a successful download — all automatically.
//!
//! # Supported features
//! - Metalink 4 (RFC 5854, `.meta4`) and Metalink 3 (`.metalink`) manifests
//! - Multiple mirrors: v4 `priority` (lower = preferred) and v3 `preference`
//!   (higher = preferred)
//! - `sha-256`, `sha-512`, and `md5` hash types, all verified (sha-256 preferred)
//! - The manifest `<size>` is checked against the downloaded file
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
//!     "~/Downloads", // `~` is expanded
//!     false,
//!     ProxyConfig::default(),
//!     Optimizer::new(),
//! ).unwrap();
//! ```

use crate::checksum::{ChecksumAlgorithm, compute_checksum};
use crate::config::ProxyConfig;
use crate::http::ClientOptions;
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
/// Both the RFC 5854 format (`<metalink><file>…`) and the older Metalink
/// 3.x format (`<metalink><files><file>…<resources><url>…`) are accepted.
pub fn parse(content: &str) -> Result<MetalinkDoc, Box<dyn Error + Send + Sync>> {
    let doc = roxmltree::Document::parse(content)
        .map_err(|e| format!("Metalink XML parse error: {}", e))?;

    let root = doc.root_element();
    let mut files: Vec<MetalinkFile> = Vec::new();

    for file_node in root
        .descendants()
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

        for child in file_node.descendants().filter(|n| n.is_element()) {
            match child.tag_name().name() {
                "size" => {
                    size = child.text().and_then(|t| t.trim().parse::<u64>().ok());
                }
                // Piece hashes (v3 <pieces>, v4 <pieces>) are not whole-file hashes.
                "hash"
                    if child
                        .parent_element()
                        .is_some_and(|p| p.tag_name().name() == "pieces") => {}
                "hash" => {
                    let hash_type = child.attribute("type").unwrap_or("").to_lowercase();
                    let value = child.text().map(|t| t.trim().to_ascii_lowercase());
                    match hash_type.as_str() {
                        "sha-256" | "sha256" => sha256 = value,
                        "sha-512" | "sha512" => sha512 = value,
                        "md5" => md5 = value,
                        _ => {}
                    }
                }
                "url" => {
                    // v4 priority: lower is better; v3 preference: higher is better (1–100)
                    let priority = child
                        .attribute("priority")
                        .and_then(|p| p.parse::<u32>().ok())
                        .or_else(|| {
                            child
                                .attribute("preference")
                                .and_then(|p| p.parse::<u32>().ok())
                                .map(|p| 101u32.saturating_sub(p))
                        })
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
    download_metalink_with_status(source, output_dir, quiet, proxy, optimizer, None)
}

/// [`download_metalink`] with a status callback.
///
/// The callback receives a [`verifying_status`](crate::events::verifying_status)
/// message before each checksum is computed.
pub fn download_metalink_with_status(
    source: &str,
    output_dir: &str,
    quiet: bool,
    proxy: ProxyConfig,
    optimizer: Optimizer,
    status: Option<&(dyn Fn(String) + Send + Sync)>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let xml = fetch_manifest(source, quiet, &proxy)?;
    let manifest = parse(&xml)?;

    if !quiet {
        println!("Metalink: {} file(s) to download", manifest.files.len());
    }

    let out_dir = PathBuf::from(crate::utils::expand_tilde(output_dir));
    if !out_dir.exists() {
        std::fs::create_dir_all(&out_dir)?;
    }

    for file in &manifest.files {
        download_one_file(file, &out_dir, quiet, &proxy, &optimizer, status)?;
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
        let client = crate::http::build_client(proxy, &ClientOptions::default())?;
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

/// Download a single metalink file, trying each mirror in order.
fn download_one_file(
    file: &MetalinkFile,
    output_dir: &Path,
    quiet: bool,
    proxy: &ProxyConfig,
    optimizer: &Optimizer,
    status: Option<&(dyn Fn(String) + Send + Sync)>,
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

                if let Some(expected) = file.size {
                    let got = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
                    if got != expected {
                        let _ = std::fs::remove_file(&dest);
                        last_error = Some(
                            format!(
                                "Size mismatch on mirror {}: expected {expected}, got {got}",
                                mirror.url
                            )
                            .into(),
                        );
                        continue;
                    }
                }

                // Verify the strongest hash the manifest provides.
                let checks = [
                    (file.sha256.as_deref(), ChecksumAlgorithm::Sha256),
                    (file.sha512.as_deref(), ChecksumAlgorithm::Sha512),
                    (file.md5.as_deref(), ChecksumAlgorithm::Md5),
                ];
                if let Some((expected, algo)) =
                    checks.into_iter().find_map(|(h, a)| h.map(|h| (h, a)))
                {
                    if !quiet {
                        println!("  Verifying {}...", algo.name());
                    }
                    if let Some(cb) = status {
                        cb(crate::events::verifying_status(algo.name()));
                    }
                    let got = compute_checksum(&dest, &algo)?;
                    if got != expected {
                        eprintln!(
                            "  {} mismatch: expected {}, got {}",
                            algo.name(),
                            expected,
                            got
                        );
                        // Remove the corrupted file and try the next mirror.
                        let _ = std::fs::remove_file(&dest);
                        last_error = Some(Box::new(crate::KgetError::ChecksumMismatch {
                            algorithm: algo.name().to_string(),
                            expected: expected.to_string(),
                            got,
                        }));
                        continue;
                    }
                    if !quiet {
                        println!("  {} OK ✓", algo.name());
                    }
                }

                return Ok(());
            }
            Err(e) => {
                if !quiet {
                    eprintln!("  Mirror failed: {}", e);
                }
                // Never let the next mirror resume another mirror's partial data.
                discard_partial(&dest);
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

    let mut dl = AdvancedDownloader::new(
        url.to_string(),
        output_path,
        quiet,
        proxy.clone(),
        optimizer.clone(),
    )?;
    dl.set_resume_policy(crate::advanced_download::ResumePolicy::AlwaysResume);
    dl.download()
}

fn discard_partial(dest: &Path) {
    let part = crate::download::part_path_for(dest);
    let _ = std::fs::remove_file(&part);
    let mut state = dest.as_os_str().to_os_string();
    state.push(".kget");
    let _ = std::fs::remove_file(PathBuf::from(state));
}

/// Remove characters that are unsafe in filenames across platforms.
fn sanitise_filename(name: &str) -> String {
    // Manifest names may contain directories ("dir/file"); keep only the file name.
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    crate::utils::sanitize_filename(base)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_metalink_v3() {
        let xml = r#"<?xml version="1.0"?>
<metalink version="3.0" xmlns="http://www.metalinker.org/">
  <files>
    <file name="ex.iso">
      <size>42</size>
      <verification><hash type="sha512">ABCD</hash></verification>
      <resources>
        <url type="http" preference="10">http://slow/ex.iso</url>
        <url type="http" preference="100">http://fast/ex.iso</url>
      </resources>
    </file>
  </files>
</metalink>"#;
        let doc = parse(xml).unwrap();
        assert_eq!(doc.files.len(), 1);
        let f = &doc.files[0];
        assert_eq!(f.size, Some(42));
        assert_eq!(f.sha512.as_deref(), Some("abcd"));
        assert_eq!(f.urls[0].url, "http://fast/ex.iso");
    }

    #[test]
    fn parses_metalink_v4() {
        let xml = r#"<?xml version="1.0"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="a.bin">
    <hash type="sha-256">00ff</hash>
    <url priority="2">http://b/a.bin</url>
    <url priority="1">http://a/a.bin</url>
  </file>
</metalink>"#;
        let doc = parse(xml).unwrap();
        assert_eq!(doc.files[0].urls[0].url, "http://a/a.bin");
        assert_eq!(doc.files[0].sha256.as_deref(), Some("00ff"));
    }

    #[test]
    fn sanitises_directory_names() {
        assert_eq!(sanitise_filename("dir/sub/file.iso"), "file.iso");
        assert_eq!(sanitise_filename(".."), "download");
    }
}
