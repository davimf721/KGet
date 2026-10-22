//! Configuration management for KGet.
//!
//! This module provides configuration structures for all KGet features:
//! proxy settings, optimization parameters, torrent options, and protocol-specific configs.
//!
//! Configuration is stored in JSON format at:
//! - macOS: `~/Library/Application Support/kget/config.json`
//! - Linux: `~/.config/kget/config.json`
//! - Windows: `%APPDATA%\kget\config.json`
//!
//! Set the `KGET_CONFIG_DIR` environment variable to use a different directory
//! (useful for tests, portable installs and sandboxed frontends).
//!
//! Missing fields in an existing `config.json` fall back to their defaults, and a
//! corrupt file is backed up to `config.json.bak` instead of preventing startup.
//!
//! # Example
//!
//! ```rust,no_run
//! use kget::Config;
//!
//! // Load existing config or create default
//! let config = Config::load().unwrap_or_default();
//!
//! // Modify and save
//! let mut config = config;
//! config.optimization.max_connections = 8;
//! config.save().unwrap();
//! ```

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;

/// Proxy configuration for routing downloads through a proxy server.
///
/// Supports HTTP, HTTPS, and SOCKS5 proxies with optional authentication.
///
/// # Example
///
/// ```rust
/// use kget::{ProxyConfig, ProxyType};
///
/// let proxy = ProxyConfig {
///     enabled: true,
///     url: Some("http://proxy.example.com:8080".to_string()),
///     username: Some("user".to_string()),
///     password: Some("pass".to_string()),
///     proxy_type: ProxyType::Http,
/// };
/// ```
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProxyConfig {
    /// Whether to use the proxy for downloads
    pub enabled: bool,
    /// Proxy server URL (e.g., "http://proxy:8080" or "socks5://127.0.0.1:1080")
    pub url: Option<String>,
    /// Username for proxy authentication (optional)
    pub username: Option<String>,
    /// Password for proxy authentication (optional). Never written to
    /// `config.json`: `kget --save-config` keeps it in the system keychain
    /// (see [`crate::credentials`]).
    #[serde(skip_serializing)]
    pub password: Option<String>,
    /// Type of proxy protocol to use
    pub proxy_type: ProxyType,
}

/// Supported proxy protocol types.
///
/// The type describes how KGet talks **to the proxy**; every type is used for
/// both `http://` and `https://` downloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProxyType {
    /// Proxy that speaks plain HTTP (`http://proxy:3128`)
    Http,
    /// Proxy reached over TLS (`https://proxy:443`)
    Https,
    /// SOCKS5 proxy (`socks5://host:1080`); DNS is resolved by the proxy
    Socks5,
}

impl ProxyType {
    /// Parse a proxy type name (`http`, `https`, `socks5`, `socks5h`, `socks`).
    /// Unknown values map to [`ProxyType::Http`].
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "https" => ProxyType::Https,
            "socks5" | "socks5h" | "socks" => ProxyType::Socks5,
            _ => ProxyType::Http,
        }
    }

    /// Infer the proxy type from the scheme of a proxy URL.
    pub fn from_url(url: &str) -> Self {
        let lower = url.trim().to_ascii_lowercase();
        if lower.starts_with("socks") {
            ProxyType::Socks5
        } else if lower.starts_with("https://") {
            ProxyType::Https
        } else {
            ProxyType::Http
        }
    }
}

impl std::fmt::Debug for ProxyConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyConfig")
            .field("enabled", &self.enabled)
            .field("url", &self.url)
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("proxy_type", &self.proxy_type)
            .finish()
    }
}

impl Default for ProxyConfig {
    /// Create a disabled proxy configuration.
    fn default() -> Self {
        Self {
            enabled: false,
            url: None,
            username: None,
            password: None,
            proxy_type: ProxyType::Http,
        }
    }
}

/// Configuration for download optimization features.
///
/// Controls speed limiting and parallel connections.
///
/// `compression`, `compression_level`, `cache_enabled` and `cache_dir` are kept
/// only so existing `config.json` files keep loading; they have no effect.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OptimizationConfig {
    /// Unused since 1.7.1 (kept for config compatibility)
    pub compression: bool,
    /// Unused since 1.7.1 (kept for config compatibility)
    pub compression_level: u8,
    /// Unused since 1.7.1 (kept for config compatibility)
    pub cache_enabled: bool,
    /// Unused since 1.7.1 (kept for config compatibility)
    pub cache_dir: String,
    /// Speed limit in bytes per second (None = unlimited)
    pub speed_limit: Option<u64>,
    /// Maximum parallel connections per download (1-32)
    pub max_connections: usize,
}

impl Default for OptimizationConfig {
    fn default() -> Self {
        Self {
            compression: false,
            compression_level: 6,
            cache_enabled: false,
            cache_dir: "~/.cache/kget".to_string(),
            speed_limit: None,
            max_connections: 4,
        }
    }
}

// Function to provide the default value for max_peer_connections
fn default_torrent_max_peer_connections() -> u32 {
    50
}

// Function to provide the default value for max_upload_slots
fn default_torrent_max_upload_slots() -> u32 {
    4
}

/// Configuration for BitTorrent downloads.
///
/// These settings apply when using the native torrent client (`torrent-native` feature)
/// or the Transmission RPC integration (`torrent-transmission` feature).
///
/// # Example
///
/// ```rust
/// use kget::Config;
///
/// let mut config = Config::default();
/// config.torrent.enabled = true;
/// config.torrent.download_dir = Some("/home/user/Downloads".to_string());
/// config.torrent.max_peers = 100;
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TorrentConfig {
    /// Enable torrent download support
    pub enabled: bool,
    /// Default download directory (None = use current directory)
    pub download_dir: Option<String>,
    /// Maximum number of peers to connect to
    pub max_peers: usize,
    /// Maximum number of seeds to upload to
    pub max_seeds: usize,
    /// Custom listen port for incoming connections (None = random)
    pub port: Option<u16>,
    /// Enable DHT (Distributed Hash Table) for peer discovery
    pub dht_enabled: bool,
    /// Maximum peer connections per torrent
    pub max_peer_connections: u32,
    /// Maximum upload slots per torrent
    pub max_upload_slots: u32,
}

impl Default for TorrentConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            download_dir: None,
            max_peers: 50,
            max_seeds: 25,
            port: None,
            dht_enabled: true,
            max_peer_connections: default_torrent_max_peer_connections(),
            max_upload_slots: default_torrent_max_upload_slots(),
        }
    }
}

/// Configuration for FTP downloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FtpConfig {
    /// Use passive mode for FTP connections (recommended for NAT/firewall)
    pub passive_mode: bool,
    /// Default FTP port (21)
    pub default_port: u16,
}

impl Default for FtpConfig {
    fn default() -> Self {
        Self {
            passive_mode: true,
            default_port: 21,
        }
    }
}

/// Configuration for SFTP (SSH File Transfer Protocol) downloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SftpConfig {
    /// Default SFTP port (22)
    pub default_port: u16,
    /// Path to SSH private key file for authentication
    pub key_path: Option<String>,
}

impl Default for SftpConfig {
    fn default() -> Self {
        Self {
            default_port: 22,
            key_path: None,
        }
    }
}

/// Main configuration structure containing all KGet settings.
///
/// This is the top-level configuration object that aggregates
/// all protocol-specific and feature configurations.
///
/// # Loading Configuration
///
/// ```rust,no_run
/// use kget::Config;
///
/// // Load from file or use defaults
/// let config = Config::load().unwrap_or_default();
/// println!("Max connections: {}", config.optimization.max_connections);
/// ```
///
/// # Saving Configuration
///
/// ```rust,no_run
/// use kget::Config;
///
/// let mut config = Config::default();
/// config.optimization.max_connections = 16;
/// config.save().expect("Failed to save config");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Proxy server settings
    pub proxy: ProxyConfig,
    /// Download optimization settings
    pub optimization: OptimizationConfig,
    /// BitTorrent configuration
    pub torrent: TorrentConfig,
    /// FTP protocol configuration
    pub ftp: FtpConfig,
    /// SFTP protocol configuration
    pub sftp: SftpConfig,
    /// Explicit path to the yt-dlp binary (the `KGET_YTDLP_PATH` env var wins)
    pub ytdlp_path: Option<String>,
    /// Apply the smart rules ([`crate::rules`]) to every download
    /// (`--organize` / `--no-organize` override it for one run)
    pub rules_enabled: bool,
    /// Also apply the built-in rules (Videos, Music, Images, …) after the
    /// user rules when organizing
    pub builtin_rules: bool,
    /// Root folder that rule destinations are created in (`None` = the
    /// current directory)
    pub organize_dir: Option<String>,
    /// User-defined smart rules, in order: the first match wins
    pub rules: Vec<crate::rules::Rule>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            proxy: ProxyConfig::default(),
            optimization: OptimizationConfig::default(),
            torrent: TorrentConfig::default(),
            ftp: FtpConfig::default(),
            sftp: SftpConfig::default(),
            ytdlp_path: None,
            rules_enabled: false,
            builtin_rules: true,
            organize_dir: None,
            rules: Vec::new(),
        }
    }
}

/// Directory holding `config.json` and `history.db`.
///
/// Honours `KGET_CONFIG_DIR`; otherwise `<OS config dir>/kget`.
pub fn config_dir() -> Result<PathBuf, io::Error> {
    if let Some(dir) = std::env::var_os("KGET_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    dirs::config_dir()
        .map(|d| d.join("kget"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Not able to find config directory"))
}

/// Write `contents` to `path` atomically (temporary file + rename).
pub(crate) fn write_atomic(
    path: &std::path::Path,
    contents: &[u8],
    private: bool,
) -> io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(format!(".{}.tmp", std::process::id()));
    let tmp = path.with_file_name(tmp_name);
    fs::write(&tmp, contents)?;
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = private;
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

impl Config {
    /// Load configuration from the standard config file location.
    ///
    /// Returns `Config::default()` if the file doesn't exist. Missing fields use
    /// their defaults. A file that cannot be parsed is copied to
    /// `config.json.bak`, a warning is printed to stderr, and defaults are used.
    pub fn load() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let config_path = Self::path()?;
        let Ok(config_str) = fs::read_to_string(&config_path) else {
            return Ok(Self::default());
        };
        if config_str.trim().is_empty() {
            return Ok(Self::default());
        }
        match serde_json::from_str(&config_str) {
            Ok(config) => Ok(config),
            Err(e) => {
                let backup = config_path.with_extension("json.bak");
                let _ = fs::copy(&config_path, &backup);
                eprintln!(
                    "Warning: invalid config {} ({e}); using defaults. Backup: {}",
                    config_path.display(),
                    backup.display()
                );
                Ok(Self::default())
            }
        }
    }

    /// Save the current configuration to the standard config file location.
    ///
    /// The write is atomic and, on Unix, the file is only readable by the owner
    /// (it may contain proxy credentials).
    ///
    /// # Errors
    ///
    /// Returns an error if the config file cannot be written.
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let config_path = Self::path()?;
        let config_str = serde_json::to_string_pretty(self)?;
        write_atomic(&config_path, config_str.as_bytes(), true)?;
        Ok(())
    }

    /// Full path of `config.json` (see [`config_dir`]).
    pub fn path() -> Result<PathBuf, io::Error> {
        Ok(config_dir()?.join("config.json"))
    }
}
