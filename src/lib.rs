//! KGet - A powerful download library for Rust
//!
//! `kget` provides robust downloading capabilities including HTTP/HTTPS,
pub mod download;
pub mod advanced_download;
pub mod config;
pub mod optimization;
pub mod utils;
pub mod progress;
pub mod ftp;
pub mod sftp;
pub mod torrent;

// Re-exports para facilitar o uso por terceiros (kget::Config em vez de kget::config::Config)
pub use config::{Config, ProxyConfig, ProxyType};
pub use optimization::Optimizer;
pub use download::{download, verify_iso_integrity};
pub use advanced_download::AdvancedDownloader;
pub use progress::create_progress_bar; // Permite usar sua barra de progresso customizada

/// Opções que controlam o comportamento do download tanto na Lib quanto no CLI
#[derive(Debug, Clone)]
pub struct DownloadOptions {
    pub quiet_mode: bool,
    pub output_path: Option<String>,
    /// Se true, executa a verificação de hash imediatamente após o download do ISO
    pub verify_iso: bool,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            quiet_mode: false,
            output_path: None,
            verify_iso: false,
        }
    }
}