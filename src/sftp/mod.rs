//! SFTP (SSH File Transfer Protocol) download support.
//!
//! This module provides secure file downloads over SSH using SFTP.
//!
//! # Security
//!
//! SFTP provides encrypted file transfer, unlike plain FTP.
//! Authentication is handled via SSH (password or key-based).
//!
//! # Example
//!
//! ```rust,no_run
//! use kget::sftp::SftpDownloader;
//! use kget::{ProxyConfig, Optimizer};
//!
//! let downloader = SftpDownloader::new(
//!     "sftp://user@server.com:22/path/to/file".to_string(),
//!     "local_file.txt".to_string(),
//!     false,
//!     ProxyConfig::default(),
//!     Optimizer::new(),
//! );
//!
//! downloader.download().unwrap();
//! ```

use std::error::Error;
use std::io::Read;
use crate::config::ProxyConfig;
use crate::optimization::Optimizer;

/// SFTP file downloader using SSH.
///
/// Downloads files securely over SSH File Transfer Protocol.
///
/// # Note
///
/// Currently requires the SSH session to be established manually.
/// For key-based authentication, configure `SftpConfig.key_path` in
/// the application config.
pub struct SftpDownloader {
    url: String,
    output: String,
    quiet: bool,
    #[allow(dead_code)]
    proxy: ProxyConfig,
    #[allow(dead_code)]
    optimizer: Optimizer,
}

impl SftpDownloader {
    /// Create a new SFTP downloader.
    ///
    /// # Arguments
    ///
    /// * `url` - SFTP URL (e.g., "sftp://user@host/path")
    /// * `output` - Local path to save the file
    /// * `quiet` - Suppress console output
    /// * `proxy` - Proxy configuration
    /// * `optimizer` - Optimizer instance
    pub fn new(url: String, output: String, quiet: bool, proxy: ProxyConfig, optimizer: Optimizer) -> Self {
        Self {
            url,
            output,
            quiet,
            proxy,
            optimizer,
        }
    }

    pub fn download(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let tcp = std::net::TcpStream::connect(&self.url)?;
        let mut sess = ssh2::Session::new()?;
        sess.set_tcp_stream(tcp);
        sess.handshake()?;
        
        let sftp = sess.sftp()?;
        let mut remote_file = sftp.open(std::path::Path::new(&self.url))?;
        let mut contents = Vec::new();
        remote_file.read_to_end(&mut contents)?;
        
        std::fs::write(&self.output, contents)?;
        
        if !self.quiet {
            println!("Downloaded {} to {}", self.url, self.output);
        }
        Ok(())
    }
}