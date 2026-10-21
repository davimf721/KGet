//! Terminal helpers shared by the `kget` command line and interactive mode.
//!
//! Per-run configuration overrides (`--proxy`, `--limit`, torrent flags),
//! `--save-config`, `--doctor`, and the history commands live here so the binary only parses
//! arguments and launches modes.

use crate::config::{Config, ProxyType};
use crate::queue::{DownloadHistory, EntryStatus};
use crate::utils;
use crate::ytdlp::ytdlp_binary;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Settings given on the command line that override the saved
/// configuration for one run (never persisted unless the caller saves).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigOverrides {
    /// Proxy URL (enables the proxy)
    pub proxy: Option<String>,
    /// Proxy user name
    pub proxy_user: Option<String>,
    /// Proxy password
    pub proxy_pass: Option<String>,
    /// Proxy type (`http`, `https`, `socks5`); inferred from the URL when omitted
    pub proxy_type: Option<String>,
    /// Speed limit in bytes/second (0 = unlimited)
    pub speed_limit: Option<u64>,
    /// Enable torrent support
    pub torrent: bool,
    /// Maximum number of torrent peers
    pub max_peers: Option<usize>,
    /// Maximum number of torrent seeds
    pub max_seeds: Option<usize>,
    /// Torrent listening port
    pub torrent_port: Option<u16>,
    /// Disable DHT
    pub no_dht: bool,
}

impl ConfigOverrides {
    /// Apply the overrides to `config`.
    pub fn apply(&self, config: &mut Config) {
        if let Some(url) = &self.proxy {
            config.proxy.enabled = true;
            config.proxy.url = Some(url.clone());
            config.proxy.proxy_type = match self.proxy_type.as_deref() {
                Some(t) => ProxyType::parse(t),
                None => ProxyType::from_url(url),
            };
        } else if let Some(t) = self.proxy_type.as_deref() {
            config.proxy.proxy_type = ProxyType::parse(t);
        }
        if let Some(u) = &self.proxy_user {
            config.proxy.username = Some(u.clone());
        }
        if let Some(p) = &self.proxy_pass {
            config.proxy.password = Some(p.clone());
        }
        if let Some(l) = self.speed_limit {
            config.optimization.speed_limit = (l > 0).then_some(l);
        }
        if self.torrent {
            config.torrent.enabled = true;
        }
        if let Some(v) = self.max_peers {
            config.torrent.max_peers = v;
        }
        if let Some(v) = self.max_seeds {
            config.torrent.max_seeds = v;
        }
        if let Some(v) = self.torrent_port {
            config.torrent.port = Some(v);
        }
        if self.no_dht {
            config.torrent.dht_enabled = false;
        }
    }

    /// A copy of `config` with the overrides applied.
    pub fn applied_to(&self, config: &Config) -> Config {
        let mut config = config.clone();
        self.apply(&mut config);
        config
    }
}

/// Print diagnostics (`kget --doctor`, interactive `doctor`).
pub fn print_doctor(config: &Config) {
    println!("KGet {}", env!("CARGO_PKG_VERSION"));
    println!(
        "OS:       {} ({})",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    match crate::config::config_dir() {
        Ok(d) => println!("Config:   {}", d.display()),
        Err(e) => println!("Config:   unavailable ({e})"),
    }
    println!("PATH:     {}", std::env::var("PATH").unwrap_or_default());
    match ytdlp_binary() {
        Some(p) => println!("yt-dlp:   {p}"),
        None => println!(
            "yt-dlp:   NOT FOUND\n{}",
            crate::ytdlp::ytdlp_not_found_message()
        ),
    }
    for tool in ["unzip", "tar", "7z"] {
        match utils::which_in_path(tool) {
            Some(p) => println!("{tool:<9} {}", p.display()),
            None => println!("{tool:<9} missing (needed for --extract)"),
        }
    }
    println!(
        "Features: gui={} torrent-native={} torrent-transmission={} keychain={}",
        cfg!(feature = "gui"),
        cfg!(feature = "torrent-native"),
        cfg!(feature = "torrent-transmission"),
        cfg!(feature = "keychain")
    );
    println!("Keychain: {}", crate::credentials::status());
    println!(
        "Proxy:    {}",
        if config.proxy.enabled {
            config.proxy.url.as_deref().unwrap_or("enabled, but no URL")
        } else {
            "disabled"
        }
    );
    println!(
        "Limit:    {}",
        config
            .optimization
            .speed_limit
            .map(|l| format!("{l} B/s"))
            .unwrap_or_else(|| "unlimited".into())
    );
    println!("Connections: {}", config.optimization.max_connections);
}

/// Save `config` to `config.json` (`kget --save-config`) and return its path.
///
/// The proxy password is never written to the file: it goes to the system
/// keychain (a warning is printed when that is not possible).
pub fn save_config(config: &Config) -> Result<std::path::PathBuf, BoxError> {
    if config.proxy.password.is_some() {
        match crate::credentials::store_proxy_password(&config.proxy) {
            Ok(t) => println!("Proxy password saved in the system keychain ({t})."),
            Err(e) => eprintln!("Warning: proxy password not saved: {e}"),
        }
    }
    let mut to_save = config.clone();
    to_save.proxy.password = None;
    to_save.save()?;
    Ok(Config::path()?)
}

/// Print the `limit` most recent history entries (`kget --history`).
pub fn print_history(limit: usize) {
    let history = DownloadHistory::load();
    let entries = history.recent(limit);
    if entries.is_empty() {
        println!("No download history.");
    } else {
        println!("{:<16} {:<22} {:<12} File", "ID", "Date (UTC)", "Status");
        println!("{}", "-".repeat(86));
        for e in entries {
            println!(
                "{:<16} {:<22} {:<12} {}",
                e.id,
                e.created_at_display(),
                e.status,
                e.filename
            );
        }
    }
}

/// Remove history entries and return how many were removed.
///
/// `scope`: `completed` (or `done`) keeps only failures, `failed` removes
/// failures, anything else clears the whole history.
pub fn clear_history(scope: &str) -> Result<usize, BoxError> {
    let mut removed = 0usize;
    DownloadHistory::update(|entries| {
        let before = entries.len();
        match scope {
            "completed" | "done" => entries.retain(|e| e.status == EntryStatus::Failed),
            "failed" => entries.retain(|e| e.status != EntryStatus::Failed),
            _ => entries.clear(),
        }
        removed = before - entries.len();
    })?;
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_apply_to_config() {
        let o = ConfigOverrides {
            proxy: Some("socks5://127.0.0.1:1080".into()),
            speed_limit: Some(0),
            max_peers: Some(7),
            no_dht: true,
            ..Default::default()
        };
        let mut base = Config::default();
        base.optimization.speed_limit = Some(10);
        let c = o.applied_to(&base);
        assert!(c.proxy.enabled);
        assert_eq!(c.proxy.proxy_type, ProxyType::Socks5);
        assert_eq!(c.optimization.speed_limit, None);
        assert_eq!(c.torrent.max_peers, 7);
        assert!(!c.torrent.dht_enabled);
        // An empty override changes nothing.
        let same = ConfigOverrides::default().applied_to(&base);
        assert_eq!(same.optimization.speed_limit, Some(10));
    }
}
