//! Shared HTTP client construction and request helpers.
//!
//! Every HTTP-based downloader (simple, turbo, Metalink, builder, checksum
//! sidecars) builds its client here, so proxy handling, timeouts and the
//! user agent behave the same everywhere.
//!
//! # Proxies
//!
//! The [`ProxyType`] describes how KGet talks to the proxy; the proxy is used
//! for **both** `http://` and `https://` targets. A proxy that is enabled but
//! invalid is an error — KGet never silently falls back to a direct connection.

use crate::config::{ProxyConfig, ProxyType};
use crate::error::KgetError;
use reqwest::blocking::{Client, RequestBuilder, Response};
use std::time::Duration;

/// User agent sent with every request.
pub const USER_AGENT: &str = concat!("KGet/", env!("CARGO_PKG_VERSION"));

/// Default number of attempts per request.
pub const DEFAULT_RETRIES: u32 = 3;

/// Default I/O timeout in seconds.
pub const DEFAULT_TIMEOUT_SECS: u64 = 60;

/// Timeouts applied to a client.
#[derive(Debug, Clone, Copy)]
pub struct ClientOptions {
    /// Maximum time to establish a TCP/TLS connection.
    pub connect_timeout: Duration,
    /// Timeout for each operation (sending the request and each read), not the
    /// whole transfer. `None` disables it.
    pub io_timeout: Option<Duration>,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(20),
            io_timeout: Some(Duration::from_secs(DEFAULT_TIMEOUT_SECS)),
        }
    }
}

impl ClientOptions {
    /// Options with the same value used for the connect and I/O timeouts
    /// (`0` disables the I/O timeout).
    pub fn with_timeout_secs(secs: u64) -> Self {
        if secs == 0 {
            return Self {
                io_timeout: None,
                ..Self::default()
            };
        }
        Self {
            connect_timeout: Duration::from_secs(secs.min(60)),
            io_timeout: Some(Duration::from_secs(secs)),
        }
    }
}

/// Build a blocking client honouring the proxy configuration.
///
/// # Errors
///
/// Returns [`KgetError::Protocol`] when the proxy is enabled without a URL or
/// the URL is invalid.
pub fn build_client(proxy: &ProxyConfig, opts: &ClientOptions) -> Result<Client, KgetError> {
    let mut b = Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(opts.connect_timeout)
        .timeout(opts.io_timeout);

    if proxy.enabled {
        let raw = proxy
            .url
            .as_deref()
            .filter(|u| !u.trim().is_empty())
            .ok_or_else(|| KgetError::Protocol("Proxy is enabled but no URL is set".into()))?;
        let url = normalize_proxy_url(raw, proxy.proxy_type);
        // Proxy::all covers http:// and https:// targets; the proxy's own
        // protocol comes from the scheme of its URL.
        let mut p = reqwest::Proxy::all(&url)
            .map_err(|e| KgetError::Protocol(format!("Invalid proxy '{url}': {e}")))?;
        if let (Some(u), Some(pw)) = (&proxy.username, &proxy.password) {
            p = p.basic_auth(u, pw);
        }
        b = b.proxy(p);
    }

    b.build()
        .map_err(|e| KgetError::Protocol(format!("Failed to create HTTP client: {e}")))
}

/// Add a scheme to a bare `host:port` proxy address according to its type.
pub fn normalize_proxy_url(url: &str, kind: ProxyType) -> String {
    let url = url.trim();
    if url.contains("://") {
        return url.to_string();
    }
    match kind {
        ProxyType::Http => format!("http://{url}"),
        ProxyType::Https => format!("https://{url}"),
        // socks5h: DNS is resolved by the proxy (no DNS leak)
        ProxyType::Socks5 => format!("socks5h://{url}"),
    }
}

/// Apply user-supplied headers, skipping names/values that are not valid HTTP.
pub fn apply_headers(mut req: RequestBuilder, headers: &[(String, String)]) -> RequestBuilder {
    for (name, value) in headers {
        if let (Ok(n), Ok(v)) = (
            reqwest::header::HeaderName::from_bytes(name.as_bytes()),
            reqwest::header::HeaderValue::from_str(value),
        ) {
            req = req.header(n, v);
        }
    }
    req
}

/// Delay requested by a `Retry-After: <seconds>` header (capped at 5 minutes).
pub fn retry_after(resp: &Response) -> Option<Duration> {
    resp.headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(|s| Duration::from_secs(s.min(300)))
}

/// Exponential backoff: 500 ms, 1 s, 2 s, … capped at 30 s.
pub fn backoff_delay(attempt: u32) -> Duration {
    let ms = 500u64.saturating_mul(1u64 << attempt.saturating_sub(1).min(6));
    Duration::from_millis(ms.min(30_000))
}

/// Whether a status code is worth retrying (rate limiting, timeouts, 5xx).
pub fn is_transient_status(status: u16) -> bool {
    matches!(status, 408 | 429) || (500..=599).contains(&status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_bare_proxy_addresses() {
        assert_eq!(normalize_proxy_url("h:1", ProxyType::Http), "http://h:1");
        assert_eq!(
            normalize_proxy_url("h:1", ProxyType::Socks5),
            "socks5h://h:1"
        );
        assert_eq!(
            normalize_proxy_url("socks5://h:1", ProxyType::Http),
            "socks5://h:1"
        );
    }

    #[test]
    fn enabled_proxy_without_url_is_an_error() {
        let proxy = ProxyConfig {
            enabled: true,
            ..ProxyConfig::default()
        };
        assert!(build_client(&proxy, &ClientOptions::default()).is_err());
    }

    #[test]
    fn socks_proxy_is_accepted() {
        let proxy = ProxyConfig {
            enabled: true,
            url: Some("socks5://127.0.0.1:1080".into()),
            proxy_type: ProxyType::Socks5,
            ..ProxyConfig::default()
        };
        assert!(build_client(&proxy, &ClientOptions::default()).is_ok());
    }

    #[test]
    fn backoff_grows_and_caps() {
        assert_eq!(backoff_delay(1), Duration::from_millis(500));
        assert_eq!(backoff_delay(2), Duration::from_secs(1));
        assert!(backoff_delay(20) <= Duration::from_secs(30));
    }
}
