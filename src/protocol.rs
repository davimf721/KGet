//! Protocol detection from a URL or path.
//!
//! Every frontend (CLI, interactive mode, GUI worker) uses [`detect`] so a
//! `ftp://`, `sftp://`, `webdav://`, magnet, `.torrent`, Metalink or video URL
//! is routed to the right downloader without extra flags.
//!
//! ```rust
//! use kget::protocol::{detect, Protocol};
//!
//! assert_eq!(detect("ftp://ftp.gnu.org/gnu/hello.tar.gz"), Protocol::Ftp);
//! assert_eq!(detect("magnet:?xt=urn:btih:abc"), Protocol::Torrent);
//! assert_eq!(detect("https://example.com/file.zip"), Protocol::Http);
//! ```

use std::fmt;

/// Download protocol / backend for a URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// Plain HTTP/HTTPS download
    Http,
    /// FTP / FTPS
    Ftp,
    /// SFTP over SSH
    Sftp,
    /// WebDAV (`webdav://`, `webdavs://`)
    WebDav,
    /// Magnet link or `.torrent` file/URL
    Torrent,
    /// Metalink manifest (`.meta4`, `.metalink`)
    Metalink,
    /// Video platform handled by yt-dlp
    Video,
}

impl Protocol {
    /// Lowercase name used in JSONL events (`"http"`, `"ftp"`, …).
    pub fn as_str(&self) -> &'static str {
        match self {
            Protocol::Http => "http",
            Protocol::Ftp => "ftp",
            Protocol::Sftp => "sftp",
            Protocol::WebDav => "webdav",
            Protocol::Torrent => "torrent",
            Protocol::Metalink => "metalink",
            Protocol::Video => "video",
        }
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Detect the protocol for `url` from its scheme and shape.
pub fn detect(url: &str) -> Protocol {
    let lower = url.trim().to_ascii_lowercase();
    let path_part = lower.split(['?', '#']).next().unwrap_or(&lower);
    if lower.starts_with("magnet:?") || path_part.ends_with(".torrent") {
        return Protocol::Torrent;
    }
    if lower.starts_with("ftp://") || lower.starts_with("ftps://") {
        return Protocol::Ftp;
    }
    if lower.starts_with("sftp://") {
        return Protocol::Sftp;
    }
    if crate::webdav::is_webdav_url(&lower) {
        return Protocol::WebDav;
    }
    if crate::metalink::is_metalink(&lower) {
        return Protocol::Metalink;
    }
    if crate::ytdlp::is_video_url(&lower) {
        return Protocol::Video;
    }
    Protocol::Http
}
