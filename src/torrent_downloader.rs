use std::error::Error;
use std::path::Path;

use crate::config::{ProxyConfig, TorrentConfig};
use crate::optimization::Optimizer;
use crate::progress::create_progress_bar;
use crate::torrent::download_torrent;
use crate::utils::print;

/// Estrutura para gerenciar downloads via torrent
pub struct TorrentDownloader {
    url: String,
    output_path: String,
    quiet_mode: bool,
    proxy: ProxyConfig,
    optimizer: Optimizer,
    torrent_config: TorrentConfig,
}

impl TorrentDownloader {
    /// Cria uma nova instância de TorrentDownloader
    pub fn new(
        url: String,
        output_path: String,
        quiet_mode: bool,
        proxy: ProxyConfig,
        optimizer: Optimizer,
        torrent_config: TorrentConfig,
    ) -> Self {
        Self {
            url,
            output_path,
            quiet_mode,
            proxy,
            optimizer,
            torrent_config,
        }
    }

    /// Inicia o download via torrent
    pub fn download(&self) -> Result<(), Box<dyn Error>> {
        if !self.url.starts_with("magnet:?") {
            return Err("URL não é um magnetic link válido".into());
        }

        download_torrent(
            &self.url,
            self.quiet_mode,
            Some(self.output_path.clone()),
            self.proxy.clone(),
            self.optimizer.clone(),
            self.torrent_config.clone(),
        )
    }
}
