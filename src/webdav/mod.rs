//! WebDAV protocol adapter.
//!
//! Handles `webdav://` and `webdavs://` URLs by converting them to
//! `http://` / `https://` and injecting a `Basic` authentication header
//! when credentials are embedded in the URL.
//!
//! WebDAV files are fetched via ordinary HTTP GET — servers that implement
//! RFC 4918 always support GET for existing resources, so no special
//! PROPFIND round-trip is needed for plain file downloads.

use crate::DownloadOptions;
use crate::config::ProxyConfig;
use crate::download::download as http_download;
use crate::optimization::Optimizer;
use std::error::Error;

/// Returns `true` if the URL uses the `webdav://` or `webdavs://` scheme.
pub fn is_webdav_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    lower.starts_with("webdav://") || lower.starts_with("webdavs://")
}

/// Downloads a WebDAV resource.
pub struct WebDavDownloader {
    http_url: String,
    output: String,
    quiet: bool,
    proxy: ProxyConfig,
    optimizer: Optimizer,
    username: Option<String>,
    password: Option<String>,
}

impl WebDavDownloader {
    /// Create a new downloader from a `webdav://` or `webdavs://` URL.
    ///
    /// Credentials embedded in the URL (`webdav://user:pass@host/path`) are
    /// extracted and used for HTTP Basic authentication.
    pub fn new(
        url: String,
        output: String,
        quiet: bool,
        proxy: ProxyConfig,
        optimizer: Optimizer,
    ) -> Self {
        let (http_url, username, password) = parse_webdav_url(&url);
        Self { http_url, output, quiet, proxy, optimizer, username, password }
    }

    pub fn download(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut extra_headers: Vec<(String, String)> = Vec::new();

        if let Some(user) = &self.username {
            let pass = self.password.as_deref().unwrap_or("");
            let creds = base64_encode(format!("{user}:{pass}").as_bytes());
            extra_headers.push(("Authorization".to_string(), format!("Basic {creds}")));
        }

        let options = DownloadOptions {
            quiet_mode: self.quiet,
            output_path: Some(self.output.clone()),
            verify_iso: false,
            expected_sha256: None,
            extra_headers,
        };

        http_download(
            &self.http_url,
            self.proxy.clone(),
            self.optimizer.clone(),
            options,
            None,
        )
    }
}

// ────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────

/// Convert `webdav://user:pass@host/path` → (`http://host/path`, user, pass).
fn parse_webdav_url(url: &str) -> (String, Option<String>, Option<String>) {
    let (scheme, rest) = if url.to_lowercase().starts_with("webdavs://") {
        ("https", &url["webdavs://".len()..])
    } else {
        ("http", &url["webdav://".len()..])
    };

    // Check for embedded credentials: user:pass@host/path
    if let Some(at) = rest.find('@') {
        let creds = &rest[..at];
        let host_path = &rest[at + 1..];
        let http_url = format!("{scheme}://{host_path}");
        if let Some(colon) = creds.find(':') {
            return (
                http_url,
                Some(creds[..colon].to_string()),
                Some(creds[colon + 1..].to_string()),
            );
        }
        return (http_url, Some(creds.to_string()), None);
    }

    (format!("{scheme}://{rest}"), None, None)
}

/// Minimal RFC 4648 base64 encoder — avoids adding a `base64` crate.
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = if chunk.len() > 1 { chunk[1] as usize } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as usize } else { 0 };
        out.push(CHARS[b0 >> 2] as char);
        out.push(CHARS[((b0 & 3) << 4) | (b1 >> 4)] as char);
        out.push(if chunk.len() > 1 { CHARS[((b1 & 15) << 2) | (b2 >> 6)] as char } else { '=' });
        out.push(if chunk.len() > 2 { CHARS[b2 & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plain_webdav() {
        let (url, user, pass) = parse_webdav_url("webdav://nas.local/files/report.pdf");
        assert_eq!(url, "http://nas.local/files/report.pdf");
        assert!(user.is_none() && pass.is_none());
    }

    #[test]
    fn parse_webdavs_with_credentials() {
        let (url, user, pass) = parse_webdav_url("webdavs://alice:secret@cloud.example.com/docs/file.zip");
        assert_eq!(url, "https://cloud.example.com/docs/file.zip");
        assert_eq!(user.as_deref(), Some("alice"));
        assert_eq!(pass.as_deref(), Some("secret"));
    }

    #[test]
    fn base64_roundtrip() {
        assert_eq!(base64_encode(b"user:pass"), "dXNlcjpwYXNz");
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b""), "");
    }
}
