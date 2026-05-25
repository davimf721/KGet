//! Strongly-typed error enum for the KGet public API.
//!
//! [`KgetError`] replaces `Box<dyn Error>` at every public boundary so callers
//! can `match` on failure cases rather than downcasting opaque trait objects.

use std::fmt;

/// All errors that can arise from a KGet download operation.
#[derive(Debug)]
pub enum KgetError {
    /// An HTTP/network transport error (wraps the reqwest message).
    Network(String),

    /// A local filesystem I/O error.
    Io(std::io::Error),

    /// The downloaded file's checksum does not match the expectation.
    ChecksumMismatch {
        /// Algorithm that was used (e.g. "sha256", "blake3").
        algorithm: String,
        /// The expected (user-supplied) hash.
        expected: String,
        /// The hash computed from the file on disk.
        got: String,
    },

    /// A protocol-level error (e.g. bad URL scheme, FTP failure).
    Protocol(String),

    /// The download was explicitly cancelled.
    Cancelled,

    /// The remote resource returned HTTP 404 / was not found.
    NotFound(String),

    /// A checksum sidecar file could not be fetched or parsed.
    SidecarError(String),

    /// Catch-all for errors that don't fit a more specific variant.
    Other(String),
}

impl fmt::Display for KgetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KgetError::Network(e) =>
                write!(f, "Network error: {e}"),
            KgetError::Io(e) =>
                write!(f, "IO error: {e}"),
            KgetError::ChecksumMismatch { algorithm, expected, got } =>
                write!(f, "{algorithm} mismatch — expected {expected}, got {got}"),
            KgetError::Protocol(e) =>
                write!(f, "Protocol error: {e}"),
            KgetError::Cancelled =>
                write!(f, "Download cancelled"),
            KgetError::NotFound(url) =>
                write!(f, "Resource not found: {url}"),
            KgetError::SidecarError(e) =>
                write!(f, "Checksum sidecar error: {e}"),
            KgetError::Other(e) =>
                write!(f, "{e}"),
        }
    }
}

impl std::error::Error for KgetError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            KgetError::Io(e) => Some(e),
            _ => None,
        }
    }
}

// ── From impls ────────────────────────────────────────────────────────────────

impl From<std::io::Error> for KgetError {
    fn from(e: std::io::Error) -> Self { KgetError::Io(e) }
}

impl From<reqwest::Error> for KgetError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_status() {
            if let Some(s) = e.status() {
                if s.as_u16() == 404 {
                    let url = e.url().map(|u| u.to_string()).unwrap_or_default();
                    return KgetError::NotFound(url);
                }
            }
        }
        KgetError::Network(e.to_string())
    }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for KgetError {
    fn from(e: Box<dyn std::error::Error + Send + Sync>) -> Self {
        let msg = e.to_string();
        if msg.to_lowercase().contains("cancel") {
            KgetError::Cancelled
        } else {
            KgetError::Other(msg)
        }
    }
}

impl From<String> for KgetError {
    fn from(s: String) -> Self { KgetError::Other(s) }
}

impl From<&str> for KgetError {
    fn from(s: &str) -> Self { KgetError::Other(s.to_string()) }
}
