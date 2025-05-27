//! KGet - A powerful download library for Rust
//!
//! `kget` provides robust downloading capabilities including HTTP/HTTPS,
//! FTP, SFTP, and torrent downloads with progress tracking, proxy support,
//! and various optimizations.

mod config;
mod download;
mod advanced_download;
mod progress;
mod utils;
mod optimization;
mod ftp;
mod sftp;
mod torrent;

// Re-export public API
pub use crate::config::{Config, ProxyConfig, ProxyType, OptimizationConfig, TorrentConfig, FtpConfig, SftpConfig};
pub use crate::optimization::Optimizer;
pub use crate::progress::create_progress_bar;

/// Main download client for the KGet library
pub struct KGet {
    config: Config,
}

impl KGet {
    /// Create a new KGet client with default configuration
    pub fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let config = Config::load()?;
        Ok(Self { config })
    }

    /// Create a new KGet client with custom configuration
    pub fn with_config(config: Config) -> Self {
        Self { config }
    }

    /// Download a file from a URL to a local path
    pub fn download(
        &self,
        url: &str,
        output_path: Option<String>,
        quiet_mode: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Create optimizer from config
        let optimizer = Optimizer::new(self.config.optimization.clone());

        if url.starts_with("ftp://") {
            use crate::ftp::FtpDownloader;
            let downloader = FtpDownloader::new(
                url.to_string(), 
                output_path.unwrap_or_else(|| crate::utils::get_filename_from_url_or_default(url, "ftp_output")), 
                quiet_mode, 
                self.config.proxy.clone(),
                optimizer,
            );
            return downloader.download();
        } else if url.starts_with("sftp://") {
            use crate::sftp::SftpDownloader;
            let downloader = SftpDownloader::new(
                url.to_string(),
                output_path.unwrap_or_else(|| crate::utils::get_filename_from_url_or_default(url, "sftp_output")),
                quiet_mode,
                self.config.proxy.clone(),
                optimizer,
            );
            return downloader.download();
        } else if url.starts_with("magnet:?") {
            use crate::torrent::TorrentDownloader;
            let downloader = TorrentDownloader::new(
                url.to_string(),
                output_path.unwrap_or_else(|| "torrent_output".to_string()),
                quiet_mode,
                self.config.proxy.clone(),
                optimizer,
            );
            
            // Create tokio runtime for async torrent downloads
            let runtime = tokio::runtime::Runtime::new()?;
            return runtime.block_on(downloader.download());
        } else {
            // Regular HTTP/HTTPS download
            return download::download(url, quiet_mode, output_path, self.config.proxy.clone(), optimizer);
        }
    }

    /// Advanced download with parallel chunks and resumable capability
    pub fn advanced_download(
        &self,
        url: &str,
        output_path: Option<String>,
        quiet_mode: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let optimizer = Optimizer::new(self.config.optimization.clone());
        
        // Use the existing advanced_download implementation
        let downloader = crate::advanced_download::AdvancedDownloader::new(
            url.to_string(),
            output_path.unwrap_or_else(|| crate::utils::get_filename_from_url_or_default(url, "advanced_output")),
            quiet_mode,
            self.config.proxy.clone(),
            optimizer,
        );
        
        downloader.download()
    }

    /// Get current configuration
    pub fn get_config(&self) -> &Config {
        &self.config
    }
    
    /// Update configuration
    pub fn set_config(&mut self, config: Config) {
        self.config = config;
    }
}

/// Custom progress callback type for integration with other libraries
pub type ProgressCallback = Box<dyn Fn(u64, u64, f64) -> () + Send + Sync>;

/// Download options for fine-tuning the download process
pub struct DownloadOptions {
    pub quiet_mode: bool,
    pub retry_count: Option<u32>,
    pub retry_delay: Option<std::time::Duration>,
    pub progress_callback: Option<ProgressCallback>,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            quiet_mode: false,
            retry_count: Some(3),
            retry_delay: Some(std::time::Duration::from_secs(2)),
            progress_callback: None,
        }
    }
}

/// Simplified API for quick downloads without creating a KelpsGet instance
pub mod simple {
    use super::*;

    /// Download a file with minimal configuration
    pub fn download(
        url: &str, 
        output_path: Option<&str>
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let client = KGet::new()?;
        client.download(
            url,
            output_path.map(|s| s.to_string()),
            false
        )
    }

    /// Download a file with custom options
    pub fn download_with_options(
        url: &str, 
        output_path: Option<&str>,
        options: DownloadOptions
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let client = KGet::new()?;

       
        
        client.download(
            url,
            output_path.map(|s| s.to_string()),
            options.quiet_mode
        )
    }
}