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

    /// The server answered with an HTTP error status (other than 404).
    Http {
        /// HTTP status code (e.g. 403, 503).
        status: u16,
        /// URL that produced the error.
        url: String,
    },

    /// A checksum sidecar file could not be fetched or parsed.
    SidecarError(String),

    /// Catch-all for errors that don't fit a more specific variant.
    Other(String),
}

impl fmt::Display for KgetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KgetError::Network(e) => write!(f, "Network error: {e}"),
            KgetError::Io(e) => write!(f, "IO error: {e}"),
            KgetError::ChecksumMismatch {
                algorithm,
                expected,
                got,
            } => write!(f, "{algorithm} mismatch — expected {expected}, got {got}"),
            KgetError::Protocol(e) => write!(f, "Protocol error: {e}"),
            KgetError::Cancelled => write!(f, "Download cancelled"),
            KgetError::NotFound(url) => write!(f, "HTTP 404 Not Found: {url}"),
            KgetError::Http { status, url } => write!(f, "HTTP error {status} for {url}"),
            KgetError::SidecarError(e) => write!(f, "Checksum sidecar error: {e}"),
            KgetError::Other(e) => write!(f, "{e}"),
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

impl KgetError {
    /// Build the error for an HTTP status code (404 becomes [`KgetError::NotFound`]).
    pub fn from_status(status: u16, url: impl Into<String>) -> Self {
        let url = url.into();
        if status == 404 {
            KgetError::NotFound(url)
        } else {
            KgetError::Http { status, url }
        }
    }

    /// Whether a retry may succeed, according to `retry_on_status`.
    ///
    /// HTTP errors are retried only when their status is listed; network and
    /// generic I/O-free failures are retried; everything else is permanent.
    pub fn is_retryable(&self, retry_on_status: &[u16]) -> bool {
        match self {
            KgetError::Http { status, .. } => retry_on_status.contains(status),
            KgetError::Network(_) | KgetError::Other(_) => true,
            _ => false,
        }
    }

    /// Stable machine-readable code (used by the JSONL `error` event).
    pub fn code(&self) -> String {
        match self {
            KgetError::Network(_) => "network".into(),
            KgetError::Io(_) => "io".into(),
            KgetError::ChecksumMismatch { .. } => "checksum_mismatch".into(),
            KgetError::Protocol(_) => "protocol".into(),
            KgetError::Cancelled => "cancelled".into(),
            KgetError::NotFound(_) => "http_404".into(),
            KgetError::Http { status, .. } => format!("http_{status}"),
            KgetError::SidecarError(_) => "sidecar".into(),
            KgetError::Other(_) => "error".into(),
        }
    }
}

// ── From impls ────────────────────────────────────────────────────────────────

impl From<std::io::Error> for KgetError {
    fn from(e: std::io::Error) -> Self {
        KgetError::Io(e)
    }
}

impl From<reqwest::Error> for KgetError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_status()
            && let Some(s) = e.status()
        {
            let url = e.url().map(|u| u.to_string()).unwrap_or_default();
            return KgetError::from_status(s.as_u16(), url);
        }
        KgetError::Network(e.to_string())
    }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for KgetError {
    fn from(e: Box<dyn std::error::Error + Send + Sync>) -> Self {
        let e = match e.downcast::<KgetError>() {
            Ok(k) => return *k,
            Err(e) => e,
        };
        let e = match e.downcast::<std::io::Error>() {
            Ok(io) => return KgetError::Io(*io),
            Err(e) => e,
        };
        let e = match e.downcast::<reqwest::Error>() {
            Ok(r) => return KgetError::from(*r),
            Err(e) => e,
        };
        let msg = e.to_string();
        if msg == "Download cancelled" {
            KgetError::Cancelled
        } else {
            KgetError::Other(msg)
        }
    }
}

impl From<String> for KgetError {
    fn from(s: String) -> Self {
        KgetError::Other(s)
    }
}

impl From<&str> for KgetError {
    fn from(s: &str) -> Self {
        KgetError::Other(s.to_string())
    }
}
