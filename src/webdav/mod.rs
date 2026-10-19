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
use crate::download::download_to_path as http_download;
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
    options: DownloadOptions,
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
        let (http_url, username, password) =
            parse_webdav_url(&url).unwrap_or_else(|_| (url.clone(), None, None));
        Self {
            http_url,
            output,
            quiet,
            proxy,
            optimizer,
            username,
            password,
            options: DownloadOptions::default(),
        }
    }

    /// Extra options (resume, retries, SHA-256, progress callback…) for the
    /// underlying HTTP download. `quiet_mode` and `output_path` are ignored.
    pub fn with_options(mut self, options: DownloadOptions) -> Self {
        self.options = options;
        self
    }

    /// Download the resource and return the path of the saved file.
    pub fn download_to_path(
        &self,
        status_callback: Option<&(dyn Fn(String) + Send + Sync)>,
    ) -> Result<std::path::PathBuf, Box<dyn Error + Send + Sync>> {
        let mut options = self.options.clone();
        options.quiet_mode = self.quiet;
        options.output_path = Some(self.output.clone());

        if let Some(user) = &self.username {
            if self.http_url.starts_with("http://") && !self.quiet {
                eprintln!("Warning: sending WebDAV credentials without TLS (use webdavs://)");
            }
            let pass = self.password.as_deref().unwrap_or("");
            let creds = base64_encode(format!("{user}:{pass}").as_bytes());
            options
                .extra_headers
                .push(("Authorization".to_string(), format!("Basic {creds}")));
        }

        http_download(
            &self.http_url,
            self.proxy.clone(),
            self.optimizer.clone(),
            options,
            status_callback,
        )
    }

    /// Download the resource.
    pub fn download(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.download_to_path(None).map(|_| ())
    }
}

// ────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────

/// Convert `webdav://user:pass@host/path` → (`http://host/path`, user, pass).
///
/// Credentials are percent-decoded; `@` characters in the path are preserved.
fn parse_webdav_url(
    raw: &str,
) -> Result<(String, Option<String>, Option<String>), url::ParseError> {
    let mut u = url::Url::parse(raw)?;
    let scheme = if u.scheme().eq_ignore_ascii_case("webdavs") {
        "https"
    } else {
        "http"
    };
    let decode = |s: &str| {
        urlencoding::decode(s)
            .map(|c| c.into_owned())
            .unwrap_or_else(|_| s.to_string())
    };
    let user = (!u.username().is_empty()).then(|| decode(u.username()));
    let pass = u.password().map(decode);
    let _ = u.set_username("");
    let _ = u.set_password(None);
    // Url::set_scheme can't switch between "special" and non-special schemes,
    // so rebuild the string instead.
    let http_url = format!("{scheme}{}", &u.as_str()[u.scheme().len()..]);
    Ok((http_url, user, pass))
}

/// Minimal RFC 4648 base64 encoder — avoids adding a `base64` crate.
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = if chunk.len() > 1 {
            chunk[1] as usize
        } else {
            0
        };
        let b2 = if chunk.len() > 2 {
            chunk[2] as usize
        } else {
            0
        };
        out.push(CHARS[b0 >> 2] as char);
        out.push(CHARS[((b0 & 3) << 4) | (b1 >> 4)] as char);
        out.push(if chunk.len() > 1 {
            CHARS[((b1 & 15) << 2) | (b2 >> 6)] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            CHARS[b2 & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plain_webdav() {
        let (url, user, pass) = parse_webdav_url("webdav://nas.local/files/report.pdf").unwrap();
        assert_eq!(url, "http://nas.local/files/report.pdf");
        assert!(user.is_none() && pass.is_none());
    }

    #[test]
    fn parse_webdavs_with_credentials() {
        let (url, user, pass) =
            parse_webdav_url("webdavs://alice:secret@cloud.example.com/docs/file.zip").unwrap();
        assert_eq!(url, "https://cloud.example.com/docs/file.zip");
        assert_eq!(user.as_deref(), Some("alice"));
        assert_eq!(pass.as_deref(), Some("secret"));
    }

    #[test]
    fn parse_encoded_credentials_and_at_in_path() {
        let (url, user, pass) =
            parse_webdav_url("webdavs://bob:p%40ss@host.example/users/@me/f.txt").unwrap();
        assert_eq!(url, "https://host.example/users/@me/f.txt");
        assert_eq!(user.as_deref(), Some("bob"));
        assert_eq!(pass.as_deref(), Some("p@ss"));
    }

    #[test]
    fn base64_roundtrip() {
        assert_eq!(base64_encode(b"user:pass"), "dXNlcjpwYXNz");
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b""), "");
    }
}
