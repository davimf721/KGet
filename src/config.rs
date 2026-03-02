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
//! # Example
//!
//! ```rust
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
use std::path::PathBuf;
use std::fs;
use std::io;
use dirs::config_dir;

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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// Whether to use the proxy for downloads
    pub enabled: bool,
    /// Proxy server URL (e.g., "http://proxy:8080" or "socks5://127.0.0.1:1080")
    pub url: Option<String>,
    /// Username for proxy authentication (optional)
    pub username: Option<String>,
    /// Password for proxy authentication (optional)
    pub password: Option<String>,
    /// Type of proxy protocol to use
    pub proxy_type: ProxyType,
}

/// Supported proxy protocol types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProxyType {
    /// HTTP proxy (for HTTP downloads)
    Http,
    /// HTTPS proxy (for HTTPS downloads)
    Https,
    /// SOCKS5 proxy (works with all protocols)
    Socks5,
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
/// Controls compression, caching, speed limiting, and parallel connections.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationConfig {
    /// Enable automatic compression for cached downloads
    pub compression: bool,
    /// Compression level (1-9). Affects algorithm selection:
    /// - 1-3: Gzip (fast)
    /// - 4-6: LZ4 (balanced)
    /// - 7-9: Brotli (high compression)
    pub compression_level: u8,
    /// Enable download caching
    pub cache_enabled: bool,
    /// Directory for cached files (default: ~/.cache/kget)
    pub cache_dir: String,
    /// Speed limit in bytes per second (None = unlimited)
    pub speed_limit: Option<u64>,
    /// Maximum parallel connections per download (1-32)
    pub max_connections: usize,
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
    #[serde(default = "default_torrent_max_peer_connections")]
    pub max_peer_connections: u32,
    /// Maximum upload slots per torrent
    #[serde(default = "default_torrent_max_upload_slots")]
    pub max_upload_slots: u32,
}

/// Configuration for FTP downloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FtpConfig {
    /// Use passive mode for FTP connections (recommended for NAT/firewall)
    pub passive_mode: bool,
    /// Default FTP port (21)
    pub default_port: u16,
}

/// Configuration for SFTP (SSH File Transfer Protocol) downloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SftpConfig {
    /// Default SFTP port (22)
    pub default_port: u16,
    /// Path to SSH private key file for authentication
    pub key_path: Option<String>,
}

/// Main configuration structure containing all KGet settings.
///
/// This is the top-level configuration object that aggregates
/// all protocol-specific and feature configurations.
///
/// # Loading Configuration
///
/// ```rust
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
}

impl Config {
    /// Load configuration from the standard config file location.
    ///
    /// If the config file doesn't exist, returns `Config::default()`.
    ///
    /// # Errors
    ///
    /// Returns an error if the config file exists but cannot be read or parsed.
    pub fn load() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let config_path = Self::get_config_path()?;
        
        if !config_path.exists() {
            // If the config file does not exist, return default config
            return Ok(Self::default());
        }
        
        let config_str = fs::read_to_string(config_path)?;
        // The error occurs here if the existing JSON file does not have the field.
        let config: Config = serde_json::from_str(&config_str)?;

        Ok(config)
    }
    
    /// Save the current configuration to the standard config file location.
    ///
    /// Creates the config directory if it doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the config file cannot be written.
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let config_path = Self::get_config_path()?;

        // Create config directory if it doesn't exist
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        
        let config_str = serde_json::to_string_pretty(self)?;
        fs::write(config_path, config_str)?;
        
        Ok(())
    }
    
    fn get_config_path() -> Result<PathBuf, io::Error> {
        let mut path = config_dir().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "Not able to find config directory")
        })?;
        
        path.push("kget");
        path.push("config.json");
        
        Ok(path)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            proxy: ProxyConfig {
                enabled: false,
                url: None,
                username: None,
                password: None,
                proxy_type: ProxyType::Http,
            },
            optimization: OptimizationConfig {
                compression: true,
                compression_level: 6,
                cache_enabled: true,
                cache_dir: "~/.cache/kget".to_string(),
                speed_limit: None,
                max_connections: 4,
            },
            torrent: TorrentConfig {
                enabled: false,
                download_dir: None,
                max_peers: 50,
                max_seeds: 25,
                port: None,
                dht_enabled: true,
                max_peer_connections: default_torrent_max_peer_connections(),
                max_upload_slots: default_torrent_max_upload_slots(),
            },
            ftp: FtpConfig {
                passive_mode: true,
                default_port: 21,
            },
            sftp: SftpConfig {
                default_port: 22,
                key_path: None,
            },
        }
    }
}
