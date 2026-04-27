//! FTP download support.
//!
//! This module provides FTP file downloads with:
//! - Anonymous and authenticated access
//! - Binary transfer mode for all file types
//! - Progress tracking
//! - SOCKS5 proxy support
//!
//! # Example
//!
//! ```rust,no_run
//! use kget::ftp::FtpDownloader;
//! use kget::{ProxyConfig, Optimizer};
//!
//! let downloader = FtpDownloader::new(
//!     "ftp://ftp.example.com/pub/file.zip".to_string(),
//!     "file.zip".to_string(),
//!     false,
//!     ProxyConfig::default(),
//!     Optimizer::new(),
//! );
//!
//! downloader.download().unwrap();
//! ```

mod client;

pub use client::FtpDownloader;
